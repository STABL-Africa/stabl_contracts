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
  (`soroban-sdk = "26"`).
- Every deploy must go through `scripts/deploy_testnet.sh` so
  `deployments/testnet.json` stays the single source of truth for contract IDs —
  stabl_pay reads it.
- New contracts: add the crate, then add a `deploy <name> -- <constructor args>`
  line to `scripts/deploy_testnet.sh`.
- Testnet identity alias is `deployer` (global CLI config, funded via friendbot).

## Known issues

- Keep `ed25519-dalek` pinned to 2.x in Cargo.lock; 3.0 breaks
  `soroban-env-host` (it declares an unbounded `>= 2.0.0` requirement).
- Soroban calls need Soroban RPC (`https://soroban-testnet.stellar.org`), not
  Horizon. stabl_pay switches clients when `STELLAR_RPC_URL` is set.
