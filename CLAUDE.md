# stabl_contracts

Soroban (Rust) smart contracts for a STABL off-chain server. Mostly
Stellar smart accounts — custom account contracts implementing
`CustomAccountInterface` / `__check_auth`. See README.md for full setup.

## Commands

- `make test` — cargo unit tests (no network)
- `make build` — `stellar contract build` → `target/wasm32v1-none/release/*.wasm`
- `make deploy-testnet` — deploy all contracts, update `deployments/testnet.json`
- `make deploy-mainnet` — same against mainnet from `deployer-mainnet`,
  prompts first, writes `deployments/mainnet.json`
- Toolchain lives at `~/.cargo/bin` (rustup) + `stellar` CLI (Homebrew)

## Conventions

- Public repository. Never name the private server project, its language or
  framework, or any individual in code, comments, docs, or commit messages.
  Say "the server" or "an off-chain client".
- One crate per contract under `contracts/`, workspace deps in root `Cargo.toml`
  (`soroban-sdk = "27"`).
- Every deploy must go through `scripts/deploy.sh <network>` so
  `deployments/<network>.json` stays the single source of truth for contract
  IDs. Off-chain clients copy the manifest into their own repo and load it on
  their deploy; they never hardcode IDs. `scripts/deploy_testnet.sh` is a shim.
- New contracts: add the crate, then add a `wanted <name> && deploy <name> -- <args>`
  line to `scripts/deploy.sh`. Contracts that the server instantiates
  per user (e.g. `stabl_multi_signer`) use `upload` instead, which
  records `wasm_hash` rather than an instance `id`.
- `scripts/deploy.sh <network> <identity> <name...>` redeploys only the named
  contracts; other manifest entries are untouched. Redeploy
  `stabl_account_factory` whenever `stabl_multi_signer` is re-uploaded.
- `smart_account` (reference contract) is testnet-only; the script skips it on
  mainnet.
- Mainnet deploys run from a laptop, not CI or the server. Contract addresses
  are public by design (wasm is on-chain and this repo is public); user
  account addresses never go in the manifest.
- Smart account signers sign `sha256(signature_payload || xdr(context_rule_ids))`,
  not the raw host payload. That digest is what goes to the browser as the
  WebAuthn challenge. See `contracts/stabl_multi_signer/src/test.rs`.
- `make test` builds wasm first: `stabl_account_factory` tests `contractimport!`
  the real `stabl_multi_signer.wasm`. CI does the same.
- `make e2e` serves `scripts/e2e/` (browser harness, testnet, real passkey).
  It is the reference for how any off-chain client builds `AuthPayload`.
- Testnet identity alias is `deployer` (global CLI config, funded via friendbot).
  Mainnet alias is `deployer-mainnet`, created and funded by hand, never
  auto-generated. It holds no power over deployed contracts afterwards.
- `stabl_multi_signer` has `upgrade(new_wasm_hash)` authorized by the account
  itself (its own signers through `__check_auth`). Upload the new wasm first,
  then each account upgrades under its own rule.

## Known issues

- Keep `ed25519-dalek` pinned to 2.x in Cargo.lock; 3.0 breaks
  `soroban-env-host` (it declares an unbounded `>= 2.0.0` requirement).
- Soroban calls need Soroban RPC (`https://soroban-testnet.stellar.org`), not
  Horizon.
