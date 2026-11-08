#![cfg(test)]
extern crate std;

use crate::contract::{
    Error, EscrowEntry, EscrowParams, EscrowState, EscrowStateChange, StablEscrow,
    StablEscrowClient,
};
use soroban_sdk::{
    events::Event as _,
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation, Events as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Bytes, BytesN, Env, IntoVal, String, Symbol,
};

fn create_token<'a>(
    env: &Env,
    admin: &Address,
) -> (Address, TokenClient<'a>, StellarAssetClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = sac.address();
    (
        token.clone(),
        TokenClient::new(env, &token),
        StellarAssetClient::new(env, &token),
    )
}

fn terms_hash(env: &Env, amount: &i128, expiry: &u64) -> BytesN<32> {
    let mut terms = Bytes::new(env);
    let quote_id = Bytes::from_slice(&env, &[1; 32]);
    let beneficiary_id = Bytes::from_slice(&env, &[1; 32]);
    terms.push_back(1u8); // format version
    terms.append(&quote_id);
    terms.append(&beneficiary_id);
    terms.extend_from_array(&amount.to_be_bytes());
    terms.extend_from_array(&expiry.to_be_bytes());
    env.crypto().sha256(&terms).to_bytes() // Hash<32> -> BytesN<32>
}

const NOW: u64 = 1_762_560_000;
const DAY: u64 = 24 * 60 * 60;
const STARTING_BALANCE: i128 = 1_000_0000000;
const TX_ID: &str = "018aecc6-4202-4882-8efb-7f173d551c3e";

struct Setup<'a> {
    env: Env,
    contract_id: Address,
    client: StablEscrowClient<'a>,
    token: Address,
    token_client: TokenClient<'a>,
    sac_client: StellarAssetClient<'a>,
    sender: Address,
    recipient: Address,
}

impl<'a> Setup<'a> {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(NOW);
        let admin = Address::generate(&env);
        let sender = Address::generate(&env);
        let recipient = Address::generate(&env);
        let (token, token_client, sac_client) = create_token(&env, &admin);
        sac_client.mint(&sender, &STARTING_BALANCE);
        let contract_id = env.register(StablEscrow, ());
        let client = StablEscrowClient::new(&env, &contract_id);
        Setup {
            env,
            contract_id,
            client,
            token,
            token_client,
            sac_client,
            sender,
            recipient,
        }
    }

    fn params(&self, amount: i128) -> EscrowParams {
        let expiry = NOW + DAY;
        EscrowParams {
            token: self.token.clone(),
            amount,
            expiry,
            terms_hash: terms_hash(&self.env, &amount, &expiry),
        }
    }

    fn tx_id(&self, id: &str) -> String {
        String::from_str(&self.env, id)
    }

    /// Lock via the fallible client so the test can assert on the error.
    fn try_lock(
        &self,
        sender: &Address,
        id: &str,
        params: &EscrowParams,
    ) -> Result<
        Result<EscrowEntry, soroban_sdk::ConversionError>,
        Result<Error, soroban_sdk::InvokeError>,
    > {
        self.client
            .try_lock_for_offramp(sender, &self.recipient, &self.tx_id(id), params)
    }

    /// Lock `amount` for `sender` under `id` and return the params used.
    fn lock(&self, sender: &Address, id: &str, amount: i128) -> EscrowParams {
        let params = self.params(amount);
        self.client
            .lock_for_offramp(sender, &self.recipient, &self.tx_id(id), &params);
        params
    }
}

#[test]
fn lock_moves_funds_to_escrow() {
    let s = Setup::new();
    let amount = 6_000_000_000;
    let params = s.params(amount);
    let entry = s
        .client
        .lock_for_offramp(&s.sender, &s.recipient, &s.tx_id(TX_ID), &params);
    assert_eq!(entry.state, EscrowState::Locked);
    assert_eq!(entry.recipient, s.recipient);
    assert_eq!(entry.params, params);
    assert_eq!(s.client.get_escrow_entry(&s.sender, &s.tx_id(TX_ID)), entry);
    assert_eq!(s.token_client.balance(&s.sender), STARTING_BALANCE - amount);
    assert_eq!(s.token_client.balance(&s.contract_id), amount);
}

