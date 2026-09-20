//! # Stabl P-256 Verifier Contract
//!
//! Stateless raw secp256r1 (P-256) ECDSA signature verifier. Deployed once per
//! network and referenced by smart accounts as
//! `Signer::External(verifier, public_key)`.
//!
//! Intended for device-bound keys held in phone hardware (Android StrongBox,
//! Apple Secure Enclave), which only offer P-256. Unlike the passkey verifier
//! there is no WebAuthn ceremony: no client data, no authenticator data, no
//! relying-party binding. The device signs the account's auth digest directly.
//!
//! Like the passkey verifier this contract has no admin and no upgrade path.
//! A fix ships as a new verifier at a new address.
//!
//! `key_data` is the 65-byte uncompressed SEC1 public key (`0x04 || x || y`).
//! `sig_data` is the 64-byte `r || s` signature with `s` in low form; the host
//! rejects high-`s` signatures.

use soroban_sdk::{contract, contractimpl, Bytes, BytesN, Env, Vec};
use stellar_accounts::verifiers::Verifier;

#[contract]
pub struct StablP256VerifierContract;

#[contractimpl]
impl Verifier for StablP256VerifierContract {
    type KeyData = BytesN<65>;
    type SigData = BytesN<64>;

    /// Verify an ECDSA P-256 signature over `sha256(signature_payload)`.
    ///
    /// When called by a smart account `signature_payload` is the account's
    /// 32-byte auth digest. Hashing it again here means the device can use
    /// its platform's ordinary "sign message with SHA-256" API rather than a
    /// raw-digest signing mode.
    ///
    /// Returns `true` on success. An invalid signature, a malformed key or a
    /// high-`s` signature panics inside the host's `secp256r1_verify`.
    fn verify(
        e: &Env,
        signature_payload: Bytes,
        key_data: Self::KeyData,
        sig_data: Self::SigData,
    ) -> bool {
        e.crypto()
            .secp256r1_verify(&key_data, &e.crypto().sha256(&signature_payload), &sig_data);
        true
    }

    /// The uncompressed public key is already canonical: one encoding per key.
    fn canonicalize_key(e: &Env, key_data: Self::KeyData) -> Bytes {
        Bytes::from_slice(e, &key_data.to_array())
    }

    fn batch_canonicalize_key(e: &Env, keys_data: Vec<Self::KeyData>) -> Vec<Bytes> {
        Vec::from_iter(
            e,
            keys_data.iter().map(|key| Self::canonicalize_key(e, key)),
        )
    }
}
