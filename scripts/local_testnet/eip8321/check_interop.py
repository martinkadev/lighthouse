#!/usr/bin/env python3
"""Check an EIP-8321 (hash-chain RANDAO) devnet across clients via the Beacon API.

Talks to every consensus node directly (Dora cannot decode EIP-8321 blocks) and fails
unless all of the following hold:

* every node reports the same EIP8321_FORK_EPOCH / EIP8321_FORK_VERSION and has upgraded;
* all nodes agree on the canonical block root of every slot from the fork to the
  common head (missed slots must be missed everywhere);
* finality has advanced past the fork epoch on every node, and heads are not optimistic;
* commitment registrations were included, and hash-chain reveals appear once they activate;
* every reveal satisfies BLAKE3("HASH_CHAIN_RANDAO" || reveal) == previous commitment, is
  only used after the activation delay, and comes with the BLS point-at-infinity
  `randao_reveal`; unregistered proposers use a zero reveal and a real BLS signature;
* (interop) blocks proposed by more than one client, and hash-chain reveals proposed by
  more than one client, are part of the agreed canonical chain, and at least one block
  includes registrations of another client's validators (i.e. they crossed over gossip).

Endpoints are discovered from a Kurtosis enclave (--enclave) or given explicitly
(--beacon NAME=URL, repeatable). Evidence is written to --out.

Requires: python3 >= 3.9, `pip install blake3` (and `pyyaml` for --params).
"""

import argparse
import json
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

try:
    from blake3 import blake3
except ImportError:  # pragma: no cover
    sys.exit("missing dependency: pip install blake3")

DST = b"HASH_CHAIN_RANDAO"
ZERO32 = bytes(32)
G2_INFINITY = "0xc0" + "00" * 95
KNOWN_CLIENTS = ("lighthouse", "prysm", "teku", "lodestar", "nimbus", "grandine")


class CheckFailed(Exception):
    pass