// --- validations -----------------------------------------------------------

#[test]
fn lock_rejects_zero_amount() {
    let s = Setup::new();
    let res = s.try_lock(&s.sender, TX_ID, &s.params(0));
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
    assert_eq!(s.token_client.balance(&s.sender), STARTING_BALANCE);
}

#[test]
fn lock_rejects_negative_amount() {
    let s = Setup::new();
    let res = s.try_lock(&s.sender, TX_ID, &s.params(-1));
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn lock_rejects_duplicate_sender_and_transaction_id() {
    let s = Setup::new();
    let params = s.params(100_0000000);
    s.client
        .lock_for_offramp(&s.sender, &s.recipient, &s.tx_id(TX_ID), &params);

    let res = s.try_lock(&s.sender, TX_ID, &params);
    assert_eq!(res, Err(Ok(Error::Exists)));
    // Funds moved exactly once.
    assert_eq!(s.token_client.balance(&s.contract_id), 100_0000000);
    assert_eq!(
        s.token_client.balance(&s.sender),
        STARTING_BALANCE - 100_0000000
    );
}

#[test]
fn same_transaction_id_is_isolated_per_sender() {
    let s = Setup::new();
    let other = Address::generate(&s.env);
    s.sac_client.mint(&other, &STARTING_BALANCE);

    let a = s.params(100_0000000);
    let b = s.params(250_0000000);
    s.client
        .lock_for_offramp(&s.sender, &s.recipient, &s.tx_id(TX_ID), &a);
    s.client
        .lock_for_offramp(&other, &s.recipient, &s.tx_id(TX_ID), &b);

    assert_eq!(
        s.client.get_escrow_entry(&s.sender, &s.tx_id(TX_ID)).params,
        a
    );
    assert_eq!(s.client.get_escrow_entry(&other, &s.tx_id(TX_ID)).params, b);
    assert_eq!(s.token_client.balance(&s.contract_id), 350_0000000);
}

#[test]
fn lock_rejects_non_canonical_transaction_ids() {
    let s = Setup::new();
    let params = s.params(100_0000000);
    let bad = [
        "018AECC6-4202-4882-8EFB-7F173D551C3E",  // uppercase
        "018aecc6420248828efb7f173d551c3e",      // no dashes
        "018aecc6-4202-4882-8efb-7f173d551c3",   // 35 chars
        "018aecc6-4202-4882-8efb-7f173d551c3e0", // 37 chars
        "018aecc6-4202-4882-8efb_7f173d551c3e",  // wrong separator
        "018aecc6-4202-4882-8efb-7f173d551c3g",  // non-hex
        "018aecc64-202-4882-8efb-7f173d551c3e",  // dash in wrong place
        "",
    ];
    for id in bad {
        let res = s.try_lock(&s.sender, id, &params);
        assert_eq!(res, Err(Ok(Error::InvalidTransactionId)), "accepted {id:?}");
    }
    assert_eq!(s.token_client.balance(&s.sender), STARTING_BALANCE);
}

#[test]
fn failed_transfer_leaves_no_entry() {
    // Lock more than the sender holds: the token transfer fails, and the
    // entry written before it must roll back with it.
    let s = Setup::new();
    let res = s.try_lock(&s.sender, TX_ID, &s.params(STARTING_BALANCE + 1));
    assert!(res.is_err());
    assert_eq!(
        s.client.try_get_escrow_entry(&s.sender, &s.tx_id(TX_ID)),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(s.token_client.balance(&s.sender), STARTING_BALANCE);
}

#[test]
fn get_escrow_entry_missing_returns_error() {
    let s = Setup::new();
    assert_eq!(
        s.client.try_get_escrow_entry(&s.sender, &s.tx_id(TX_ID)),
        Err(Ok(Error::NotFound))
    );
}

#[test]
fn lock_rejects_expiry_in_the_past() {
    let s = Setup::new();
    let mut params = s.params(100_0000000);
    params.expiry = NOW - 1;
    assert_eq!(
        s.try_lock(&s.sender, TX_ID, &params),
        Err(Ok(Error::InvalidExpiry))
    );
}

#[test]
fn lock_accepts_expiry_equal_to_now() {
    // Expiry is inclusive: claim is allowed while now <= expiry, so an entry
    // expiring "now" is still claimable in this ledger.
    let s = Setup::new();
    let mut params = s.params(100_0000000);
    params.expiry = NOW;
    assert!(s.try_lock(&s.sender, TX_ID, &params).is_ok());
}

// --- event -----------------------------------------------------------------

#[test]
fn lock_publishes_escrow_locked_event() {
    let s = Setup::new();
    let params = s.params(100_0000000);
    s.client
        .lock_for_offramp(&s.sender, &s.recipient, &s.tx_id(TX_ID), &params);

    assert_eq!(
        s.env.events().all().filter_by_contract(&s.contract_id),
        [EscrowStateChange {
            recipient: s.recipient.clone(),
            sender: s.sender.clone(),
            transaction_id: s.tx_id(TX_ID),
            new_state: EscrowState::Locked,
            params,
        }
        .to_xdr(&s.env, &s.contract_id)]
    );
}

// --- auth ------------------------------------------------------------------

#[test]
fn lock_is_authorized_by_sender_only_with_nested_transfer() {
    let s = Setup::new();
    let params = s.params(100_0000000);
    let id = s.tx_id(TX_ID);
    s.client
        .lock_for_offramp(&s.sender, &s.recipient, &id, &params);

    // Exactly one signer (the sender), authorizing lock with the token
    // transfer nested under it: one signature covers both.
    assert_eq!(
        s.env.auths(),
        std::vec![(
            s.sender.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    s.contract_id.clone(),
                    Symbol::new(&s.env, "lock_for_offramp"),
                    (s.sender.clone(), s.recipient.clone(), id, params.clone()).into_val(&s.env),
                )),
                sub_invocations: std::vec![AuthorizedInvocation {
                    function: AuthorizedFunction::Contract((
                        s.token.clone(),
                        Symbol::new(&s.env, "transfer"),
                        (s.sender.clone(), s.contract_id.clone(), params.amount).into_val(&s.env),
                    )),
                    sub_invocations: std::vec![],
                }],
            }
        )]
    );
}

