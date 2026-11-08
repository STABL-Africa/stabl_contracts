# stabl_escrow reference client (TypeScript)

A light example of how an off-chain client works with the
[`stabl_escrow`](../../contracts/stabl_escrow) contract:

- compute `terms_hash` (format v1) from a quote,
- read an escrow and verify it against the quote you hold,
- claim it as the recipient,
- refund it after expiry (anyone may trigger this),
- lock funds as the sender.

The contract ID and RPC endpoint are read from
[`deployments/<network>.json`](../../deployments)

## How the escrow works

```
lock_for_offramp (sender signs)        -> Locked
release_escrow   (recipient signs)     Locked -> Claimed    while now <= expiry, pays the recipient
refund_escrow    (anyone)              Locked -> Refunded   once now > expiry, pays the sender
```

- An escrow is keyed by `(sender, transaction_id)`. `transaction_id` is a
  lowercase, hyphenated UUID string, passed as-is.
- The recipient is fixed by the sender's signature upon locking. Only that
  address can claim, and the payout is atomic: a successful
  `release_escrow` means the funds are already in the recipient's account.
- Every state change emits an `escrow_state_change` event with the recipient
  and sender as topics, and `transaction_id`, `new_state` and the params as
  data. Filter on the recipient topic to see every escrow addressed to you.
  Use events as a trigger and `get_escrow_entry` as the authoritative check:
  RPC only keeps events for a limited window.

## Setup

```sh
cd examples/escrow
npm install
```

Node 20+.

## Commands

```sh
# terms_hash for a quote
npx tsx escrow.ts hash --quote-id <uuid> --beneficiary-id <uuid> \
  --token <C...> --amount <base units> --expiry <unix seconds>

# Read an escrow; with --quote-id/--beneficiary-id it also recomputes
# terms_hash and compares it with what the sender signed
npx tsx escrow.ts check --sender <G...> --tx-id <uuid> \
  [--quote-id <uuid> --beneficiary-id <uuid>]

# Claim as the recipient
RECIPIENT_SECRET=S... npx tsx escrow.ts claim --sender <G...> --tx-id <uuid>

# Refund an expired escrow (any funded account can submit this)
SOURCE_SECRET=S... npx tsx escrow.ts refund --sender <G...> --tx-id <uuid>

# Lock as the sender
SENDER_SECRET=S... npx tsx escrow.ts lock --recipient <G...> --tx-id <uuid> \
  --token <C...> --amount <base units> --expiry <unix seconds> \
  --quote-id <uuid> --beneficiary-id <uuid>
```

Add `--network mainnet` to use the mainnet manifest once the contract is
deployed there.

A recipient should claim only after `check` shows `state: 'Locked'`,
`claimable: true`, the expected token and amount, and `terms_hash matches the quote`.
Execute the off-chain payout only after `claim` has confirmed.

## terms_hash format v1

The sender signs `terms_hash` as part of the lock, binding the escrow to one
specific quote. The contract stores the 32 bytes and never interprets them;
both off-chain parties compute it independently and compare.

```
sha256( 0x01                                   version
     || quote_id        36 bytes  lowercase UUID text
     || beneficiary_id  36 bytes  lowercase UUID text
     || token           56 bytes  C... strkey text
     || amount          16 bytes  i128 big-endian
     || expiry           8 bytes  u64 big-endian )
```

- Every field is fixed-length, so plain concatenation is unambiguous.
- `quote_id` must be random (UUIDv4 or v7) and MUST NOT be published: it
  salts the hash obfuscating the beneficiary_id for added security. 
- `amount` is in the token's base units (7 decimals for Stellar assets, so
  `5000000000` is 500.0000000). `expiry` is Unix seconds, the same value as
  the escrow's `expiry`.

### Test vector

Check your implementation against this before going further:

| Field | Value |
| --- | --- |
| quote_id | `3f2b8c1e-9d4a-4e6f-8b7c-2a1d0e9f8c7b` |
| beneficiary_id | `7c6d5e4f-3a2b-4c1d-9e8f-0a1b2c3d4e5f` |
| token | `CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC` |
| amount | `5000000000` |
| expiry | `1762646400` |
| **terms_hash** | **`c8ebe508e5e667204fce2330e7eedd993b7da31ec7dbed4cdf73d70daa6f1df0`** |

```sh
npx tsx escrow.ts hash --quote-id 3f2b8c1e-9d4a-4e6f-8b7c-2a1d0e9f8c7b \
  --beneficiary-id 7c6d5e4f-3a2b-4c1d-9e8f-0a1b2c3d4e5f \
  --token CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \
  --amount 5000000000 --expiry 1762646400
```

The same vector is pinned in the contract's Rust tests
(`terms_hash_v1_matches_published_test_vector`).

## Errors

Contract errors appear as `Error(Contract, #<code>)`:

| Code | Name | Meaning |
| --- | --- | --- |
| 1000 | NotFound | No escrow for this `(sender, transaction_id)` |
| 2000 | InvalidAmount | Lock amount not positive |
| 3000 | Exists | An escrow already exists for this `(sender, transaction_id)` |
| 4000 | InvalidTransactionId | Not a lowercase hyphenated UUID |
| 5000 | InvalidExpiry | Lock expiry already in the past |
| 5001 | Expired | Claim attempted after expiry |
| 5002 | NotExpired | Refund attempted at or before expiry |
| 6000 | InvalidState | Escrow already claimed or refunded |

## Notes

- The token is any Stellar Asset Contract (`C...` address of the asset, not
  the issuer's `G...`). A `G...` recipient of course will need a trustline for the token.
- This contract is unaudited at this time.
