# stabl_contracts

Soroban (Rust) smart contracts for the `../stabl_pay` Elixir server. Mostly
Stellar smart accounts — custom account contracts implementing
`CustomAccountInterface` / `__check_auth`. See README.md for full setup.

## Commands

- `make test` — cargo unit tests (no network)
- `make build` — `stellar contract build` → `target/wasm32v1-none/release/*.wasm`
- `make deploy-testnet` — deploy all contracts, update `deployments/testnet.json`
- Toolchain lives at `~/.cargo/bin` (rustup) + `stellar` CLI (Homebrew)

## Conventions

- One crate per contract under `contracts/`, workspace deps in root `Cargo.toml`
  (`soroban-sdk = "27"`).
- Every deploy must go through `scripts/deploy_testnet.sh` so
  `deployments/testnet.json` stays the single source of truth for contract IDs —
  stabl_pay reads it.
- New contracts: add the crate, then add a `wanted <name> && deploy <name> -- <args>`
  line to `scripts/deploy_testnet.sh`. Contracts that stabl_pay instantiates
  per user (e.g. `stabl_passkey_multi_signer`) use `upload` instead, which
  records `wasm_hash` rather than an instance `id`.
- `scripts/deploy_testnet.sh deployer <name...>` redeploys only the named
  contracts; other manifest entries are untouched.
- Smart account signers sign `sha256(signature_payload || xdr(context_rule_ids))`,
  not the raw host payload. That digest is what goes to the browser as the
  WebAuthn challenge. See `contracts/stabl_passkey_multi_signer/src/test.rs`.
- `make test` builds wasm first: `stabl_account_factory` tests `contractimport!`
  the real `stabl_passkey_multi_signer.wasm`. CI does the same.
- `make e2e` serves `scripts/e2e/` (browser harness, testnet, real passkey).
  It is the reference for how stabl_pay must build `AuthPayload`.
- Testnet identity alias is `deployer` (global CLI config, funded via friendbot).

## Known issues

- Keep `ed25519-dalek` pinned to 2.x in Cargo.lock; 3.0 breaks
  `soroban-env-host` (it declares an unbounded `>= 2.0.0` requirement).
- Soroban calls need Soroban RPC (`https://soroban-testnet.stellar.org`), not
  Horizon. stabl_pay switches clients when `STELLAR_RPC_URL` is set.
