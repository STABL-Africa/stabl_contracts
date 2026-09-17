use soroban_sdk::{
    auth::{Context, CustomAccountInterface},
    contract, contractimpl,
    crypto::Hash,
    Address, Env, Map, String, Val, Vec,
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
