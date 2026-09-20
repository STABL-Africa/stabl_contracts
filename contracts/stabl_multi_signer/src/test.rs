#![cfg(test)]
extern crate std;

use crate::contract::{StablPasskeyMultiSigner, StablPasskeyMultiSignerClient};
use ed25519_dalek::SigningKey;
use p256::ecdsa::{
    signature::hazmat::PrehashSigner, Signature as P256Signature, SigningKey as P256SigningKey,
};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::SecretKey as P256SecretKey;
use soroban_sdk::auth::{Context, ContractContext};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::xdr::{AccountId, PublicKey, ScAddress, ToXdr, Uint256};
use soroban_sdk::{
    map, symbol_short, vec, Address, Bytes, BytesN, Env, IntoVal, InvokeError, Map, TryFromVal,
    Val, Vec,
};
use stabl_passkey_verifier::contract::StablPasskeyVerifierContract;
use stellar_accounts::smart_account::{AuthPayload, Signer, SmartAccountError};
use stellar_accounts::verifiers::utils::base64_url_encode;
use stellar_accounts::verifiers::webauthn::{
    WebAuthnSigData, AUTH_DATA_FLAGS_BE, AUTH_DATA_FLAGS_BS, AUTH_DATA_FLAGS_UP, AUTH_DATA_FLAGS_UV,
};

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

// ---------------------------------------------------------------------------
// Passkey (WebAuthn) signer through the deployed verifier contract.
//
// A synthetic P-256 key stands in for the authenticator so the test can sign
// whatever digest the account demands. Real assertions from a browser are
// covered in the verifier crate's own tests.
// ---------------------------------------------------------------------------

/// A fake passkey: P-256 key pair plus a credential ID.
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

    /// `key_data` exactly as a server stores and registers it on-chain:
    /// 65-byte uncompressed public key followed by the credential ID.
    fn key_data(&self, e: &Env) -> Bytes {
        let mut b = Bytes::from_array(e, &self.pubkey);
        b.extend_from_array(&self.cred_id);
        b
    }

    fn signer(&self, e: &Env, verifier: &Address) -> Signer {
        Signer::External(verifier.clone(), self.key_data(e))
    }

    /// Produce a WebAuthn assertion whose `challenge` is `challenge`, the way a
    /// browser would when `navigator.credentials.get` is handed those bytes.
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

/// What signers actually sign: `sha256(signature_payload || xdr(context_rule_ids))`.
/// A server must send this, not the raw payload, as the WebAuthn challenge.
fn auth_digest(e: &Env, payload: &[u8; 32], rule_ids: &Vec<u32>) -> [u8; 32] {
    let mut preimage = Bytes::from_array(e, payload);
    preimage.append(&rule_ids.clone().to_xdr(e));
    e.crypto().sha256(&preimage).to_array()
}

fn some_context(e: &Env) -> Vec<Context> {
    vec![
        e,
        Context::Contract(ContractContext {
            contract: Address::generate(e),
            fn_name: symbol_short!("transfer"),
            args: vec![e],
        }),
    ]
}

/// Runs `__check_auth` the way the host would. Any failure, whether a typed
/// `SmartAccountError`, a `WebAuthnError` surfacing from the verifier, or a
/// host crypto error, is an authorization failure, so callers only need
/// `is_ok` / `is_err`.
fn check_auth(
    e: &Env,
    account: &Address,
    payload: &[u8; 32],
    auth_payload: &AuthPayload,
    contexts: &Vec<Context>,
) -> Result<(), Result<SmartAccountError, InvokeError>> {
    e.try_invoke_contract_check_auth::<SmartAccountError>(
        account,
        &BytesN::from_array(e, payload),
        auth_payload.into_val(e),
        contexts,
    )
}

#[test]
fn single_passkey_signer_authorizes_through_verifier() {
    let e = Env::default();
    let verifier = e.register(StablPasskeyVerifierContract, ());
    let passkey = Passkey::new(11);
    let signer = passkey.signer(&e, &verifier);

    // No policies: every signer in the rule must sign.
    let account = e.register(
        StablPasskeyMultiSigner,
        (vec![&e, signer.clone()], Map::<Address, Val>::new(&e)),
    );

    let payload = [0x42u8; 32];
    let contexts = some_context(&e);
    let rule_ids: Vec<u32> = vec![&e, 0];
    let challenge = auth_digest(&e, &payload, &rule_ids);

    let auth_payload = AuthPayload {
        signers: map![&e, (signer, passkey.assert(&e, &challenge))],
        context_rule_ids: rule_ids,
    };

    assert_eq!(
        check_auth(&e, &account, &payload, &auth_payload, &contexts),
        Ok(())
    );
}

