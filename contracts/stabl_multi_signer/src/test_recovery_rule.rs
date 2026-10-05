//! Spike for social recovery completion: a context rule with no signers and
//! one policy, used with an `AuthPayload` that carries no signatures. The
//! policy alone decides. A stub stands in for the recovery controller and only
//! allows `add_signer` on the account itself.
#![cfg(test)]
extern crate std;

use crate::contract::{StablPasskeyMultiSigner, StablPasskeyMultiSignerClient};
use soroban_sdk::auth::{Context, ContractContext};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::xdr::{
    InvokeContractArgs, ScAddress, ScSymbol, ScVal, SorobanAddressCredentials,
    SorobanAuthorizationEntry, SorobanAuthorizedFunction, SorobanAuthorizedInvocation,
    SorobanCredentials, VecM,
};
use soroban_sdk::{
    contract, contractimpl, map, panic_with_error, symbol_short, vec, Address, Bytes, Env, IntoVal,
    Map, String, Symbol, TryFromVal, Val, Vec,
};
use stellar_accounts::policies::Policy;
use stellar_accounts::smart_account::{
    AuthPayload, ContextRule, ContextRuleType, Signer, SmartAccountError,
};

#[contract]
struct OnlyAddSignerPolicy;

#[contractimpl]
impl Policy for OnlyAddSignerPolicy {
    type AccountParams = ();

    fn enforce(
        e: &Env,
        context: Context,
        authenticated_signers: Vec<Signer>,
        _context_rule: ContextRule,
        smart_account: Address,
    ) {
        smart_account.require_auth();
        assert!(authenticated_signers.is_empty());
        match context {
            Context::Contract(ContractContext {
                contract, fn_name, ..
            }) if contract == smart_account && fn_name == Symbol::new(e, "add_signer") => {}
            _ => panic_with_error!(e, SmartAccountError::UnvalidatedContext),
        }
    }

    fn install(_e: &Env, _params: (), _context_rule: ContextRule, smart_account: Address) {
        smart_account.require_auth();
    }

    fn uninstall(_e: &Env, _context_rule: ContextRule, smart_account: Address) {
        smart_account.require_auth();
    }
}

struct Setup {
    e: Env,
    account: Address,
    recovery_rule: u32,
}

/// Account with one delegated owner on the default rule, plus a recovery rule
/// `CallContract(self)` with no signers and the stub policy.
fn setup() -> Setup {
    let e = Env::default();
    let owner = Address::generate(&e);
    let policy = e.register(OnlyAddSignerPolicy, ());
    let account = e.register(
        StablPasskeyMultiSigner,
        (
            vec![&e, Signer::Delegated(owner)],
            Map::<Address, Val>::new(&e),
        ),
    );

    e.mock_all_auths();
    let rule = StablPasskeyMultiSignerClient::new(&e, &account).add_context_rule(
        &ContextRuleType::CallContract(account.clone()),
        &String::from_str(&e, "recovery"),
        &None,
        &vec![&e],
        &map![&e, (policy, ().into_val(&e))],
    );
    e.set_auths(&[]);
    assert!(rule.signers.is_empty());

    Setup {
        e,
        account,
        recovery_rule: rule.id,
    }
}

fn no_signatures(e: &Env, rule: u32) -> AuthPayload {
    AuthPayload {
        signers: Map::new(e),
        context_rule_ids: vec![e, rule],
    }
}

fn self_call(s: &Setup, fn_name: &str, args: Vec<Val>) -> Vec<Context> {
    vec![
        &s.e,
        Context::Contract(ContractContext {
            contract: s.account.clone(),
            fn_name: Symbol::new(&s.e, fn_name),
            args,
        }),
    ]
}

#[test]
fn zero_signer_rule_is_decided_by_its_policy_alone() {
    let s = setup();
    let e = &s.e;
    let new_signer = Signer::Delegated(Address::generate(e));
    let payload = soroban_sdk::BytesN::from_array(e, &[7u8; 32]);
    let auth: Val = no_signatures(e, s.recovery_rule).into_val(e);

    let allowed = self_call(
        &s,
        "add_signer",
        vec![e, 0u32.into_val(e), new_signer.into_val(e)],
    );
    assert_eq!(
        e.try_invoke_contract_check_auth::<SmartAccountError>(&s.account, &payload, auth, &allowed),
        Ok(())
    );

    let refused = self_call(&s, "upgrade", vec![e, Bytes::new(e).into_val(e)]);
    assert!(e
        .try_invoke_contract_check_auth::<SmartAccountError>(&s.account, &payload, auth, &refused)
        .is_err());

    // The rule is CallContract(self): a call on any other contract never matches.
    let elsewhere = vec![
        e,
        Context::Contract(ContractContext {
            contract: Address::generate(e),
            fn_name: symbol_short!("transfer"),
            args: vec![e],
        }),
    ];
    assert!(e
        .try_invoke_contract_check_auth::<SmartAccountError>(&s.account, &payload, auth, &elsewhere)
        .is_err());
}

/// Through the real host auth path, not a direct `__check_auth` call: an
/// authorization entry for the account carrying an empty-signer payload
/// completes `add_signer`, including the policy's nested
/// `smart_account.require_auth()`.
#[test]
fn zero_signer_rule_completes_a_real_invocation() {
    let s = setup();
    let e = &s.e;
    let new_owner = Address::generate(e);
    let new_signer = Signer::Delegated(new_owner);
    let client = StablPasskeyMultiSignerClient::new(e, &s.account);

    // No authorization entry: refused, so the pass below is the rule's doing.
    assert!(client.try_add_signer(&0, &new_signer).is_err());

    let args: Vec<Val> = vec![e, 0u32.into_val(e), new_signer.into_val(e)];
    let xdr_args: std::vec::Vec<ScVal> = args
        .iter()
        .map(|v| ScVal::try_from_val(e, &v).unwrap())
        .collect();
    let signature_val: Val = no_signatures(e, s.recovery_rule).into_val(e);
    let signature = ScVal::try_from_val(e, &signature_val).unwrap();

    e.set_auths(&[SorobanAuthorizationEntry {
        credentials: SorobanCredentials::Address(SorobanAddressCredentials {
            address: ScAddress::try_from(s.account.clone()).unwrap(),
            nonce: 1,
            signature_expiration_ledger: e.ledger().sequence() + 100,
            signature,
        }),
        root_invocation: SorobanAuthorizedInvocation {
            function: SorobanAuthorizedFunction::ContractFn(InvokeContractArgs {
                contract_address: ScAddress::try_from(s.account.clone()).unwrap(),
                function_name: ScSymbol("add_signer".try_into().unwrap()),
                args: VecM::try_from(xdr_args).unwrap(),
            }),
            sub_invocations: VecM::default(),
        },
    }]);

    client.add_signer(&0, &new_signer);
    assert!(client.get_context_rule(&0).signers.contains(&new_signer));
}
