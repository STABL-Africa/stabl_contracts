//! # Stabl Passkey Verifier Contract
//!
//! Stateless WebAuthn (passkey) signature verifier. Deployed once per network
//! and referenced by smart accounts as `Signer::External(verifier, key_data)`.
//!
//! The contract has no admin and no upgrade path on purpose: whoever could
//! upgrade it could change the authorization outcome of every account that
//! points at it. A fix ships as a new verifier at a new address, and each
//! account migrates its signers under its own authorization.
//!
//! `key_data` is the 65-byte uncompressed secp256r1 public key followed by
//! the variable-length credential ID. `sig_data` is XDR-encoded
//! `WebAuthnSigData`.

use soroban_sdk::{
    contract, contracterror, contractimpl, panic_with_error, xdr::FromXdr, Bytes, BytesN, Env, Vec,
};
use stellar_accounts::verifiers::{
    utils::extract_from_bytes,
    webauthn::{self, WebAuthnError, WebAuthnSigData},
    Verifier,
};

/// Errors raised by this contract before the signature reaches the WebAuthn
/// checks. Codes sit outside the OpenZeppelin `WebAuthnError`
/// range (3110-3119) so we never collide in diagnostics.
#[contracterror]
#[repr(u32)]
pub enum PasskeyVerifierError {
    /// `sig_data` is not a valid XDR-encoded `WebAuthnSigData`.
    SigDataDecodeFailed = 1,
}

#[contract]
pub struct StablPasskeyVerifierContract;

#[contractimpl]
impl Verifier for StablPasskeyVerifierContract {
    type KeyData = Bytes;
    type SigData = Bytes;

    /// Verify a WebAuthn assertion against `signature_payload`.
    ///
    /// `signature_payload` must equal the base64url-decoded `challenge` in the
    /// assertion's client data. When called by a smart account this is the
    /// account's auth digest, not the raw host payload.
    ///
    /// Returns `true` on success. Malformed input panics with a typed error;
    /// a well-formed but invalid signature panics inside the host's
    /// `secp256r1_verify`.
    fn verify(
        e: &Env,
        signature_payload: Bytes,
        key_data: Self::KeyData,
        sig_data: Self::SigData,
    ) -> bool {
        let sig_struct = WebAuthnSigData::from_xdr(e, &sig_data)
            .unwrap_or_else(|_| panic_with_error!(e, PasskeyVerifierError::SigDataDecodeFailed));

        let pub_key: BytesN<65> = extract_from_bytes(e, &key_data, 0..65)
            .unwrap_or_else(|| panic_with_error!(e, WebAuthnError::KeyDataInvalid));

        webauthn::verify(e, &signature_payload, &pub_key, &sig_struct)
    }

    /// The 65-byte public key with the credential ID suffix stripped. This is
    /// the identity smart accounts use to detect duplicate signers.
    fn canonicalize_key(e: &Env, key_data: Bytes) -> Bytes {
        webauthn::canonicalize_key(e, &key_data)
    }

    fn batch_canonicalize_key(e: &Env, keys_data: Vec<Bytes>) -> Vec<Bytes> {
        webauthn::batch_canonicalize_key(e, &keys_data)
    }
}
