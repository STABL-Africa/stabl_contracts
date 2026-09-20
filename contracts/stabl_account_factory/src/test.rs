#![cfg(test)]
extern crate std;

use crate::contract::{StablAccountFactory, StablAccountFactoryClient};
use p256::ecdsa::{
    signature::hazmat::PrehashSigner, Signature as P256Signature, SigningKey as P256SigningKey,
};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::SecretKey as P256SecretKey;
use soroban_sdk::auth::{Context, ContractContext};
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    map, symbol_short, vec, Address, Bytes, BytesN, Env, IntoVal, InvokeError, Map, Val, Vec,
};
use stabl_passkey_verifier::contract::StablPasskeyVerifierContract;
use stellar_accounts::smart_account::{AuthPayload, Signer, SmartAccountError};
use stellar_accounts::verifiers::utils::base64_url_encode;
use stellar_accounts::verifiers::webauthn::{
    WebAuthnSigData, AUTH_DATA_FLAGS_BE, AUTH_DATA_FLAGS_BS, AUTH_DATA_FLAGS_UP, AUTH_DATA_FLAGS_UV,
};

// The real account wasm, so the factory test exercises exactly what testnet
// runs. Requires `stellar contract build` first (see Makefile `test`).
mod account_wasm {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/stabl_multi_signer.wasm"
    );
}

struct Passkey {
    signing_key: P256SigningKey,
    pubkey: [u8; 65],
    cred_id: [u8; 16],
}

impl Passkey {
    fn new(seed: u8) -> Self {
        let secret = P256SecretKey::from_slice(&[seed; 32]).unwrap();
        let signing_key = P256SigningKey::from(&secret);
        let encoded = secret.public_key().to_encoded_point(false);
        let mut pubkey = [0u8; 65];
        pubkey.copy_from_slice(encoded.as_bytes());
        Self {
            signing_key,
            pubkey,
            cred_id: [seed; 16],
        }
    }

    fn signer(&self, e: &Env, verifier: &Address) -> Signer {
        let mut key_data = Bytes::from_array(e, &self.pubkey);
        key_data.extend_from_array(&self.cred_id);
        Signer::External(verifier.clone(), key_data)
    }

    fn assert(&self, e: &Env, challenge: &[u8; 32]) -> Bytes {
        let mut encoded = [0u8; 43];
        base64_url_encode(&mut encoded, challenge);
        let json = std::format!(
            r#"{{"type":"webauthn.get","challenge":"{}","origin":"https://app.stabl.africa","crossOrigin":false}}"#,
            std::str::from_utf8(&encoded).unwrap()
        );
        let client_data = Bytes::from_slice(e, json.as_bytes());
        let mut auth = [0u8; 37];
        auth[32] =
            AUTH_DATA_FLAGS_UP | AUTH_DATA_FLAGS_UV | AUTH_DATA_FLAGS_BE | AUTH_DATA_FLAGS_BS;
        let authenticator_data = Bytes::from_array(e, &auth);
        let mut msg = authenticator_data.clone();
        msg.extend_from_array(&e.crypto().sha256(&client_data).to_array());
        let digest = e.crypto().sha256(&msg).to_array();
        let sig: P256Signature = self.signing_key.sign_prehash(&digest).unwrap();
        let sig = sig.normalize_s().unwrap_or(sig).to_bytes();
        let mut raw = [0u8; 64];
        raw.copy_from_slice(&sig);
        WebAuthnSigData {
            signature: BytesN::from_array(e, &raw),
            authenticator_data,
            client_data,
        }
        .to_xdr(e)
    }
}

struct Fixture {
    e: Env,
    factory: StablAccountFactoryClient<'static>,
    verifier: Address,
    wasm_hash: BytesN<32>,
}

fn setup() -> Fixture {
    let e = Env::default();
    // Deploying through the factory costs more budget than a native call;
    // lift the limits so the test measures behaviour, not fees.
    e.cost_estimate().budget().reset_unlimited();
    let wasm_hash = e.deployer().upload_contract_wasm(account_wasm::WASM);
    let verifier = e.register(StablPasskeyVerifierContract, ());
    let factory_id = e.register(StablAccountFactory, (wasm_hash.clone(),));
    // Leak the Env clone so the client can borrow it for 'static; test-only.
    let e_ref: &'static Env = std::boxed::Box::leak(std::boxed::Box::new(e.clone()));
    let factory = StablAccountFactoryClient::new(e_ref, &factory_id);
    Fixture {
        e,
        factory,
        verifier,
        wasm_hash,
    }
}

