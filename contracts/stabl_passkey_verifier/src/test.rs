extern crate std;

use hex_literal::hex;
use soroban_sdk::{xdr::ToXdr, Bytes, BytesN, Env};
use stellar_accounts::verifiers::{
    webauthn::{
        WebAuthnSigData
    },
};

use crate::contract::{StablPasskeyVerifierContract, StablPasskeyVerifierContractClient};

  const PAYLOAD: [u8; 32] = hex!("020ae3e7ae23efccb9e1f6bb1a647e132b52b5c5e7b2fd4a4fa55eeaa26caaac");
  const AUTH_DATA: [u8; 37] = hex!("49960de5880e8c687434170f6476605b8fe4aeb9a28632c7995cf3ba831d97631d00000000");
  const SIG: [u8; 64] = hex!("2d69c0bdb13c1a5ccc8ab2609b39d9991d5cf9cb62e9499fa5e9dfd25ba720ec366683fc3a002adacafbd230d51193314dbd3455a60346e1c441eda4ad5ad19a");
  const PUBKEY: [u8; 65] = hex!("04f8ab6b27338793c7ecd7871709e05a8f0372e0b6aced71d5c61d0de2ce50535a313b865a3f961c9eeb720b20e322a97fef20ee8be763f3b927e8776574887634");
  const CRED_ID: [u8; 16] = hex!("01b2994876ec9ddb1c1d744a21914f8b");
  const CLIENT_DATA: &[u8] = br#"{"type":"webauthn.get","challenge":"Agrj564j78y54fa7GmR-EytStcXnsv1KT6Ve6qJsqqw","origin":"http://localhost:4000","crossOrigin":false}"#;

  fn fixture(e: &Env) -> (Bytes, Bytes, Bytes) {
      let mut key_data = Bytes::from_array(e, &PUBKEY);
      key_data.extend_from_array(&CRED_ID);
      let sig_data = WebAuthnSigData {
          client_data: Bytes::from_slice(e, CLIENT_DATA),
          authenticator_data: Bytes::from_array(e, &AUTH_DATA),
          signature: BytesN::from_array(e, &SIG),
      }
      .to_xdr(e);
      (Bytes::from_array(e, &PAYLOAD), key_data, sig_data)
  }

  #[test]
  fn verifies_real_passkey_assertion() {
      let e = Env::default();
      let client = StablPasskeyVerifierContractClient::new(&e, &e.register(StablPasskeyVerifierContract, ()));
      let (payload, key_data, sig_data) = fixture(&e);
      assert!(client.verify(&payload, &key_data, &sig_data));
  }

  #[test]
  fn canonicalize_strips_credential_id() {
      let e = Env::default();
      let client = StablPasskeyVerifierContractClient::new(&e, &e.register(StablPasskeyVerifierContract, ()));
      let (_, key_data, _) = fixture(&e);
      assert_eq!(client.canonicalize_key(&key_data), Bytes::from_array(&e, &PUBKEY));
  }

  #[test]
  #[should_panic]
  fn rejects_tampered_signature() {
      let e = Env::default();
      let client = StablPasskeyVerifierContractClient::new(&e, &e.register(StablPasskeyVerifierContract, ()));
      let (payload, key_data, _) = fixture(&e);
      let mut sig = SIG;
      sig[0] ^= 1;
      let sig_data = WebAuthnSigData {
          client_data: Bytes::from_slice(&e, CLIENT_DATA),
          authenticator_data: Bytes::from_array(&e, &AUTH_DATA),
          signature: BytesN::from_array(&e, &sig),
      }
      .to_xdr(&e);
      client.verify(&payload, &key_data, &sig_data);
  }