#![cfg(test)]
extern crate std;

use hex_literal::hex;
use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Address, Bytes, BytesN, Env};
use stellar_accounts::verifiers::webauthn::WebAuthnSigData;

use crate::contract::{StablPasskeyVerifierContract, StablPasskeyVerifierContractClient};

// Fixture captured from a throwaway passkey registered against
// http://localhost:4000 during development. Everything here is public data
// (public key, credential ID, signed assertion); the passkey is never used
// for anything else.
const PAYLOAD: [u8; 32] = hex!("020ae3e7ae23efccb9e1f6bb1a647e132b52b5c5e7b2fd4a4fa55eeaa26caaac");
const AUTH_DATA: [u8; 37] =
    hex!("49960de5880e8c687434170f6476605b8fe4aeb9a28632c7995cf3ba831d97631d00000000");
const SIG: [u8; 64] = hex!(
    "2d69c0bdb13c1a5ccc8ab2609b39d9991d5cf9cb62e9499fa5e9dfd25ba720ec"
    "366683fc3a002adacafbd230d51193314dbd3455a60346e1c441eda4ad5ad19a"
);
const PUBKEY: [u8; 65] = hex!(
    "04f8ab6b27338793c7ecd7871709e05a8f0372e0b6aced71d5c61d0de2ce50535a"
    "313b865a3f961c9eeb720b20e322a97fef20ee8be763f3b927e8776574887634"
);
const CRED_ID: [u8; 16] = hex!("01b2994876ec9ddb1c1d744a21914f8b");
const CLIENT_DATA: &[u8] = br#"{"type":"webauthn.get","challenge":"Agrj564j78y54fa7GmR-EytStcXnsv1KT6Ve6qJsqqw","origin":"http://localhost:4000","crossOrigin":false}"#;

fn client(e: &Env) -> StablPasskeyVerifierContractClient<'_> {
    StablPasskeyVerifierContractClient::new(e, &e.register(StablPasskeyVerifierContract, ()))
}

fn key_data(e: &Env) -> Bytes {
    let mut key_data = Bytes::from_array(e, &PUBKEY);
    key_data.extend_from_array(&CRED_ID);
    key_data
}

fn sig_data(e: &Env, sig: &[u8; 64]) -> Bytes {
    WebAuthnSigData {
        client_data: Bytes::from_slice(e, CLIENT_DATA),
        authenticator_data: Bytes::from_array(e, &AUTH_DATA),
        signature: BytesN::from_array(e, sig),
    }
    .to_xdr(e)
}

#[test]
fn verifies_real_passkey_assertion() {
    let e = Env::default();
    let payload = Bytes::from_array(&e, &PAYLOAD);
    assert!(client(&e).verify(&payload, &key_data(&e), &sig_data(&e, &SIG)));
}

#[test]
fn canonicalize_strips_credential_id() {
    let e = Env::default();
    assert_eq!(
        client(&e).canonicalize_key(&key_data(&e)),
        Bytes::from_array(&e, &PUBKEY)
    );
}

#[test]
fn batch_canonicalize_strips_credential_ids() {
    let e = Env::default();
    let keys = soroban_sdk::vec![&e, key_data(&e), Bytes::from_array(&e, &PUBKEY)];
    let canonical = client(&e).batch_canonicalize_key(&keys);
    assert_eq!(canonical.len(), 2);
    assert_eq!(canonical.get_unchecked(0), Bytes::from_array(&e, &PUBKEY));
    assert_eq!(canonical.get_unchecked(1), Bytes::from_array(&e, &PUBKEY));
}

#[test]
#[should_panic]
fn rejects_tampered_signature() {
    let e = Env::default();
    let mut sig = SIG;
    sig[0] ^= 1;
    let payload = Bytes::from_array(&e, &PAYLOAD);
    client(&e).verify(&payload, &key_data(&e), &sig_data(&e, &sig));
}

#[test]
#[should_panic]
fn rejects_wrong_payload() {
    let e = Env::default();
    let mut payload = PAYLOAD;
    payload[0] ^= 1;
    let payload = Bytes::from_array(&e, &payload);
    client(&e).verify(&payload, &key_data(&e), &sig_data(&e, &SIG));
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn rejects_sig_data_of_wrong_type() {
    // Valid XDR, but an ScVal that is not a WebAuthnSigData. The host decodes
    // it fine; our conversion rejects it with the typed error.
    let e = Env::default();
    let payload = Bytes::from_array(&e, &PAYLOAD);
    let wrong_type = Bytes::from_array(&e, &[1, 2, 3]).to_xdr(&e);
    client(&e).verify(&payload, &key_data(&e), &wrong_type);
}

#[test]
#[should_panic(expected = "Error(Value, InvalidInput)")]
fn rejects_garbage_sig_data() {
    // Bytes that are not XDR at all never reach the contract's error path:
    // the host's deserializer traps first.
    let e = Env::default();
    let payload = Bytes::from_array(&e, &PAYLOAD);
    let garbage = Bytes::from_array(&e, &[0xde, 0xad, 0xbe, 0xef]);
    client(&e).verify(&payload, &key_data(&e), &garbage);
}

#[test]
#[should_panic(expected = "Error(Contract, #3119)")]
fn rejects_short_key_data() {
    let e = Env::default();
    let payload = Bytes::from_array(&e, &PAYLOAD);
    let short_key = Bytes::from_slice(&e, &PUBKEY[..64]);
    client(&e).verify(&payload, &short_key, &sig_data(&e, &SIG));
}

#[test]
fn contract_holds_no_state() {
    // Deploying twice yields independent, equally valid verifiers: nothing
    // is stored, so there is nothing for an operator to change later.
    let e = Env::default();
    let a = e.register(StablPasskeyVerifierContract, ());
    let b = e.register(StablPasskeyVerifierContract, ());
    assert_ne!(a, b);
    assert_ne!(a, Address::generate(&e));
    let payload = Bytes::from_array(&e, &PAYLOAD);
    for id in [a, b] {
        let c = StablPasskeyVerifierContractClient::new(&e, &id);
        assert!(c.verify(&payload, &key_data(&e), &sig_data(&e, &SIG)));
    }
}

#[test]
#[ignore]
fn dump_fixture_hex_for_cli() {
    let e = Env::default();
    std::println!("payload={}", hex_of(&PAYLOAD));
    std::println!("key_data={}", hex_bytes(&key_data(&e)));
    std::println!("sig_data={}", hex_bytes(&sig_data(&e, &SIG)));
}

fn hex_of(b: &[u8]) -> std::string::String {
    b.iter().map(|x| std::format!("{:02x}", x)).collect()
}

fn hex_bytes(b: &Bytes) -> std::string::String {
    let mut v = std::vec![0u8; b.len() as usize];
    b.copy_into_slice(&mut v);
    hex_of(&v)
}