fn no_policies(e: &Env) -> Map<Address, Val> {
    Map::new(e)
}

#[test]
fn stores_wasm_hash() {
    let f = setup();
    assert_eq!(f.factory.wasm_hash(), f.wasm_hash);
}

#[test]
fn predict_matches_deploy_and_emits_event() {
    let f = setup();
    let e = &f.e;
    let salt = BytesN::from_array(e, &[1u8; 32]);
    let predicted = f.factory.predict(&salt);

    let passkey = Passkey::new(5);
    let account = f.factory.deploy(
        &salt,
        &vec![e, passkey.signer(e, &f.verifier)],
        &no_policies(e),
    );
    assert_eq!(account, predicted);

    let from_factory = e.events().all().filter_by_contract(&f.factory.address);
    assert_eq!(from_factory.events().len(), 1);
}

#[test]
fn deployed_account_has_the_signer() {
    let f = setup();
    let e = &f.e;
    let passkey = Passkey::new(6);
    let account = f.factory.deploy(
        &BytesN::from_array(e, &[2u8; 32]),
        &vec![e, passkey.signer(e, &f.verifier)],
        &no_policies(e),
    );
    let client = account_wasm::Client::new(e, &account);
    assert_eq!(client.get_context_rules_count(), 1);
    let rule = client.get_context_rule(&0);
    assert_eq!(rule.signer_ids.len(), 1);
}

#[test]
fn same_salt_twice_fails() {
    let f = setup();
    let e = &f.e;
    let salt = BytesN::from_array(e, &[3u8; 32]);
    let signers = vec![e, Passkey::new(7).signer(e, &f.verifier)];
    f.factory.deploy(&salt, &signers, &no_policies(e));
    assert!(f
        .factory
        .try_deploy(&salt, &signers, &no_policies(e))
        .is_err());
}

#[test]
fn different_salts_give_different_accounts() {
    let f = setup();
    let e = &f.e;
    let signers = vec![e, Passkey::new(8).signer(e, &f.verifier)];
    let a = f.factory.deploy(
        &BytesN::from_array(e, &[4u8; 32]),
        &signers,
        &no_policies(e),
    );
    let b = f.factory.deploy(
        &BytesN::from_array(e, &[5u8; 32]),
        &signers,
        &no_policies(e),
    );
    assert_ne!(a, b);
}

#[test]
fn passkey_authorizes_on_factory_deployed_account() {
    // Full path on the real wasm: factory -> account constructor -> verifier
    // canonicalize -> __check_auth -> verifier verify.
    let f = setup();
    let e = &f.e;
    let passkey = Passkey::new(9);
    let signer = passkey.signer(e, &f.verifier);
    let account = f.factory.deploy(
        &BytesN::from_array(e, &[6u8; 32]),
        &vec![e, signer.clone()],
        &no_policies(e),
    );

    let payload = [0x55u8; 32];
    let rule_ids: Vec<u32> = vec![e, 0];
    let mut preimage = Bytes::from_array(e, &payload);
    preimage.append(&rule_ids.clone().to_xdr(e));
    let challenge = e.crypto().sha256(&preimage).to_array();

    let auth_payload = AuthPayload {
        signers: map![e, (signer, passkey.assert(e, &challenge))],
        context_rule_ids: rule_ids,
    };
    let contexts: Vec<Context> = vec![
        e,
        Context::Contract(ContractContext {
            contract: Address::generate(e),
            fn_name: symbol_short!("transfer"),
            args: vec![e],
        }),
    ];

    let result: Result<(), Result<SmartAccountError, InvokeError>> = e
        .try_invoke_contract_check_auth::<SmartAccountError>(
            &account,
            &BytesN::from_array(e, &payload),
            auth_payload.into_val(e),
            &contexts,
        );
    assert!(result.is_ok());
}
