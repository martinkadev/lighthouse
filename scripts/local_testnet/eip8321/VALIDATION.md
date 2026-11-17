# Validation record

## Kurtosis Lighthouse <-> Prysm, 2026-11-16

`network_params_interop.yaml`: 2 × Lighthouse and 2 × Prysm beacon nodes, each with its
own validator client (64 keys) and Geth `heze-devnet-2-66af2fe`; minimal preset, Heze at
genesis, EIP-8321 at epoch 4 with fork version `0xe8321101`. Started with
`./start_interop.sh -l -b false`, checked with `check_interop.py --params`.

Images:

- Lighthouse: `eip8321-interop` branch, built `--features portable,spec-minimal`.
- Prysm: `Humantvis/prysm@eip-8321-heze` (80eff90), built with
  `go build -tags minimal ./cmd/beacon-chain ./cmd/validator` on an Ubuntu 24.04 base,
  with the binaries at `/beacon-chain` and `/app/cmd/validator/validator`.

| Run | Lighthouse build | Result |
| --- | --- | --- |
| 1 | `eip8321-fixes` (9549885) | 132 slots after the fork, 0 missed, all nodes agreed on every slot and finalized epoch 18; 256 registrations, 107 hash-chain reveals (57 Lighthouse, 50 Prysm). **But** Lighthouse logged `Failed to subscribe … randao_commitment_registration … NotAllowed` and `NoPeersSubscribedToTopic`: every node only included its own validators' registrations. |
| 2 | + subscription whitelist fix | Lighthouse → Prysm registrations crossed over gossip (a Prysm block included 28 Lighthouse registrations). Lighthouse dropped every Prysm registration: `Unknown gossipsub topic`. |
| 3 | + topic parser fix | 67 slots after the fork, 0 missed, all nodes agreed on every slot and finalized epoch 10; 256 registrations, 128 of them included by a proposer of the other client; 43 reveals (23 Lighthouse, 20 Prysm). Each Lighthouse node processed 192 gossip registrations (128 from Prysm, 64 from the other Lighthouse node). No gossip penalties or decode errors on either client. |

`check_interop.py` verified for every canonical block from the fork on:
`BLAKE3("HASH_CHAIN_RANDAO" || reveal)` equals the previous commitment, no reveal before
`COMMITMENT_REGISTRATION_DELAY`, point-at-infinity `randao_reveal` on the hash-chain path,
zero reveal with a BLS reveal on the legacy path, and identical block roots on all nodes.

Known, not EIP-8321 specific: Prysm logs that it does not recognise `SLOT_DURATION_MS_HEZE`
and `MIN_BLOB_DATA_RETENTION_MS` in the generated config, and does not list the
`EIP8321_*` keys in `/eth/v1/config/spec` (the checker reads the fork from
`/eth/v1/config/fork_schedule`). Dora cannot decode EIP-8321 blocks.
