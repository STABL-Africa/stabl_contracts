# Testnet end-to-end harness

A static page that authorises a call on a deployed `stabl_passkey_multi_signer`
account with a real browser passkey. Everything an off-chain client has to do
happens in the page, so `e2e.js` is also the reference for server-side
implementations in any language.

```sh
make e2e          # serves this directory on http://localhost:4100
```

## Prerequisites

1. A passkey registered in your browser for `rpId` `localhost` (any localhost
   origin will do; a passkey created on `http://localhost:<port>` has `rpId`
   `localhost` and works from any other localhost port).
2. Its 65-byte uncompressed P-256 public key and credential ID. Concatenated,
   these are the `key_data` bytes.
3. An account deployed through `stabl_account_factory` with that passkey as an
   `External(verifier, key_data)` signer, for example:

   ```sh
   stellar contract invoke --id <factory> --source deployer --network testnet -- \
     deploy --salt <32-byte hex> \
     --signers '[{"External": ["<verifier>", "<key_data hex>"]}]' \
     --policies '{}'
   ```

4. A funded testnet key to pay fees (`stellar keys show deployer`).

## Running

Fill in the fields, run `get_context_rules_count` first to confirm the SDK
loads and RPC is reachable, then run `add_signer`. The page:

1. builds and simulates the call to get the auth entry the account must sign,
2. computes the host `signature_payload`, then the smart account's
   `auth_digest = sha256(signature_payload || xdr(context_rule_ids))`,
3. hands `auth_digest` to `navigator.credentials.get` as the challenge,
4. converts the DER signature to low-s `r||s`, packs `WebAuthnSigData` and
   `AuthPayload`, sets them on the auth entry,
5. re-simulates, assembles, signs with the fee payer, submits.

Every intermediate value is logged in hex so a server implementation can be
checked step by step. Confirm the result with:

```sh
stellar contract invoke --id <account> --source deployer --network testnet --send=no \
  -- get_context_rule --context_rule_id 0
```

The rule should now list two signers.

## Failure hints

- `Error(Contract, #3114)`: challenge mismatch. The digest given to the
  authenticator was not `sha256(signature_payload || xdr(context_rule_ids))`.
- `ExternalVerificationFailed`: the verifier rejected the signature. Usually a
  high-s signature or wrong `key_data`.
- Module load failure: the page pulls `@stellar/stellar-sdk` from esm.sh. If
  that is blocked, switch to the jsdelivr UMD bundle, which exposes
  `window.StellarSdk`.
