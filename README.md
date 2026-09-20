# stabl_contracts

[![CI](https://github.com/STABL-Africa/stabl_contracts/actions/workflows/ci.yml/badge.svg)](https://github.com/STABL-Africa/stabl_contracts/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Soroban smart contracts for [STABL](https://stabl.africa): Stellar **smart
accounts** (custom account contracts) that let groups hold and move funds
on-chain under programmable authorization rules, signed with passkeys.

> [!WARNING]
> This is experimental software and is provided on an "as is" and "as available"
> basis. We do not give any warranties and will not be liable for any losses
> incurred through any use of this code base. These contracts have not been
> audited and are deployed to testnet only. Do not use them with real funds.

## What this adds on top of OpenZeppelin

The account logic is [OpenZeppelin's `stellar-accounts`](https://github.com/OpenZeppelin/stellar-contracts).
This repository is the integration layer around it.

- a stateless, admin-less **WebAuthn verifier** deployed once per network,
- a permissionless **factory** so clients that can only issue plain contract
  invocations (no constructor deploys) can still create accounts,
- tests that pin the exact bytes a client must produce: `key_data` layout,
  the auth digest `sha256(signature_payload || xdr(context_rule_ids))` used as
  the WebAuthn challenge, low-s `r||s` signatures, and `AuthPayload` encoding,
  verified against a real browser assertion and against the built wasm,
- a [browser harness](scripts/e2e/README.md) that runs the whole flow on
  testnet and logs every intermediate value, usable as a reference for a
  server implementation in any language.

## Contracts

| Crate | Description |
| --- | --- |
| [`stabl_multi_signer`](contracts/stabl_multi_signer) | Multi-signer smart account built on [OpenZeppelin Stellar Contracts](https://github.com/OpenZeppelin/stellar-contracts) (`stellar-accounts`). Signers are either delegated (another Stellar address, verified natively) or external (a verifier contract plus public key, used for secp256r1/WebAuthn passkeys). Authorization is expressed as context rules with pluggable policies such as signature thresholds. |
| [`stabl_account_factory`](contracts/stabl_account_factory) | Permissionless, immutable factory that deploys `stabl_multi_signer` instances from a pinned wasm hash via `deploy(salt, signers, policies)`. Exists because the account needs its constructor and some client SDKs can only issue plain invocations, not constructor deploys. `predict(salt)` gives the address ahead of time. |
| [`stabl_passkey_verifier`](contracts/stabl_passkey_verifier) | Stateless WebAuthn (passkey) signature verifier implementing OpenZeppelin's `Verifier` trait. Deployed once per network and referenced by smart accounts as `Signer::External(verifier, pubkey ++ credential_id)`. No admin, no upgrade path: a fix is a new address that each account migrates to under its own authorization. |
| [`smart_account`](contracts/smart_account) | Minimal reference custom account: a stored set of ed25519 signers, any one of which authorizes. Useful for understanding `__check_auth` end to end without the OpenZeppelin machinery. |

Both contracts implement Soroban's `CustomAccountInterface`: the contract
address itself acts as a Stellar account, and `__check_auth` decides whether
a given set of signatures authorizes a given set of invocations.

## Trying it on testnet

`scripts/e2e/` is a browser page that authorises a call on a deployed account
with a real passkey, end to end, and logs every intermediate value. It doubles
as the reference for off-chain client implementations. See
[scripts/e2e/README.md](scripts/e2e/README.md).

## Layout

```text
contracts/<name>/        one crate per contract (workspace member)
deployments/testnet.json contract IDs + network endpoints per environment
scripts/deploy_testnet.sh build + deploy everything, update the manifest
```

## Prerequisites

- Rust via rustup, with the wasm target: `rustup target add wasm32v1-none`
- Stellar CLI: `brew install stellar-cli` (or see the
  [Stellar docs](https://developers.stellar.org/docs/tools/cli/install-cli))
- A funded testnet identity: `stellar keys generate deployer --network testnet --fund`

## Workflow

```sh
make test            # cargo unit tests (Env simulation, no network)
make build           # compile contracts to wasm
make deploy-testnet  # build, deploy to testnet, write deployments/testnet.json
make fund            # top up the deployer account from friendbot
```

`make deploy-testnet` deploys every contract listed in
`scripts/deploy_testnet.sh` and records the contract IDs in
`deployments/testnet.json`. Redeploying creates a *new* contract instance
(new ID); the manifest always points at the latest.

Ad-hoc invocation for poking at a deployed contract:

```sh
stellar contract invoke --id smart_account --source deployer --network testnet -- signers
```

(`smart_account` resolves through the alias saved in `.stellar/` at deploy time.)

## Integrating from off-chain code

Soroban contract calls go through **Soroban RPC**
(`https://soroban-testnet.stellar.org`).
`deployments/testnet.json` carries everything a client
needs: network passphrase, RPC and Horizon URLs, and the current contract ID
for each contract. Treat it as the single source of truth and read contract
IDs from it rather than hard-coding them.

## Gotchas

- `soroban-env-host` declares `ed25519-dalek >= 2.0.0` (unbounded); the
  ed25519-dalek 3.0 release breaks its build. `Cargo.lock` pins 2.2.0. If a
  fresh resolve fails in `soroban-env-host`, re-pin with
  `cargo update -p ed25519-dalek@3.0.0 --precise 2.2.0`.
- Testnet resets quarterly; after a reset, re-fund the deployer and rerun
  `make deploy-testnet`.

## Contributing

Issues and pull requests are welcome. Please run `cargo fmt --all` and
`make test` before opening a PR; CI checks formatting, unit tests and the
wasm build.

## Security

See [SECURITY.md](SECURITY.md) for how to report vulnerabilities.

## License

[MIT](LICENSE) © 2026 STABL Africa
