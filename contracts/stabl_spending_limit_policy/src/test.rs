#![cfg(test)]
extern crate std;

use soroban_sdk::auth::{Context, ContractContext};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{
    symbol_short, vec, Address, ConversionError, Env, Error, IntoVal, InvokeError, String, Symbol,
    Vec,
};
use stellar_accounts::policies::spending_limit::{SpendingLimitAccountParams, SpendingLimitError};
use stellar_accounts::smart_account::{ContextRule, ContextRuleType, Signer};

use crate::contract::{StablSpendingLimitPolicyContract, StablSpendingLimitPolicyContractClient};

const DAY: u32 = 17280;

fn client(e: &Env) -> StablSpendingLimitPolicyContractClient<'_> {
    StablSpendingLimitPolicyContractClient::new(
        e,
        &e.register(StablSpendingLimitPolicyContract, ()),
    )
}

/// A `CallContract(token)` rule with one delegated signer. Only `id`,
/// `context_type` and a non-empty signer list matter to the policy.
fn rule(e: &Env, id: u32, token: &Address) -> ContextRule {
    ContextRule {
        id,
        context_type: ContextRuleType::CallContract(token.clone()),
        name: String::from_str(e, "small"),
        signers: vec![e, Signer::Delegated(Address::generate(e))],
        signer_ids: vec![e, 0],
        policies: vec![e],
        policy_ids: vec![e],
        valid_until: None,
    }
}

fn params(spending_limit: i128, period_ledgers: u32) -> SpendingLimitAccountParams {
    SpendingLimitAccountParams {
        spending_limit,
        period_ledgers,
    }
}

/// The policy panics with `SpendingLimitError` codes rather than returning
/// them, so `try_*` surfaces them as a generic contract error. This is the
/// shape a `try_*` call returns for a given code.
fn failed(
    err: SpendingLimitError,
) -> Result<Result<(), ConversionError>, Result<Error, InvokeError>> {
    Err(Ok(err.into()))
}

fn call(e: &Env, token: &Address, fn_name: Symbol, amount: i128) -> Context {
    Context::Contract(ContractContext {
        contract: token.clone(),
        fn_name,
        args: vec![
            e,
            Address::generate(e).into_val(e),
            Address::generate(e).into_val(e),
            amount.into_val(e),
        ],
    })
}

fn transfer(e: &Env, token: &Address, amount: i128) -> Context {
    call(e, token, symbol_short!("transfer"), amount)
}

struct Setup {
    e: Env,
    policy: Address,
    account: Address,
    token: Address,
    rule: ContextRule,
}

impl Setup {
    fn new(limit: i128, period: u32) -> Self {
        let e = Env::default();
        e.mock_all_auths();
        // OZ evicts entries at or below `current - period` (saturating), so at
        // ledger 0 every entry would be evicted at once. Start somewhere real.
        e.ledger().set_sequence_number(1_000_000);
        let policy = e.register(StablSpendingLimitPolicyContract, ());
        let account = Address::generate(&e);
        let token = Address::generate(&e);
        let rule = rule(&e, 1, &token);
        StablSpendingLimitPolicyContractClient::new(&e, &policy).install(
            &params(limit, period),
            &rule,
            &account,
        );
        Setup {
            e,
            policy,
            account,
            token,
            rule,
        }
    }

    fn c(&self) -> StablSpendingLimitPolicyContractClient<'_> {
        StablSpendingLimitPolicyContractClient::new(&self.e, &self.policy)
    }

    fn spend(
        &self,
        amount: i128,
    ) -> Result<Result<(), ConversionError>, Result<Error, InvokeError>> {
        self.c().try_enforce(
            &transfer(&self.e, &self.token, amount),
            &self.rule.signers,
            &self.rule,
            &self.account,
        )
    }
}

#[test]
fn install_then_read_back() {
    let s = Setup::new(500, DAY);
    let data = s.c().get_spending_limit_data(&1, &s.account);
    assert_eq!(data.spending_limit, 500);
    assert_eq!(data.period_ledgers, DAY);
    assert_eq!(data.cached_total_spent, 0);
}

#[test]
fn install_rejects_bad_params_and_non_call_contract_rule() {
    let e = Env::default();
    e.mock_all_auths();
    let c = client(&e);
    let account = Address::generate(&e);
    let r = rule(&e, 1, &Address::generate(&e));

    for bad in [params(0, DAY), params(-1, DAY), params(100, 0)] {
        assert_eq!(
            c.try_install(&bad, &r, &account),
            failed(SpendingLimitError::InvalidLimitOrPeriod)
        );
    }

    let mut default_rule = r.clone();
    default_rule.context_type = ContextRuleType::Default;
    assert_eq!(
        c.try_install(&params(100, DAY), &default_rule, &account),
        failed(SpendingLimitError::OnlyCallContractAllowed)
    );
}

#[test]
fn install_twice_for_same_rule_is_rejected() {
    let s = Setup::new(500, DAY);
    assert_eq!(
        s.c().try_install(&params(100, DAY), &s.rule, &s.account),
        failed(SpendingLimitError::AlreadyInstalled)
    );
}

#[test]
fn transfer_at_limit_passes_one_over_fails() {
    let s = Setup::new(500, DAY);
    assert_eq!(
        s.spend(501),
        failed(SpendingLimitError::SpendingLimitExceeded)
    );
    assert_eq!(s.spend(500), Ok(Ok(())));
    assert_eq!(
        s.spend(1),
        failed(SpendingLimitError::SpendingLimitExceeded)
    );
}

