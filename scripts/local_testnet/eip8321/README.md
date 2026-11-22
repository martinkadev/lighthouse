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
| `network_params_your_client.yaml` | Template for other teams: the published Lighthouse image plus one node of your client. |

## Interop with your client

**Tested so far:** Lighthouse ↔ Prysm
([Humantvis/prysm@eip-8321-heze](https://github.com/Humantvis/prysm/tree/eip-8321-heze)):
both clients propose, register and reveal, and registrations cross clients over gossip. See
[VALIDATION.md](VALIDATION.md). No other client has been tested against Lighthouse yet.

### Your implementation must

- build on **Heze** (`heze-devnet-2`) with EIP-8321 as a **separate fork after Heze**, so Heze
  blocks and state are unchanged;
- follow the pinned consensus-specs feature spec, i.e. pass the EIP-8321 vectors from
  [`lukassta/consensus-specs` `v1.7.0-eip8321.1`](https://github.com/lukassta/consensus-specs/releases/tag/v1.7.0-eip8321.1).
  Lighthouse's `testing/ef_tests/download_eip8321_vectors.sh` fetches and checksums them. Where
  the EIP web page and the spec differ, the spec wins (see the list below);
- read `EIP8321_FORK_EPOCH` and `EIP8321_FORK_VERSION` from the network's `config.yaml`.

Passing the vectors is a good sign, but they do not cover networking. Lighthouse passed every
vector and still had two gossip bugs that only a live devnet exposed (see VALIDATION.md), so
the devnet run is the real test.

### Versions

| Component | Version |
| --- | --- |
| Lighthouse image | `ghcr.io/martinkadev/lighthouse:eip8321` (latest `eip8321` branch) or `ghcr.io/martinkadev/lighthouse:sha-<commit>` (pinned); supports the minimal preset |
| Execution client | `ethpandaops/geth:heze-devnet-2-66af2fe` |
| ethereum-package | `5795b77e65235ad223d4d3b413b2739190bf60cc` + `ethereum-package.patch` (applied by `start_interop.sh`) |
| Kurtosis CLI | 1.22.2 |

### Network parameters

Every node in one network must use the same values. Ours are:

| Parameter | Value |
| --- | --- |
| Preset | `minimal` |
| Fork schedule | Electra, Fulu, Gloas and Heze at genesis; EIP-8321 at epoch 4 |
| `EIP8321_FORK_VERSION` | `0xe8321101` |

Fork epoch and execution client are per-network choices: another epoch (e.g. 5) or another
execution client works if every participant uses it.

### Steps

1. Build your beacon node and validator client images for **amd64** with the **minimal**
   preset.
2. Copy `network_params_your_client.yaml`, set `cl_type` and your images. If your client cannot
   propose or register yet, set its `validator_count: 0`: Lighthouse then produces every block
   and your node follows and verifies (follower mode).
3. Run it and check it:

   ```sh
   ./start_interop.sh -n network_params_your_client.yaml -e eip8321-yours
   # wait ~10 minutes (past epoch 10)
   python3 check_interop.py --enclave eip8321-yours --params network_params_your_client.yaml
   # follower mode: add --require-clients 1
   ```

4. Send us the `check_interop.py` output and the `eip8321-evidence-*` folder, or open an issue.

### Client support in ethereum-package

Your client must be supported by ethereum-package (Lighthouse, Prysm, Teku, Lodestar, Nimbus
and Grandine are). If it needs extra flags for EIP-8321, use `cl_extra_params` /
`vc_extra_params`. If it needs launcher changes, extend `ethereum-package.patch` and send us
the change. The Prysm team needed this for their validator client, and the current patch
already contains it.


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

Every push to the `eip8321` branch publishes the Lighthouse image to
`ghcr.io/martinkadev/lighthouse:eip8321` and `ghcr.io/martinkadev/lighthouse:sha-<commit>`
(`.github/workflows/ghcr.yml`). `start_interop.sh` pulls it when a YAML file uses that name.

With a plain local tag such as `lighthouse:eip8321-minimal` (the default in the repository's own
YAML files), `start_interop.sh` builds the image from this checkout (`portable,spec-minimal`)
unless `-b false` is given. To build it yourself:

```sh
docker build --build-arg FEATURES=portable,spec-minimal -t lighthouse:eip8321-minimal .
```

Images of other clients come from their teams. `start_interop.sh` uses them if they exist
locally (e.g. after `docker load`) and otherwise tries `docker pull`. The Prysm YAML files expect
`bct/prysm-beacon:eip8321-minimal` and `bct/prysm-validator:eip8321-minimal`; edit
`cl_image`/`vc_image` for other names.

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

## Results

See [VALIDATION.md](VALIDATION.md) for the recorded interop runs and the bugs they found.
