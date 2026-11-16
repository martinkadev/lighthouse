# EIP-8321 interoperability testing

Kurtosis devnets that take Lighthouse through the EIP-8321 (hash-chain RANDAO) feature fork,
alone and together with the Prysm implementation from
[Humantvis/prysm@eip-8321-heze](https://github.com/Humantvis/prysm/tree/eip-8321-heze).

All networks use the minimal preset (8 slots per epoch, 6 s slots), start at Heze and
upgrade to EIP-8321 at epoch 4 (slot 32) with fork version `0xe8321101`. Registrations are
included from the fork on and activate `COMMITMENT_REGISTRATION_DELAY = 3` epochs later, so
hash-chain reveals appear from epoch 7 (slot 56).

| File | Network |
| --- | --- |
| `network_params_lighthouse.yaml` | 3 × Lighthouse + Geth, 192 validators. Baseline: run this first. |
| `network_params_interop.yaml` | 2 × Lighthouse and 2 × Prysm, each with its own VC and Geth, 64 validators per node. Both clients propose, attest and register, so each must import the other's blocks on the legacy BLS path and on the hash-chain path. |
| `network_params_lighthouse_follower.yaml` | Prysm proposes everything, Lighthouse only follows. Isolates "Lighthouse rejects Prysm" from "Prysm rejects Lighthouse". |

## What has to match between clients

The EIP-8321 fork is a consensus-layer-only fork after Heze; Geth stays on Heze and is driven
with the Heze engine methods. For the clients to interoperate they must agree on:

- `EIP8321_FORK_EPOCH` / `EIP8321_FORK_VERSION` (read from the generated `config.yaml`; the
  patch below appends them) and therefore on the fork digest of every gossip topic and RPC
  context;
- the SSZ layout and `hash_tree_root` of the EIP-8321 `BeaconBlockBody` (progressive container,
  `active_fields` `0xbd 0x7f`: the Heze layout plus `hash_chain_reveal` and
  `randao_commitment_registrations`) and `BeaconState` (Heze plus `randao_commitments` and
  `pending_randao_commitments`);
- `DOMAIN_RANDAO_COMMITMENT_REGISTRATION = 0x11000000` (the consensus-specs value; the EIP text
  says `0x0F000000`), signed with the genesis validators root;
- the `randao_commitment_registration` global gossip topic and its IGNORE/REJECT rules
  (zero commitment or unknown index: REJECT; already registered/pending or already seen:
  IGNORE; bad signature: REJECT);
- `process_randao`, `process_randao_commitment_registration` and
  `process_pending_randao_commitments` (covered by the shared consensus-spec vectors, see
  `.github/workflows/eip8321-vectors.yml`).

## Requirements

Docker, [Kurtosis CLI](https://docs.kurtosis.com/install/), `git`, `yq`, Python 3 with
`pip install blake3 pyyaml`. Allow ~16 GB RAM for the 4-node interop network.

## Images

Lighthouse is built by `start_interop.sh` (`portable,spec-minimal`). To build it yourself:

```sh
docker build --build-arg FEATURES=portable,spec-minimal -t lighthouse:eip8321-minimal .
```

Prysm images come from the Prysm team's branch (`testing/devnet/eip8321/Build.ps1`, which runs
`build-linux.sh` with Bazel `--config=minimal`) and must be tagged
`bct/prysm-beacon:eip8321-minimal` and `bct/prysm-validator:eip8321-minimal`, or edit
`cl_image`/`vc_image` in the YAML files.

## ethereum-package

`ethereum-package.patch` is the patch used by the Prysm team, applied to ethereum-package
revision `5795b77e65235ad223d4d3b413b2739190bf60cc`. It adds the `eip8321_fork_epoch` and
`eip8321_fork_version` network parameters, writes them into the generated `config.yaml`
(the genesis itself stays a normal Heze genesis; every client performs the upgrade at the
fork epoch) and gives the Prysm VC the gRPC endpoint it uses for EIP-8321 duties. Lighthouse
needs no package changes. `start_interop.sh` checks out and patches the package in
`.ethereum-package/`. With `-l` it also vendors the Kurtosis packages ethereum-package imports
and `replace`s them locally, for hosts where the Kurtosis engine cannot clone from GitHub
(for example behind an HTTP proxy).

## Run

```sh
cd scripts/local_testnet/eip8321
./start_interop.sh -n network_params_lighthouse.yaml -e eip8321-lh      # baseline
./start_interop.sh -n network_params_interop.yaml -e eip8321-interop    # Lighthouse + Prysm
./start_interop.sh -n network_params_lighthouse_follower.yaml -e eip8321-follow -b false
```

Wait until about epoch 10 (≈ 8 minutes after genesis), then:

```sh
python3 check_interop.py --enclave eip8321-interop --params network_params_interop.yaml
# single-client networks:
python3 check_interop.py --enclave eip8321-lh --require-clients 1
```

`check_interop.py` queries every consensus node through the Beacon API and fails unless all of these hold:

- the fork config matches on every node, and every node has upgraded;
- every node returns the same canonical block root for every slot from the fork to the
  common head;
- finality is past the fork on every node, and no head is optimistic;
- registrations were included;
- every hash-chain reveal satisfies `BLAKE3("HASH_CHAIN_RANDAO" || reveal) == commitment`,
  is used only after activation, and comes with the G2 point-at-infinity `randao_reveal`;
- legacy proposers keep a zero reveal and a BLS reveal;
- with `--params`, canonical blocks and hash-chain reveals were produced by at least two
  different clients.

The raw block JSON and `summary.json` are written to `eip8321-evidence-<timestamp>/`.

Dora is not used: the upstream explorer cannot decode the EIP-8321 block body.

## Debugging

```sh
kurtosis service logs -f eip8321-interop cl-1-lighthouse-geth | grep -iE "randao|eip8321|reject|penal"
kurtosis service logs -f eip8321-interop cl-3-prysm-geth     | grep -iE "randao|eip8321|reject|invalid"
```

Typical symptoms:

- peers disconnect at slot 32: the fork digests differ (fork version or blob schedule);
- `could not decode` / SSZ errors on block topics: body layout mismatch;
- one client's blocks are orphaned only after epoch 7: the hash-chain path of `process_randao`
  or the activation timing differs;
- registrations never included by one client: gossip validation or op-pool packing differs.

## Lighthouse-only simulation

Without Docker, the same lifecycle can be checked for Lighthouse with the in-process simulator
and the mock execution layer:

```sh
cargo run --release --bin simulator eip8321-sim
```

It runs Gloas → Heze → EIP-8321 and checks the following:

- every validator registers;
- proposers switch to hash-chain reveals that hash to their previous commitment;
- all nodes, including one that joins late and syncs the EIP-8321 blocks over req/resp,
  agree on the canonical chain;
- finality advances past the fork.
