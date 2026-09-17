#![cfg(test)]
extern crate std;
use super::*;
use ed25519_dalek::{Signer as _, SigningKey};
use soroban_sdk::testutils::{BytesN as _, Events as _};
use soroban_sdk::xdr::{ContractEventBody, ScBytes, ScSymbol, ScVal, ScVec};
use soroban_sdk::{vec, Env, IntoVal};

fn signer_pk(env: &Env, sk: &SigningKey) -> BytesN<32> {
    BytesN::from_array(env, &sk.verifying_key().to_bytes())
}

fn sign(env: &Env, sk: &SigningKey, payload: &BytesN<32>) -> Ed25519Signature {
    Ed25519Signature {
        public_key: signer_pk(env, sk),
        signature: BytesN::from_array(env, &sk.sign(&payload.to_array()).to_bytes()),
    }
}

#[test]
fn constructor_stores_signers() {
    let env = Env::default();
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let pk = signer_pk(&env, &sk);
    let contract_id = env.register(SmartAccount, (vec![&env, pk.clone()],));
    let client = SmartAccountClient::new(&env, &contract_id);
    assert_eq!(client.signers(), vec![&env, pk]);
}

#[test]
fn set_signers_emits_rotated_event() {
    let env = Env::default();
    env.mock_all_auths();
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let contract_id = env.register(SmartAccount, (vec![&env, signer_pk(&env, &sk)],));
    let client = SmartAccountClient::new(&env, &contract_id);

    let new_pk = signer_pk(&env, &SigningKey::from_bytes(&[9u8; 32]));
    client.set_signers(&vec![&env, new_pk.clone()]);

    let all = env.events().all().filter_by_contract(&contract_id);
    let ContractEventBody::V0(body) = &all.events().last().unwrap().body;

    let sym = |s: &str| ScVal::Symbol(ScSymbol(s.try_into().unwrap()));
    assert_eq!(
        body.topics.to_vec(),
        std::vec![sym("signers"), sym("rotated")]
    );

    let expected_data = ScVal::Vec(Some(ScVec(
        std::vec![ScVal::Bytes(ScBytes(
            new_pk.to_array().to_vec().try_into().unwrap()
        ))]
        .try_into()
        .unwrap(),
    )));
    assert_eq!(body.data, expected_data);
}

#[test]
fn check_auth_accepts_known_signer() {
    let env = Env::default();
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let contract_id = env.register(SmartAccount, (vec![&env, signer_pk(&env, &sk)],));

    let payload = BytesN::random(&env);
    let sigs = vec![&env, sign(&env, &sk, &payload)];

    env.try_invoke_contract_check_auth::<Error>(
        &contract_id,
        &payload,
        sigs.into_val(&env),
        &vec![&env],
    )
    .unwrap();
}

#[test]
fn check_auth_rejects_unknown_signer() {
    let env = Env::default();
    let known = SigningKey::from_bytes(&[7u8; 32]);
    let unknown = SigningKey::from_bytes(&[9u8; 32]);
    let contract_id = env.register(SmartAccount, (vec![&env, signer_pk(&env, &known)],));

    let payload = BytesN::random(&env);
    let sigs = vec![&env, sign(&env, &unknown, &payload)];

    let res = env.try_invoke_contract_check_auth::<Error>(
        &contract_id,
        &payload,
        sigs.into_val(&env),
        &vec![&env],
    );
    assert_eq!(res.unwrap_err().unwrap(), Error::UnknownSigner);
}

#[test]
fn check_auth_rejects_empty_signatures() {
    let env = Env::default();
    let known = SigningKey::from_bytes(&[7u8; 32]);
    let contract_id = env.register(SmartAccount, (vec![&env, signer_pk(&env, &known)],));

    let payload = BytesN::random(&env);
    let sigs: Vec<Ed25519Signature> = vec![&env];

    let res = env.try_invoke_contract_check_auth::<Error>(
        &contract_id,
        &payload,
        sigs.into_val(&env),
        &vec![&env],
    );
    assert_eq!(res.unwrap_err().unwrap(), Error::NoSignature);
}