#[test]
fn lock_without_sender_auth_fails() {
    let s = Setup::new();
    s.env.set_auths(&[]); // turn off mock_all_auths: no signatures at all
    let res = s.try_lock(&s.sender, TX_ID, &s.params(100_0000000));
    assert!(res.is_err());
    assert_eq!(s.token_client.balance(&s.sender), STARTING_BALANCE);
}

// --- release ---------------------------------------------------------------

#[test]
fn release_pays_recipient_and_marks_claimed() {
    let s = Setup::new();
    let params = s.lock(&s.sender, TX_ID, 100_0000000);

    let entry = s.client.release_escrow(&s.sender, &s.tx_id(TX_ID));

    assert_eq!(entry.state, EscrowState::Claimed);
    assert_eq!(entry.params, params);
    assert_eq!(s.token_client.balance(&s.recipient), 100_0000000);
    assert_eq!(s.token_client.balance(&s.contract_id), 0);
    // The new state must be persisted, not just returned.
    assert_eq!(
        s.client.get_escrow_entry(&s.sender, &s.tx_id(TX_ID)).state,
        EscrowState::Claimed
    );
}

#[test]
fn release_twice_fails_and_cannot_drain_other_escrows() {
    // Two customers' funds sit in the same pooled contract balance. A second
    // release of the first escrow must not pay out of the second one.
    let s = Setup::new();
    let other = Address::generate(&s.env);
    s.sac_client.mint(&other, &STARTING_BALANCE);
    s.lock(&s.sender, TX_ID, 100_0000000);
    s.lock(&other, TX_ID, 100_0000000);

    s.client.release_escrow(&s.sender, &s.tx_id(TX_ID));
    assert_eq!(
        s.client.try_release_escrow(&s.sender, &s.tx_id(TX_ID)),
        Err(Ok(Error::InvalidState))
    );

    assert_eq!(s.token_client.balance(&s.recipient), 100_0000000);
    assert_eq!(s.token_client.balance(&s.contract_id), 100_0000000);
    assert_eq!(
        s.client.get_escrow_entry(&other, &s.tx_id(TX_ID)).state,
        EscrowState::Locked
    );
}

