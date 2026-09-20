#![cfg(test)]
extern crate std;

use soroban_sdk::auth::{Context, ContractContext};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{
    symbol_short, vec, Address, ConversionError, Env, Error, InvokeError, String, Vec,
};
use stellar_accounts::policies::simple_threshold::{
    SimpleThresholdAccountParams, SimpleThresholdError,
};
use stellar_accounts::smart_account::{ContextRule, ContextRuleType, Signer};

use crate::contract::{StablThresholdPolicyContract, StablThresholdPolicyContractClient};

fn client(e: &Env) -> StablThresholdPolicyContractClient<'_> {
    StablThresholdPolicyContractClient::new(e, &e.register(StablThresholdPolicyContract, ()))
}

/// A rule with `n` delegated signers. Only `id` and `signers.len()` matter
/// to the policy.
fn rule(e: &Env, id: u32, n: u32) -> ContextRule {
    let signers: Vec<Signer> =
        Vec::from_iter(e, (0..n).map(|_| Signer::Delegated(Address::generate(e))));
    ContextRule {
        id,
        context_type: ContextRuleType::Default,
        name: String::from_str(e, "default"),
        signers,
        signer_ids: Vec::from_iter(e, 0..n),
        policies: vec![e],
        policy_ids: vec![e],
        valid_until: None,
    }
}

fn params(threshold: u32) -> SimpleThresholdAccountParams {
    SimpleThresholdAccountParams { threshold }
}

/// The policy panics with `SimpleThresholdError` codes rather than returning
/// them, so `try_*` surfaces them as a generic contract error. This is the
/// shape a `try_*` call returns for a given code.
fn failed(
    err: SimpleThresholdError,
) -> Result<Result<(), ConversionError>, Result<Error, InvokeError>> {
    Err(Ok(err.into()))
}

fn failed_u32(
    err: SimpleThresholdError,
) -> Result<Result<u32, ConversionError>, Result<Error, InvokeError>> {
    Err(Ok(err.into()))
}

fn some_context(e: &Env) -> Context {
    Context::Contract(ContractContext {
        contract: Address::generate(e),
        fn_name: symbol_short!("transfer"),
        args: vec![e],
    })
}

#[test]
fn install_then_get_threshold() {
    let e = Env::default();
    e.mock_all_auths();
    let c = client(&e);
    let account = Address::generate(&e);
    let r = rule(&e, 0, 3);

    c.install(&params(2), &r, &account);
    assert_eq!(c.get_threshold(&0, &account), 2);
}

#[test]
fn install_rejects_zero_and_oversized_threshold() {
    let e = Env::default();
    e.mock_all_auths();
    let c = client(&e);
    let account = Address::generate(&e);
    let r = rule(&e, 0, 2);

    assert_eq!(
        c.try_install(&params(0), &r, &account),
        failed(SimpleThresholdError::InvalidThreshold)
    );
    assert_eq!(
        c.try_install(&params(3), &r, &account),
        failed(SimpleThresholdError::InvalidThreshold)
    );
}

#[test]
fn install_twice_for_same_rule_is_rejected() {
    let e = Env::default();
    e.mock_all_auths();
    let c = client(&e);
    let account = Address::generate(&e);
    let r = rule(&e, 0, 2);

    c.install(&params(1), &r, &account);
    assert_eq!(
        c.try_install(&params(2), &r, &account),
        failed(SimpleThresholdError::AlreadyInstalled)
    );
}

#[test]
fn storage_is_per_account_and_per_rule() {
    let e = Env::default();
    e.mock_all_auths();
    let c = client(&e);
    let alice = Address::generate(&e);
    let bob = Address::generate(&e);

    c.install(&params(1), &rule(&e, 0, 3), &alice);
    c.install(&params(2), &rule(&e, 1, 3), &alice);
    c.install(&params(3), &rule(&e, 0, 3), &bob);

    assert_eq!(c.get_threshold(&0, &alice), 1);
    assert_eq!(c.get_threshold(&1, &alice), 2);
    assert_eq!(c.get_threshold(&0, &bob), 3);
    assert_eq!(
        c.try_get_threshold(&1, &bob),
        failed_u32(SimpleThresholdError::SmartAccountNotInstalled)
    );
}

#[test]
fn enforce_passes_at_threshold_and_fails_below() {
    let e = Env::default();
    e.mock_all_auths();
    let c = client(&e);
    let account = Address::generate(&e);
    let r = rule(&e, 0, 3);
    c.install(&params(2), &r, &account);

    let one: Vec<Signer> = vec![&e, r.signers.get_unchecked(0)];
    let two: Vec<Signer> = vec![&e, r.signers.get_unchecked(0), r.signers.get_unchecked(1)];

    assert_eq!(
        c.try_enforce(&some_context(&e), &one, &r, &account),
        failed(SimpleThresholdError::NotAllowed)
    );
    c.enforce(&some_context(&e), &two, &r, &account);
    c.enforce(&some_context(&e), &r.signers, &r, &account);
}

#[test]
fn set_threshold_changes_enforcement() {
    let e = Env::default();
    e.mock_all_auths();
    let c = client(&e);
    let account = Address::generate(&e);
    let r = rule(&e, 0, 2);
    c.install(&params(2), &r, &account);

    let one: Vec<Signer> = vec![&e, r.signers.get_unchecked(0)];
    assert!(c
        .try_enforce(&some_context(&e), &one, &r, &account)
        .is_err());

    c.set_threshold(&1, &r, &account);
    assert_eq!(c.get_threshold(&0, &account), 1);
    c.enforce(&some_context(&e), &one, &r, &account);
}

#[test]
fn uninstall_removes_entry() {
    let e = Env::default();
    e.mock_all_auths();
    let c = client(&e);
    let account = Address::generate(&e);
    let r = rule(&e, 0, 2);

    c.install(&params(1), &r, &account);
    c.uninstall(&r, &account);
    assert_eq!(
        c.try_get_threshold(&0, &account),
        failed_u32(SimpleThresholdError::SmartAccountNotInstalled)
    );
    assert_eq!(
        c.try_uninstall(&r, &account),
        failed(SimpleThresholdError::SmartAccountNotInstalled)
    );
}

#[test]
fn mutations_require_account_auth() {
    // No mock_all_auths: the policy demands smart_account.require_auth(), and
    // a bare address in a test has nobody to provide it.
    let e = Env::default();
    let c = client(&e);
    let account = Address::generate(&e);
    let r = rule(&e, 0, 2);

    assert!(c.try_install(&params(1), &r, &account).is_err());
}
