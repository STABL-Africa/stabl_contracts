//! # Stabl Account Factory
//!
//! Deploys `stabl_passkey_multi_signer` instances from a pinned wasm hash.
//!
//! Exists because the smart account must be initialised through its
//! constructor, and the off-chain server (Elixir `stellar_sdk`) can only issue
//! plain contract invocations, not `CreateContractV2` host functions with
//! constructor arguments. Wrapping the deploy in a contract call makes it
//! reachable from any client.
//!
//! The factory is permissionless and immutable: anyone may call `deploy` and
//! pays the fee; the wasm hash is fixed at construction. A new account version
//! means a new factory, recorded in `deployments/<network>.json`.

use soroban_sdk::{
    contract, contractevent, contractimpl, contracttype, panic_with_error, Address, BytesN, Env,
    Map, Val, Vec,
};
use stellar_accounts::smart_account::Signer;

#[contracttype]
#[derive(Clone)]
enum DataKey {
    WasmHash,
}

#[soroban_sdk::contracterror]
#[repr(u32)]
pub enum FactoryError {
    /// Factory has no wasm hash stored (constructor never ran).
    NotInitialized = 1,
}

/// Emitted once per deployed account.
#[contractevent]
pub struct AccountDeployed {
    #[topic]
    pub account: Address,
    pub salt: BytesN<32>,
    pub wasm_hash: BytesN<32>,
}

// Instance storage TTL management: extend when within this many ledgers of
// expiry, to this many ledgers ahead. Roughly 30 days at ~5s ledgers.
const TTL_THRESHOLD: u32 = 100_000;
const TTL_EXTEND_TO: u32 = 518_400;

#[contract]
pub struct StablAccountFactory;

#[contractimpl]
impl StablAccountFactory {
    /// Pin the smart account wasm this factory deploys.
    pub fn __constructor(e: &Env, wasm_hash: BytesN<32>) {
        e.storage().instance().set(&DataKey::WasmHash, &wasm_hash);
    }

    /// Deploy a new smart account with the given initial signers and policies.
    ///
    /// `salt` fixes the resulting address (see [`Self::predict`]); reusing a
    /// salt fails because the contract already exists. The caller pays the
    /// fee. No authorization is required beyond that.
    pub fn deploy(
        e: &Env,
        salt: BytesN<32>,
        signers: Vec<Signer>,
        policies: Map<Address, Val>,
    ) -> Address {
        let wasm_hash = Self::wasm_hash(e);
        e.storage()
            .instance()
            .extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);

        let account = e
            .deployer()
            .with_current_contract(salt.clone())
            .deploy_v2(wasm_hash.clone(), (signers, policies));

        AccountDeployed {
            account: account.clone(),
            salt,
            wasm_hash,
        }
        .publish(e);

        account
    }

    /// Address `deploy` will produce for `salt`, without deploying.
    pub fn predict(e: &Env, salt: BytesN<32>) -> Address {
        e.deployer().with_current_contract(salt).deployed_address()
    }

    /// The smart account wasm hash this factory deploys.
    pub fn wasm_hash(e: &Env) -> BytesN<32> {
        e.storage()
            .instance()
            .get(&DataKey::WasmHash)
            .unwrap_or_else(|| panic_with_error!(e, FactoryError::NotInitialized))
    }
}
