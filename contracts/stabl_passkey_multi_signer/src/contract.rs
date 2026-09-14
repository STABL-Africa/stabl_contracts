use soroban_sdk::{
    auth::{Context, CustomAccountInterface},
    contract, contractclient, contracterror, contractimpl, contracttype,
    crypto::Hash,
    panic_with_error, symbol_short, Address, BytesN, Env, IntoVal, Map, String, Symbol, Val, Vec,
};
use stellar_accounts::policies::simple_threshold::SimpleThresholdAccountParams;
use stellar_accounts::smart_account::{
    add_context_rule, add_policy, add_signer, do_check_auth, get_context_rule,
    get_context_rules_count, remove_context_rule, remove_policy, remove_signer,
    update_context_rule_name, update_context_rule_valid_until, AuthPayload, ContextRule,
    ContextRuleType, ExecutionEntryPoint, Signer, SmartAccount, SmartAccountError,
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
