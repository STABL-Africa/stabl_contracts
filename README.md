# stabl_contracts

Soroban smart contracts for stabl, primarily Stellar **smart accounts**
(custom account contracts) used by the `../stabl_pay` Elixir server.

## Layout

```text
contracts/<name>/        one crate per contract (workspace member)
deployments/testnet.json contract IDs + network endpoints per environment
scripts/deploy_testnet.sh build + deploy everything, update the manifest
```

## Prerequisites

- Rust via rustup, with the wasm target: `rustup target add wasm32v1-none`
- Stellar CLI: `brew install stellar-cli`
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

## Contracts

### smart_account

A custom account contract (`CustomAccountInterface`): the contract address
acts as a Stellar account whose authorization logic is `__check_auth`.
Current starter logic: a stored set of ed25519 signers, any one of which
authorizes. `set_signers` rotates the set and requires the account's own
auth. Next step is secp256r1/WebAuthn verification in `__check_auth` so
stabl_pay passkeys can sign directly.

The constructor takes the initial signer set as raw 32-byte ed25519 public
keys (hex). The deploy script seeds it with the deployer key.

## Pointing stabl_pay at these contracts

Soroban contract calls go through **Soroban RPC**, not Horizon — Horizon
only serves classic operations. stabl_pay's `dev.exs` already switches to
`StablPay.Blockchain.StellarRpcClient` when `STELLAR_RPC_URL` is set, so run
the server with:

```sh
STELLAR_RPC_URL=https://soroban-testnet.stellar.org mix phx.server
```

and read contract IDs from `deployments/testnet.json` in this repo
(network passphrase and both endpoints are included in the manifest).

## Gotchas

- `soroban-env-host` declares `ed25519-dalek >= 2.0.0` (unbounded); the
  ed25519-dalek 3.0 release breaks its build. `Cargo.lock` pins 2.2.0 — if a
  fresh resolve fails in `soroban-env-host`, re-pin with
  `cargo update -p ed25519-dalek@3.0.0 --precise 2.2.0`.
- Testnet resets quarterly; after a reset, re-fund the deployer and rerun
  `make deploy-testnet`.
