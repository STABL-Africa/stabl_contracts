//! # Stabl Spending Limit Policy Contract
//!
//! Rolling-window spending cap for smart accounts. Deployed once per network
//! and attached to a per-token context rule as
//! `policies: { <this contract>: SpendingLimitAccountParams { spending_limit, period_ledgers } }`.
//!
//! The intended pairing is a `CallContract(token)` rule with a low threshold
//! (e.g. 1-of-3) plus this cap, next to a stricter `Default` rule. Small
//! transfers go through the cheap rule; anything over the cap is resubmitted
//! under the default rule with more signatures.
//!
//! Storage is keyed by `(smart_account, context_rule_id)`, so one deployment
//! serves every account and each account can only touch its own entry. Like
//! the verifiers and the threshold policy, this contract has no admin and no
//! upgrade path.
//!
//! ## What is metered
//!
//! - The rule must be `ContextRuleType::CallContract(token)`; install rejects
//!   anything else. One rule per token, amounts in that token's base units
//!   (stroops for 7-decimal SACs).
//! - Only a function literally named `transfer` is allowed, with the amount
//!   read from argument index 2 as `i128`. Every other function under the
//!   rule (`approve`, `transfer_from`, `burn`, ...) is rejected with
//!   `NotAllowed`, not passed through. Those calls must use another rule.
//! - Zero-amount transfers are permitted and not recorded.
//! - `period_ledgers` is a rolling window, not a calendar day. About 17280
//!   ledgers per day at ~5s ledgers.
//! - At most 1000 transfers are tracked per window; beyond that the rule
//!   fails with `HistoryCapacityExceeded` until old entries roll off.

use soroban_sdk::{auth::Context, contract, contractimpl, Address, Env, Vec};
use stellar_accounts::{
    policies::{spending_limit, Policy},
    smart_account::{ContextRule, Signer},
};

#[contract]
pub struct StablSpendingLimitPolicyContract;

#[contractimpl]
impl Policy for StablSpendingLimitPolicyContract {
    type AccountParams = spending_limit::SpendingLimitAccountParams;

    /// Called by the smart account during `__check_auth`. Records the
    /// transfer and passes when the window total stays within the limit,
    /// panics with `SpendingLimitError` otherwise.
    fn enforce(
        e: &Env,
        context: Context,
        authenticated_signers: Vec<Signer>,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        spending_limit::enforce(
            e,
            &context,
            &authenticated_signers,
            &context_rule,
            &smart_account,
        )
    }

    /// Called by the smart account when the policy is attached to a rule.
    /// Rejects non-`CallContract` rules, a non-positive limit and a zero
    /// period.
    fn install(
        e: &Env,
        install_params: Self::AccountParams,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        spending_limit::install(e, &install_params, &context_rule, &smart_account)
    }

    /// Called by the smart account when the policy is detached from a rule.
    fn uninstall(e: &Env, context_rule: ContextRule, smart_account: Address) {
        spending_limit::uninstall(e, &context_rule, &smart_account)
    }
}

#[contractimpl]
impl StablSpendingLimitPolicyContract {
    /// Limit, period and in-window history for `smart_account`'s rule
    /// `context_rule_id`. The cached total may include entries that have
    /// aged out but not yet been evicted; eviction happens on `enforce`.
    pub fn get_spending_limit_data(
        e: &Env,
        context_rule_id: u32,
        smart_account: Address,
    ) -> spending_limit::SpendingLimitData {
        spending_limit::get_spending_limit_data(e, context_rule_id, &smart_account)
    }

    /// Change the limit. Must be authorized by `smart_account`, so this is
    /// invoked through the account itself. The period cannot be changed;
    /// uninstall and reinstall for that.
    pub fn set_spending_limit(
        e: Env,
        spending_limit: i128,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        spending_limit::set_spending_limit(&e, spending_limit, &context_rule, &smart_account)
    }
}