#[test]
fn cumulative_transfers_crossing_limit_fail() {
    let s = Setup::new(500, DAY);
    assert_eq!(s.spend(300), Ok(Ok(())));
    assert_eq!(
        s.spend(201),
        failed(SpendingLimitError::SpendingLimitExceeded)
    );
    assert_eq!(s.spend(200), Ok(Ok(())));
    assert_eq!(
        s.c()
            .get_spending_limit_data(&1, &s.account)
            .cached_total_spent,
        500
    );
}

#[test]
fn window_rolls_after_period() {
    let s = Setup::new(500, DAY);
    let start = s.e.ledger().sequence();
    assert_eq!(s.spend(500), Ok(Ok(())));

    // One ledger short of the window: still spent.
    s.e.ledger().set_sequence_number(start + DAY - 1);
    assert_eq!(
        s.spend(1),
        failed(SpendingLimitError::SpendingLimitExceeded)
    );

    // Entry at `start` is evicted once `start <= current - period`.
    s.e.ledger().set_sequence_number(start + DAY);
    assert_eq!(s.spend(500), Ok(Ok(())));
}

#[test]
fn zero_transfer_passes_and_is_not_recorded() {
    let s = Setup::new(500, DAY);
    assert_eq!(s.spend(500), Ok(Ok(())));
    assert_eq!(s.spend(0), Ok(Ok(())));
    assert_eq!(
        s.c()
            .get_spending_limit_data(&1, &s.account)
            .spending_history
            .len(),
        1
    );
}

#[test]
fn negative_transfer_is_rejected() {
    let s = Setup::new(500, DAY);
    assert_eq!(s.spend(-1), failed(SpendingLimitError::LessThanZero));
}

#[test]
fn non_transfer_functions_are_rejected_not_passed_through() {
    let s = Setup::new(500, DAY);
    for f in [
        symbol_short!("approve"),
        Symbol::new(&s.e, "transfer_from"),
        symbol_short!("burn"),
    ] {
        assert_eq!(
            s.c().try_enforce(
                &call(&s.e, &s.token, f, 1),
                &s.rule.signers,
                &s.rule,
                &s.account
            ),
            failed(SpendingLimitError::NotAllowed)
        );
    }
}

#[test]
fn enforce_without_authenticated_signers_is_rejected() {
    let s = Setup::new(500, DAY);
    let none: Vec<Signer> = vec![&s.e];
    assert_eq!(
        s.c()
            .try_enforce(&transfer(&s.e, &s.token, 1), &none, &s.rule, &s.account),
        failed(SpendingLimitError::NotAllowed)
    );
}

#[test]
fn storage_is_per_account_and_per_rule() {
    let s = Setup::new(500, DAY);
    let other_rule = rule(&s.e, 2, &s.token);
    let bob = Address::generate(&s.e);
    s.c().install(&params(100, DAY), &other_rule, &s.account);
    s.c().install(&params(50, DAY), &s.rule, &bob);

    assert_eq!(s.spend(500), Ok(Ok(())));
    // Rule 2 of the same account and rule 1 of bob are untouched.
    s.c().enforce(
        &transfer(&s.e, &s.token, 100),
        &other_rule.signers,
        &other_rule,
        &s.account,
    );
    s.c().enforce(
        &transfer(&s.e, &s.token, 50),
        &s.rule.signers,
        &s.rule,
        &bob,
    );

    assert_eq!(
        s.c()
            .get_spending_limit_data(&1, &s.account)
            .cached_total_spent,
        500
    );
    assert_eq!(
        s.c()
            .get_spending_limit_data(&2, &s.account)
            .cached_total_spent,
        100
    );
    assert_eq!(
        s.c().get_spending_limit_data(&1, &bob).cached_total_spent,
        50
    );
}

#[test]
fn set_spending_limit_changes_enforcement() {
    let s = Setup::new(500, DAY);
    assert_eq!(s.spend(400), Ok(Ok(())));
    s.c().set_spending_limit(&1000, &s.rule, &s.account);
    assert_eq!(s.spend(600), Ok(Ok(())));
    assert_eq!(
        s.spend(1),
        failed(SpendingLimitError::SpendingLimitExceeded)
    );

    assert_eq!(
        s.c().try_set_spending_limit(&0, &s.rule, &s.account),
        failed(SpendingLimitError::InvalidLimitOrPeriod)
    );
}

#[test]
fn uninstall_removes_entry() {
    let s = Setup::new(500, DAY);
    s.c().uninstall(&s.rule, &s.account);
    assert!(s.c().try_get_spending_limit_data(&1, &s.account).is_err());
    assert_eq!(
        s.c().try_uninstall(&s.rule, &s.account),
        failed(SpendingLimitError::SmartAccountNotInstalled)
    );
}

#[test]
fn mutations_require_account_auth() {
    // No mock_all_auths: the policy demands smart_account.require_auth(), and
    // a bare address in a test has nobody to provide it.
    let e = Env::default();
    let c = client(&e);
    let account = Address::generate(&e);
    let r = rule(&e, 1, &Address::generate(&e));

    assert!(c.try_install(&params(100, DAY), &r, &account).is_err());
}
