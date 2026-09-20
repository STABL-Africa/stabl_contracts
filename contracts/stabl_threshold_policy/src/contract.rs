//! # Stabl Threshold Policy Contract
//!
//! M-of-N signature threshold policy for smart accounts. Deployed once per
//! network and attached to a context rule as
//! `policies: { <this contract>: SimpleThresholdAccountParams { threshold } }`.
//!
//! Without a policy a context rule demands every one of its signers. With this
//! policy installed, any `threshold` of them suffice. Storage is keyed by
//! `(smart_account, context_rule_id)`, so one deployment serves every account
//! and each account can only touch its own entry: every mutating call does
//! `smart_account.require_auth()`, which the account satisfies simply by being
//! the caller.
//!
//! Like the verifiers this contract has no admin and no upgrade path.
//!
//! ## Threshold and signer set can drift
//!
//! The policy is not told when signers are added to or removed from the rule.
//! Removing signers below the threshold bricks the rule; adding signers
//! silently weakens it. Off-chain clients must bundle `set_threshold` with any
//! signer change, in the same transaction. See the `simple_threshold` module
//! docs in `stellar-accounts` for the full warning.

use soroban_sdk::{auth::Context, contract, contractimpl, Address, Env, Vec};
use stellar_accounts::{
    policies::{simple_threshold, Policy},
    smart_account::{ContextRule, Signer},
};

#[contract]
pub struct StablThresholdPolicyContract;

#[contractimpl]
impl Policy for StablThresholdPolicyContract {
    type AccountParams = simple_threshold::SimpleThresholdAccountParams;

    /// Called by the smart account during `__check_auth`. Passes when at least
    /// `threshold` of the rule's signers authenticated, panics with
    /// `SimpleThresholdError::NotAllowed` otherwise.
    fn enforce(
        e: &Env,
        context: Context,
        authenticated_signers: Vec<Signer>,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        simple_threshold::enforce(
            e,
            &context,
            &authenticated_signers,
            &context_rule,
            &smart_account,
        )
    }

    /// Called by the smart account when the policy is attached to a rule.
    /// Rejects a threshold of zero or one larger than the rule's signer count.
    fn install(
        e: &Env,
        install_params: Self::AccountParams,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        simple_threshold::install(e, &install_params, &context_rule, &smart_account)
    }

    /// Called by the smart account when the policy is detached from a rule.
    fn uninstall(e: &Env, context_rule: ContextRule, smart_account: Address) {
        simple_threshold::uninstall(e, &context_rule, &smart_account)
    }
}

#[contractimpl]
impl StablThresholdPolicyContract {
    /// Current threshold for `smart_account`'s rule `context_rule_id`.
    pub fn get_threshold(e: &Env, context_rule_id: u32, smart_account: Address) -> u32 {
        simple_threshold::get_threshold(e, context_rule_id, &smart_account)
    }

    /// Change the threshold. Must be authorized by `smart_account`, so this is
    /// invoked through the account itself, alongside any signer change.
    pub fn set_threshold(
        e: Env,
        threshold: u32,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        simple_threshold::set_threshold(&e, threshold, &context_rule, &smart_account)
    }
}