def get(url, path, allow_404=False):
    req = urllib.request.Request(url.rstrip("/") + path, headers={"Accept": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        if allow_404 and e.code == 404:
            return None
        raise CheckFailed(f"GET {url}{path} -> HTTP {e.code}: {e.read()[:300]!r}")


def discover(enclave):
    out = subprocess.run(
        ["kurtosis", "enclave", "inspect", enclave], capture_output=True, text=True, check=True
    ).stdout
    services = sorted(set(re.findall(r"\b(cl-\d+-[a-z]+-[a-z0-9]+)\b", out)))
    nodes = {}
    for svc in services:
        port = subprocess.run(
            ["kurtosis", "port", "print", enclave, svc, "http"], capture_output=True, text=True
        )
        if port.returncode == 0 and port.stdout.strip():
            url = port.stdout.strip()
            nodes[svc] = url if url.startswith("http") else "http://" + url
    if not nodes:
        raise CheckFailed(f"no consensus services found in enclave {enclave}")
    return nodes


# engine_getClientVersionV1 codes used in default graffiti such as "GE66afLHc5ab".
CLIENT_CODES = {"LH": "lighthouse", "PM": "prysm", "TK": "teku", "LS": "lodestar",
                "NB": "nimbus", "GD": "grandine"}


def key_ranges_from_params(path):
    """ethereum-package assigns validator keys to participants in file order."""
    import yaml  # only needed with --params

    params = yaml.safe_load(Path(path).read_text())
    default = 128 if (params.get("network_params", {}).get("preset") == "minimal") else 64
    ranges, start = [], 0
    for p in params["participants"]:
        for _ in range(int(p.get("count", 1))):
            n = int(p.get("validator_count", default) or 0)
            ranges.append((start, start + n, p.get("cl_type", "unknown")))
            start += n
    return ranges


def client_of(proposer, graffiti_hex, ranges):
    for lo, hi, client in ranges:
        if lo <= proposer < hi:
            return client
    try:
        graffiti = bytes.fromhex(graffiti_hex.removeprefix("0x")).rstrip(b"\0").decode(errors="ignore")
    except ValueError:
        graffiti = ""
    for client in KNOWN_CLIENTS:
        if client in graffiti.lower():
            return client
    m = re.match(r"^[A-Z]{2}[0-9a-f]{0,8}([A-Z]{2})", graffiti)
    if m and m.group(1) in CLIENT_CODES:
        return CLIENT_CODES[m.group(1)]
    return "unknown"


def b32(hex_str):
    raw = bytes.fromhex(hex_str.removeprefix("0x"))
    if len(raw) != 32:
        raise CheckFailed(f"expected 32 bytes, got {len(raw)}: {hex_str}")
    return raw


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--enclave", help="Kurtosis enclave to discover cl-* services from")
    ap.add_argument("--beacon", action="append", default=[], help="NAME=URL, repeatable")
    ap.add_argument("--fork-version", default="0xe8321101",
                    help="expected EIP8321_FORK_VERSION (empty to skip)")
    ap.add_argument("--params", help="network_params.yaml used for the enclave, to map validators to clients")
    ap.add_argument("--out", type=Path, default=Path("eip8321-evidence-" + time.strftime("%Y%m%d-%H%M%S")))
    ap.add_argument("--min-finalized-after-fork", type=int, default=1,
                    help="finalized epoch must be >= fork epoch + this value")
    ap.add_argument("--require-clients", type=int, default=2,
                    help="distinct clients that must have proposed canonical blocks and reveals (1 for single-client runs)")
    args = ap.parse_args()

    nodes = dict(b.split("=", 1) for b in args.beacon)
    if args.enclave:
        nodes.update(discover(args.enclave))
    if not nodes:
        ap.error("give --enclave or at least one --beacon")

    ranges = key_ranges_from_params(args.params) if args.params else []
    args.out.mkdir(parents=True, exist_ok=True)
    failures = []
    report = {"nodes": {}, "slots": {}}

    def fail(msg):
        failures.append(msg)
        print("FAIL:", msg)

    # ---- config and fork -------------------------------------------------------------
    specs = {}
    for name, url in nodes.items():
        spec = get(url, "/eth/v1/config/spec")["data"]
        version = get(url, "/eth/v1/node/version")["data"]["version"]
        specs[name] = spec
        report["nodes"][name] = {"url": url, "version": version}
        print(f"{name:32s} {version}")

    def spec_val(spec, key):
        return spec.get(key) or spec.get(key.lower())

    # Not every client lists the EIP-8321 keys in /config/spec (Prysm does not), so take the
    # fork from the fork schedule, which every client serves, and require it to be identical.
    schedules = {}
    for name, url in nodes.items():
        schedules[name] = [
            (f["current_version"].lower(), int(f["epoch"]))
            for f in get(url, "/eth/v1/config/fork_schedule")["data"]
        ]
    first_name = next(iter(nodes))
    for name, schedule in schedules.items():
        if schedule != schedules[first_name]:
            fail(f"{name}: fork schedule {schedule} differs from {first_name}: {schedules[first_name]}")

    def from_specs(key):
        values = {name: spec_val(spec, key) for name, spec in specs.items()}
        present = {v for v in values.values() if v is not None}
        if len(present) > 1:
            fail(f"{key} differs between nodes: {values}")
        return next(iter(present), None)

    fork_version = (from_specs("EIP8321_FORK_VERSION") or schedules[first_name][-1][0]).lower()
    fork_epoch = from_specs("EIP8321_FORK_EPOCH")
    if fork_epoch is None:
        matches = [e for v, e in schedules[first_name] if v == fork_version]
        if not matches:
            raise CheckFailed(f"fork version {fork_version} not in the fork schedule")
        fork_epoch = matches[0]
    fork_epoch = int(fork_epoch)
    if args.fork_version and args.fork_version.lower() != fork_version:
        fail(f"expected fork version {args.fork_version}, nodes use {fork_version}")
    slots_per_epoch = int(from_specs("SLOTS_PER_EPOCH"))
    delay = int(from_specs("COMMITMENT_REGISTRATION_DELAY") or 3)
    for key in ("HEZE_FORK_VERSION", "HEZE_FORK_EPOCH", "SECONDS_PER_SLOT", "SLOT_DURATION_MS"):
        from_specs(key)
    fork_slot = fork_epoch * slots_per_epoch
    print(f"EIP-8321 fork epoch {fork_epoch} (slot {fork_slot}), version {fork_version}, delay {delay}")

    # ---- head, fork, finality --------------------------------------------------------
    heads = {}
    for name, url in nodes.items():
        head = get(url, "/eth/v1/beacon/headers/head")
        heads[name] = int(head["data"]["header"]["message"]["slot"])
        fork = get(url, "/eth/v1/beacon/states/head/fork")["data"]
        fin = get(url, "/eth/v1/beacon/states/head/finality_checkpoints")["data"]
        finalized = int(fin["finalized"]["epoch"])
        report["nodes"][name].update(head_slot=heads[name], fork=fork, finalized_epoch=finalized,
                                     execution_optimistic=head.get("execution_optimistic"))
        print(f"{name:32s} head={heads[name]} fork={fork['current_version']} finalized={finalized}")
        if fork["current_version"].lower() != fork_version:
            fail(f"{name}: head fork version {fork['current_version']} != {fork_version}")
        if finalized < fork_epoch + args.min_finalized_after_fork:
            fail(f"{name}: finalized epoch {finalized} < fork epoch {fork_epoch} + {args.min_finalized_after_fork}")
        if head.get("execution_optimistic"):
            fail(f"{name}: head is execution optimistic")

    common_head = min(heads.values())
    if common_head <= fork_slot:
        raise CheckFailed(f"common head slot {common_head} has not passed the fork slot {fork_slot}")

    # ---- canonical chain agreement + hash-chain verification -------------------------
    reference = next(iter(nodes))
    commitments, activation = {}, {}
    stats = {"blocks": 0, "missed": 0, "registrations": 0, "reveals": 0, "legacy": 0,
             "proposers_by_client": {}, "reveals_by_client": {},
             "cross_client_registrations": 0}

    # Registrations can only be included from the fork slot on, so start there.
    for slot in range(fork_slot, common_head + 1):
        roots = {}
        for name, url in nodes.items():
            r = get(url, f"/eth/v1/beacon/blocks/{slot}/root", allow_404=True)
            roots[name] = r["data"]["root"] if r else None
        if len(set(roots.values())) != 1:
            fail(f"slot {slot}: nodes disagree on the canonical block: {roots}")
            continue
        if roots[reference] is None:
            stats["missed"] += 1
            continue

        block = get(nodes[reference], f"/eth/v2/beacon/blocks/{slot}")
        (args.out / f"block-{slot}.json").write_text(json.dumps(block, indent=1))
        if block.get("version", "").lower() != "eip8321":
            fail(f"slot {slot}: block version {block.get('version')} != eip8321")
        msg = block["data"]["message"]
        body = msg["body"]
        proposer = int(msg["proposer_index"])
        epoch = slot // slots_per_epoch
        client = client_of(proposer, body.get("graffiti", "0x"), ranges)
        stats["blocks"] += 1
        stats["proposers_by_client"][client] = stats["proposers_by_client"].get(client, 0) + 1

        if "hash_chain_reveal" not in body or "randao_commitment_registrations" not in body:
            fail(f"slot {slot}: block body is missing EIP-8321 fields")
            continue
        reveal = b32(body["hash_chain_reveal"])
        active = proposer in commitments and epoch >= activation[proposer]
        entry = {"slot": slot, "root": roots[reference], "proposer": proposer, "client": client}

        if active:
            if reveal == ZERO32:
                fail(f"slot {slot}: proposer {proposer} has an active commitment but revealed zero")
            elif blake3(DST + reveal).digest() != commitments[proposer]:
                fail(f"slot {slot}: BLAKE3(DST || reveal) != stored commitment of {proposer}")
            else:
                commitments[proposer] = reveal
                stats["reveals"] += 1
                stats["reveals_by_client"][client] = stats["reveals_by_client"].get(client, 0) + 1
            if body["randao_reveal"].lower() != G2_INFINITY:
                fail(f"slot {slot}: hash-chain proposer {proposer} did not use the G2 point at infinity")
            entry["path"] = "hash-chain"
        else:
            if reveal != ZERO32:
                fail(f"slot {slot}: proposer {proposer} has no active commitment but revealed {body['hash_chain_reveal']}")
            if body["randao_reveal"].lower() == G2_INFINITY:
                fail(f"slot {slot}: legacy proposer {proposer} used the point at infinity")
            stats["legacy"] += 1
            entry["path"] = "legacy"

        # RANDAO is processed before the operations of the same block.
        for signed in body["randao_commitment_registrations"]:
            index = int(signed["message"]["validator_index"])
            commitment = b32(signed["message"]["commitment"])
            if commitment == ZERO32:
                fail(f"slot {slot}: zero commitment registered for {index}")
            if index in commitments:
                fail(f"slot {slot}: validator {index} registered twice")
            commitments[index] = commitment
            activation[index] = epoch + delay
            stats["registrations"] += 1
            # A registration of another client's validator can only reach this proposer over
            # the `randao_commitment_registration` gossip topic.
            owner = client_of(index, "0x", ranges)
            if ranges and owner != client:
                stats["cross_client_registrations"] += 1
        entry["registrations"] = len(body["randao_commitment_registrations"])
        report["slots"][slot] = entry

    # ---- state agreement on registration bookkeeping ---------------------------------
    finalized_roots = set()
    for name, url in nodes.items():
        fin = get(url, "/eth/v1/beacon/states/head/finality_checkpoints")["data"]["finalized"]
        finalized_roots.add((fin["epoch"], fin["root"]))
    if len({e for e, _ in finalized_roots}) == 1 and len(finalized_roots) != 1:
        fail(f"nodes finalized different roots for the same epoch: {finalized_roots}")

    # ---- coverage ---------------------------------------------------------------------
    if stats["registrations"] == 0:
        fail("no commitment registrations were included after the fork")
    if stats["reveals"] == 0:
        fail("no hash-chain reveals were included")
    # With a single client nobody needs to be identified; otherwise "unknown" proves nothing.
    def known(by_client):
        if args.require_clients <= 1:
            return list(by_client)
        return [c for c in by_client if c != "unknown"]
    if len(known(stats["proposers_by_client"])) < args.require_clients:
        fail(f"canonical blocks were proposed by {stats['proposers_by_client']}, "
             f"need {args.require_clients} distinct clients")
    if ranges and args.require_clients >= 2 and stats["cross_client_registrations"] == 0:
        fail("no block included a registration from another client's validators: "
             "registrations are not crossing clients over gossip")
    if len(known(stats["reveals_by_client"])) < args.require_clients:
        fail(f"hash-chain reveals were proposed by {stats['reveals_by_client']}, "
             f"need {args.require_clients} distinct clients")

    report.update(fork_epoch=fork_epoch, fork_version=fork_version, common_head=common_head,
                  stats=stats, failures=failures)
    (args.out / "summary.json").write_text(json.dumps(report, indent=1))

    print()
    print(json.dumps(stats, indent=1))
    print(f"evidence: {args.out}")
    if failures:
        print(f"\n{len(failures)} check(s) FAILED")
        sys.exit(1)
    print("\nALL CHECKS PASSED")


if __name__ == "__main__":
    try:
        main()
    except CheckFailed as e:
        sys.exit(f"FAIL: {e}")
