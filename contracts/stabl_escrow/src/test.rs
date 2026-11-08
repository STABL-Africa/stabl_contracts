#![cfg(test)]
extern crate std;

use crate::contract::{EscrowParams, StablEscrow, StablEscrowClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Bytes, BytesN, Env, String,
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

#[test]
fn lock_moves_funds_to_escrow() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_762_560_000);
    let expiry = env.ledger().timestamp() + 24 * 60 * 60;
    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let (token, token_client, sac_client) = create_token(&env, &admin);
    sac_client.mint(&sender, &1_000_0000000);
    let contract_id = env.register(StablEscrow, ());
    let client = StablEscrowClient::new(&env, &contract_id);

    let recipient = Address::generate(&env);
    let amount = 5_000_000_000;
    let transaction_id = String::from_str(&env, "018aecc6-4202-4882-8efb-7f173d551c3e");
    let params = EscrowParams {
        token: token,
        amount: 5_000_000_000,
        recipient: recipient,
        expiry: expiry,
        terms_hash: terms_hash(&env, &amount, &expiry),
    };
    client.lock_for_offramp(&sender, &transaction_id, &params);
    std::dbg!(token_client.balance(&sender));
    std::dbg!(token_client.balance(&contract_id));
    std::dbg!(client.get_escrow_entry(&sender, &transaction_id));
}
