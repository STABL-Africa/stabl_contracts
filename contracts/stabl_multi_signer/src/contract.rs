use soroban_sdk::{
    auth::{Context, CustomAccountInterface},
    contract, contractimpl,
    crypto::Hash,
    Address, BytesN, Env, Map, String, Val, Vec,
};
use stellar_accounts::smart_account::{
    add_context_rule, do_check_auth, AuthPayload, ContextRule, ContextRuleType, Signer,
    SmartAccount, SmartAccountError,
};

#[contract]
pub struct StablPasskeyMultiSigner;

#[contractimpl]
impl StablPasskeyMultiSigner {
    pub fn __constructor(e: &Env, signers: Vec<Signer>, policies: Map<Address, Val>) {
        add_context_rule(
            e,
            &ContextRuleType::Default,
            &String::from_str(e, "default"),
            None,
            &signers,
            &policies,
        );
    }

    /// Replace this account's code with an already-uploaded wasm.
    ///
    /// Authorized by the account itself: the call goes through `__check_auth`
    /// like any other, so whatever signers and policies govern the default
    /// context rule govern upgrades too. Nobody else, including whoever
    /// deployed the factory, can upgrade an account.
    ///
    /// Storage is untouched; the new code must read the old layout. The
    /// swap takes effect after this invocation completes.
    pub fn upgrade(e: &Env, new_wasm_hash: BytesN<32>) {
        e.current_contract_address().require_auth();
        e.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}

#[contractimpl]
impl CustomAccountInterface for StablPasskeyMultiSigner {
    type Error = SmartAccountError;
    type Signature = AuthPayload;

    fn __check_auth(
        e: Env,
        signature_payload: Hash<32>,
        signatures: AuthPayload,
        auth_contexts: Vec<Context>,
    ) -> Result<(), Self::Error> {
        do_check_auth(&e, &signature_payload, &signatures, &auth_contexts)
    }
}

#[contractimpl(contracttrait)]
impl SmartAccount for StablPasskeyMultiSigner {}
