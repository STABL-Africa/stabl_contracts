#![cfg(test)]
extern crate std;
use crate::contract::{StablPasskeyMultiSigner, StablPasskeyMultiSignerClient};
use ed25519_dalek::SigningKey;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::xdr::{AccountId, PublicKey, ScAddress, Uint256};
use soroban_sdk::TryFromVal;
use soroban_sdk::{vec, Address, BytesN, Env, Map, Val};
use stellar_accounts::policies;
use stellar_accounts::smart_account::Signer;
use stellar_accounts::verifiers::webauthn::{self, WebAuthnSigData};

fn g_address(env: &Env, sk: &SigningKey) -> Address {
    let pk = sk.verifying_key().to_bytes();
    let sc = ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(pk))));
    Address::try_from_val(env, &sc).unwrap()
}

#[test]
fn constructor_sets_context_rule_with_g_addr() {
    let env = Env::default();
    let policies = Map::<Address, Val>::new(&env);
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let g = g_address(&env, &sk);

    let contract_id = env.register(
        StablPasskeyMultiSigner,
        (
            vec![
                &env,
                Signer::Delegated(Address::generate(&env)),
                Signer::Delegated(g.clone()),
            ],
            policies,
        ),
    );

    let account = StablPasskeyMultiSignerClient::new(&env, &contract_id);

    assert_eq!(&account.get_context_rule(&0).signer_ids.len(), &2);
}
