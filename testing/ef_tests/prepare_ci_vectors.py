#!/usr/bin/env python3
"""Install checksum-pinned release vectors into a fresh directory (Python 3.12+)."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile

LOCK = Path(__file__).with_name("ci-vectors.lock.json")


def verify_checksum(path, expected):
    with path.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != expected:
        raise ValueError(f"Checksum mismatch for {path.name}: {actual} != {expected}")


def download(cache, name, repository, release, expected):
    path = cache / f"{name}.tar.gz"
    if not path.exists():
        partial = path.with_suffix(".partial")
        url = f"https://github.com/{repository}/releases/download/{release}/{name}.tar.gz"
        subprocess.run(["curl", "--fail", "--location", "--show-error", "--retry", "3",
                        "--retry-all-errors", "--output", str(partial), url], check=True)
        verify_checksum(partial, expected)
        partial.replace(path)
    verify_checksum(path, expected)
    return path


def extract(archive, destination, prefixes=None):
    count = 0
    with tarfile.open(archive, "r|gz") as source:
        for member in source:
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts:
                raise ValueError(f"Unsafe archive member: {member.name}")
            if not (member.isfile() or member.isdir()):
                raise ValueError(f"Unsupported archive member: {member.name}")
            normalized = str(name)
            if prefixes and not any(normalized == prefix or normalized.startswith(prefix + "/")
                                    for prefix in prefixes):
                continue
            source.extract(member, destination, filter="data")
            count += member.isfile()
    if count == 0:
        raise ValueError(f"No matching vector files in {archive}")


def audit(output, preset):
    counts = {}
    for suite in ("epoch_processing/pending_randao_commitments", "operations/randao",
                  "operations/randao_commitment_registration", "fork/fork"):
        directory = output / "tests" / preset / "eip8321" / suite
        count = sum(1 for _ in directory.rglob("pre.ssz_snappy"))
        if not count:
            raise ValueError(f"Missing EIP-8321 vectors: {directory}")
        counts[suite] = count
    for directory in (output / "tests/general", output / "bls12-381-tests"):
        if not any(directory.rglob("*.yaml")):
            raise ValueError(f"Missing auxiliary vectors: {directory}")
    if preset == "minimal":
        for fork in ("fulu", "gloas"):
            directory = output / "tests/minimal" / fork / "fork_choice_compliance"
            if not any(directory.rglob("*.yaml")):
                raise ValueError(f"Missing compliance vectors: {directory}")
    return counts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--preset", choices=("minimal", "mainnet"), required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--cache-dir", type=Path, required=True)
    args = parser.parse_args()
    output = args.output_dir.resolve()
    cache = args.cache_dir.resolve()
    if output.exists():
        parser.error(f"Refusing to overwrite existing vectors: {output}")
    lock = json.loads(LOCK.read_text())
    cache.mkdir(parents=True, exist_ok=True)
    output.mkdir(parents=True)
    names = ["general", args.preset, "bls_tests_yaml"]
    if args.preset == "minimal":
        names.append("comptests")
    for name in names:
        print(f"Preparing {name}.tar.gz", flush=True)
        bls = name == "bls_tests_yaml"
        archive = download(cache, name, lock["bls_repository" if bls else "repository"],
                           lock["bls_release" if bls else "release"], lock["sha256"][name])
        destination = output / "bls12-381-tests" if bls else output
        destination.mkdir(exist_ok=True)
        prefixes = [f"tests/minimal/{fork}/fork_choice_compliance" for fork in ("fulu", "gloas")] if name == "comptests" else None
        print(f"Extracting {archive.name}", flush=True)
        extract(archive, destination, prefixes)
    counts = audit(output, args.preset)
    provenance = dict(lock, preset=args.preset, downloaded=names, eip8321_pre_state_counts=counts)
    (output / "ci-vector-provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    print(json.dumps(provenance, indent=2))


if __name__ == "__main__":
    main()
