#![cfg(test)]
extern crate std;

use hex_literal::hex;
use p256::ecdsa::{signature::hazmat::PrehashSigner, Signature, SigningKey};
use sha2::{Digest, Sha256};
use soroban_sdk::{Bytes, BytesN, Env};

use crate::contract::{StablP256VerifierContract, StablP256VerifierContractClient};

// Any 32-byte auth digest; the value is irrelevant to the verifier.
const PAYLOAD: [u8; 32] = hex!("020ae3e7ae23efccb9e1f6bb1a647e132b52b5c5e7b2fd4a4fa55eeaa26caaac");

// Fixed test scalar so the fixture is deterministic. Never used for anything
// but this test.
const SECRET: [u8; 32] = hex!("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721");

fn client(e: &Env) -> StablP256VerifierContractClient<'_> {
    StablP256VerifierContractClient::new(e, &e.register(StablP256VerifierContract, ()))
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&SECRET.into()).unwrap()
}

/// 65-byte uncompressed SEC1 public key, the `key_data` layout.
fn pubkey(e: &Env) -> BytesN<65> {
    let point = signing_key().verifying_key().to_encoded_point(false);
    BytesN::from_array(e, point.as_bytes().try_into().unwrap())
}

/// Sign `sha256(payload)` and return `r || s` with `s` in low form, exactly
/// what a device must submit.
fn sign(e: &Env, payload: &[u8]) -> BytesN<64> {
    let digest = Sha256::digest(payload);
    let sig: Signature = signing_key().sign_prehash(&digest).unwrap();
    let sig = sig.normalize_s().unwrap_or(sig);
    BytesN::from_array(e, &sig.to_bytes().into())
}

#[test]
fn verifies_valid_signature() {
    let e = Env::default();
    let payload = Bytes::from_array(&e, &PAYLOAD);
    assert!(client(&e).verify(&payload, &pubkey(&e), &sign(&e, &PAYLOAD)));
}

#[test]
fn rejects_signature_over_different_payload() {
    let e = Env::default();
    let mut other = PAYLOAD;
    other[0] ^= 0x01;
    let payload = Bytes::from_array(&e, &PAYLOAD);
    // Host secp256r1_verify failure surfaces as an Err from try_verify, not
    // a contract error code.
    assert!(client(&e)
        .try_verify(&payload, &pubkey(&e), &sign(&e, &other))
        .is_err());
}

#[test]
fn rejects_high_s_signature() {
    let e = Env::default();
    let payload = Bytes::from_array(&e, &PAYLOAD);
    let low = sign(&e, &PAYLOAD).to_array();
    // Flip s to n - s: same curve point, still mathematically valid, but the
    // host insists on the low form so clients must normalise.
    let sig = Signature::from_slice(&low).unwrap();
    let (r, s) = sig.split_scalars();
    let high = Signature::from_scalars(*r, -*s).unwrap();
    assert!(
        high.normalize_s().is_some(),
        "fixture must actually be high-s"
    );
    let high = BytesN::from_array(&e, &high.to_bytes().into());
    assert!(client(&e).try_verify(&payload, &pubkey(&e), &high).is_err());
}

#[test]
fn rejects_wrong_key() {
    let e = Env::default();
    let payload = Bytes::from_array(&e, &PAYLOAD);
    let other = SigningKey::from_bytes(&[7u8; 32].into()).unwrap();
    let point = other.verifying_key().to_encoded_point(false);
    let other_pub: BytesN<65> = BytesN::from_array(&e, point.as_bytes().try_into().unwrap());
    assert!(client(&e)
        .try_verify(&payload, &other_pub, &sign(&e, &PAYLOAD))
        .is_err());
}

#[test]
fn canonical_key_is_the_key_itself() {
    let e = Env::default();
    let key = pubkey(&e);
    let canon = client(&e).canonicalize_key(&key);
    assert_eq!(canon, Bytes::from_array(&e, &key.to_array()));
}