#[test]
fn signing_raw_payload_instead_of_auth_digest_fails() {
    let e = Env::default();
    let verifier = e.register(StablPasskeyVerifierContract, ());
    let passkey = Passkey::new(12);
    let signer = passkey.signer(&e, &verifier);
    let account = e.register(
        StablPasskeyMultiSigner,
        (vec![&e, signer.clone()], Map::<Address, Val>::new(&e)),
    );

    let payload = [0x42u8; 32];
    let contexts = some_context(&e);

    // Wrong: challenge is the host payload, not sha256(payload || rule_ids).
    let auth_payload = AuthPayload {
        signers: map![&e, (signer, passkey.assert(&e, &payload))],
        context_rule_ids: vec![&e, 0],
    };

    assert!(check_auth(&e, &account, &payload, &auth_payload, &contexts).is_err());
}

#[test]
fn two_passkeys_no_policy_requires_both() {
    let e = Env::default();
    let verifier = e.register(StablPasskeyVerifierContract, ());
    let alice = Passkey::new(21);
    let bob = Passkey::new(22);
    let account = e.register(
        StablPasskeyMultiSigner,
        (
            vec![&e, alice.signer(&e, &verifier), bob.signer(&e, &verifier)],
            Map::<Address, Val>::new(&e),
        ),
    );

    let payload = [0x07u8; 32];
    let contexts = some_context(&e);
    let rule_ids: Vec<u32> = vec![&e, 0];
    let challenge = auth_digest(&e, &payload, &rule_ids);

    let only_alice = AuthPayload {
        signers: map![
            &e,
            (alice.signer(&e, &verifier), alice.assert(&e, &challenge))
        ],
        context_rule_ids: rule_ids.clone(),
    };
    assert!(check_auth(&e, &account, &payload, &only_alice, &contexts).is_err());

    let both = AuthPayload {
        signers: map![
            &e,
            (alice.signer(&e, &verifier), alice.assert(&e, &challenge)),
            (bob.signer(&e, &verifier), bob.assert(&e, &challenge)),
        ],
        context_rule_ids: rule_ids,
    };
    assert!(check_auth(&e, &account, &payload, &both, &contexts).is_ok());
}

#[test]
fn unregistered_passkey_is_rejected() {
    let e = Env::default();
    let verifier = e.register(StablPasskeyVerifierContract, ());
    let alice = Passkey::new(31);
    let mallory = Passkey::new(32);
    let account = e.register(
        StablPasskeyMultiSigner,
        (
            vec![&e, alice.signer(&e, &verifier)],
            Map::<Address, Val>::new(&e),
        ),
    );

    let payload = [0x09u8; 32];
    let contexts = some_context(&e);
    let rule_ids: Vec<u32> = vec![&e, 0];
    let challenge = auth_digest(&e, &payload, &rule_ids);

    let auth_payload = AuthPayload {
        signers: map![
            &e,
            (
                mallory.signer(&e, &verifier),
                mallory.assert(&e, &challenge)
            )
        ],
        context_rule_ids: rule_ids,
    };
    assert!(check_auth(&e, &account, &payload, &auth_payload, &contexts).is_err());
}

#[test]
#[should_panic]
fn duplicate_passkey_with_different_credential_id_is_rejected_at_construction() {
    let e = Env::default();
    let verifier = e.register(StablPasskeyVerifierContract, ());
    let alice = Passkey::new(41);
    let mut same_key_other_cred = Passkey::new(41);
    same_key_other_cred.cred_id = [0xffu8; 16];

    // Canonical identity is the public key alone; the verifier's
    // batch_canonicalize_key lets the account spot the duplicate.
    e.register(
        StablPasskeyMultiSigner,
        (
            vec![
                &e,
                alice.signer(&e, &verifier),
                same_key_other_cred.signer(&e, &verifier),
            ],
            Map::<Address, Val>::new(&e),
        ),
    );
}
