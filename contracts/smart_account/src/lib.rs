//! Smart account: a Soroban custom account contract.
//!
//! The contract address itself becomes a Stellar "account" whose
//! authorization is decided by `__check_auth` instead of a classic keypair.
//! This starter verifies ed25519 signatures against a stored signer set
//! (any one known signer authorizes). Extend `__check_auth` with
//! `env.crypto().secp256r1_verify` to support WebAuthn/passkey signers.
#![no_std]
use soroban_sdk::{
    auth::{Context, CustomAccountInterface},
    contract, contracterror, contractimpl, contracttype,
    crypto::Hash,
    symbol_short, BytesN, Env, Vec,
};

#[contract]
pub struct SmartAccount;

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    NoSignature = 2,
    UnknownSigner = 3,
    NoSigners = 4,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Signers,
}

/// One ed25519 signature over the signature payload, attributed to a signer.
#[contracttype]
#[derive(Clone)]
pub struct Ed25519Signature {
    pub public_key: BytesN<32>,
    pub signature: BytesN<64>,
}

#[contractimpl]
impl SmartAccount {
    /// Runs once at deploy time; `signers` are raw ed25519 public keys.
    pub fn __constructor(env: Env, signers: Vec<BytesN<32>>) -> Result<(), Error> {
        if signers.is_empty() {
            return Err(Error::NoSigners);
        }
        env.storage().instance().set(&DataKey::Signers, &signers);
        env.events().publish(
            (symbol_short!("signers"), symbol_short!("created")),
            signers,
        );
        Ok(())
    }

    pub fn signers(env: Env) -> Result<Vec<BytesN<32>>, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Signers)
            .ok_or(Error::NotInitialized)
    }

    /// Rotate the signer set. Authorized by the account itself, so the call
    /// must carry signatures that pass `__check_auth`.
    pub fn set_signers(env: Env, signers: Vec<BytesN<32>>) -> Result<(), Error> {
        if signers.is_empty() {
            return Err(Error::NoSigners);
        }
        env.current_contract_address().require_auth();
        env.storage().instance().set(&DataKey::Signers, &signers);
        env.events().publish(
            (symbol_short!("signers"), symbol_short!("rotated")),
            signers,
        );
        Ok(())
    }
}

#[contractimpl]
impl CustomAccountInterface for SmartAccount {
    type Signature = Vec<Ed25519Signature>;
    type Error = Error;

    fn __check_auth(
        env: Env,
        signature_payload: Hash<32>,
        signatures: Vec<Ed25519Signature>,
        _auth_contexts: Vec<Context>,
    ) -> Result<(), Error> {
        let signers: Vec<BytesN<32>> = env
            .storage()
            .instance()
            .get(&DataKey::Signers)
            .ok_or(Error::NotInitialized)?;

        if signatures.is_empty() {
            return Err(Error::NoSignature);
        }
        for sig in signatures.iter() {
            if !signers.contains(&sig.public_key) {
                return Err(Error::UnknownSigner);
            }
            // Panics (and thus fails auth) on an invalid signature.
            env.crypto().ed25519_verify(
                &sig.public_key,
                &signature_payload.clone().into(),
                &sig.signature,
            );
        }
        Ok(())
    }
}

mod test;