#[test]
fn release_allowed_at_exact_expiry() {
    let s = Setup::new();
    let params = s.lock(&s.sender, TX_ID, 100_0000000);
    s.env.ledger().set_timestamp(params.expiry);
    let entry = s.client.release_escrow(&s.sender, &s.tx_id(TX_ID));
    assert_eq!(entry.state, EscrowState::Claimed);
}

#[test]
fn release_after_expiry_fails() {
    let s = Setup::new();
    let params = s.lock(&s.sender, TX_ID, 100_0000000);
    s.env.ledger().set_timestamp(params.expiry + 1);
    assert_eq!(
        s.client.try_release_escrow(&s.sender, &s.tx_id(TX_ID)),
        Err(Ok(Error::Expired))
    );
    assert_eq!(s.token_client.balance(&s.contract_id), 100_0000000);
    assert_eq!(
        s.client.get_escrow_entry(&s.sender, &s.tx_id(TX_ID)).state,
        EscrowState::Locked
    );
}

#[test]
fn release_missing_entry_fails() {
    let s = Setup::new();
    assert_eq!(
        s.client.try_release_escrow(&s.sender, &s.tx_id(TX_ID)),
        Err(Ok(Error::NotFound))
    );
}

#[test]
fn release_only_touches_its_own_sender_entry() {
    let s = Setup::new();
    let other = Address::generate(&s.env);
    s.sac_client.mint(&other, &STARTING_BALANCE);
    s.lock(&s.sender, TX_ID, 100_0000000);
    s.lock(&other, TX_ID, 250_0000000);

    s.client.release_escrow(&other, &s.tx_id(TX_ID));

    assert_eq!(s.token_client.balance(&s.recipient), 250_0000000);
    assert_eq!(
        s.client.get_escrow_entry(&s.sender, &s.tx_id(TX_ID)).state,
        EscrowState::Locked
    );
}

#[test]
fn release_is_authorized_by_stored_recipient_only() {
    let s = Setup::new();
    s.lock(&s.sender, TX_ID, 100_0000000);
    let id = s.tx_id(TX_ID);

    s.client.release_escrow(&s.sender, &id);

    // Only the recipient bound at lock time signs. The payout transfer is
    // made by the contract itself, so it needs no external signature.
    assert_eq!(
        s.env.auths(),
        std::vec![(
            s.recipient.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    s.contract_id.clone(),
                    Symbol::new(&s.env, "release_escrow"),
                    (s.sender.clone(), id).into_val(&s.env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );
}

#[test]
fn release_without_recipient_auth_fails() {
    let s = Setup::new();
    s.lock(&s.sender, TX_ID, 100_0000000);
    s.env.set_auths(&[]); // no signatures: not even the recipient's

    assert!(s
        .client
        .try_release_escrow(&s.sender, &s.tx_id(TX_ID))
        .is_err());
    assert_eq!(s.token_client.balance(&s.recipient), 0);
    assert_eq!(s.token_client.balance(&s.contract_id), 100_0000000);
    assert_eq!(
        s.client.get_escrow_entry(&s.sender, &s.tx_id(TX_ID)).state,
        EscrowState::Locked
    );
}

#[test]
fn release_publishes_claimed_event() {
    let s = Setup::new();
    let params = s.lock(&s.sender, TX_ID, 100_0000000);
    s.client.release_escrow(&s.sender, &s.tx_id(TX_ID));

    let events = s.env.events().all().filter_by_contract(&s.contract_id);
    assert_eq!(
        events,
        [EscrowStateChange {
            recipient: s.recipient.clone(),
            sender: s.sender.clone(),
            transaction_id: s.tx_id(TX_ID),
            new_state: EscrowState::Claimed,
            params,
        }
        .to_xdr(&s.env, &s.contract_id)]
    );
}
