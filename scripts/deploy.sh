#!/usr/bin/env bash
# Build all contracts and deploy them to a Stellar network, recording the
# resulting contract IDs in deployments/<network>.json: the manifest that
# off-chain clients read to find the current contract addresses.
#
# Usage: scripts/deploy.sh <network> [identity] [name...]
#   network   testnet | mainnet
#   identity  Stellar CLI key alias to deploy from
#             (default: deployer on testnet, deployer-mainnet on mainnet)
#   name...   optional subset of contracts to (re)deploy; default is all.
#             Existing manifest entries for other contracts are left alone.
#
# Mainnet asks for confirmation unless CONFIRM=1 is set. There is no friendbot:
# the identity must already exist and hold enough XLM for the uploads.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

NETWORK="${1:?usage: scripts/deploy.sh <testnet|mainnet> [identity] [name...]}"
shift
case "$NETWORK" in
  testnet)
    PASSPHRASE="Test SDF Network ; September 2015"
    RPC_URL="https://soroban-testnet.stellar.org"
    HORIZON_URL="https://horizon-testnet.stellar.org"
    DEFAULT_IDENTITY=deployer
    ;;
  mainnet)
    PASSPHRASE="Public Global Stellar Network ; September 2015"
    RPC_URL="https://mainnet.sorobanrpc.com"
    HORIZON_URL="https://horizon.stellar.org"
    DEFAULT_IDENTITY=deployer-mainnet
    ;;
  *)
    echo "unknown network '$NETWORK' (expected testnet or mainnet)" >&2
    exit 1
    ;;
esac

IDENTITY="${1:-$DEFAULT_IDENTITY}"
shift $(( $# > 0 ? 1 : 0 ))
ONLY=("$@")
DEPLOYMENTS="deployments/${NETWORK}.json"
# Every CLI call carries the network explicitly so the script does not depend
# on a `stellar network add` having been run (the built-in mainnet entry has
# no RPC URL).
NET_ARGS=(--rpc-url "$RPC_URL" --network-passphrase "$PASSPHRASE")

if ! stellar keys public-key "$IDENTITY" >/dev/null 2>&1; then
  if [ "$NETWORK" = testnet ]; then
    echo "Identity '$IDENTITY' not found — generating and funding via friendbot"
    stellar keys generate "$IDENTITY" --network testnet --fund
  else
    echo "Identity '$IDENTITY' not found. Create it first, e.g." >&2
    echo "  stellar keys generate $IDENTITY   (then fund the printed G address)" >&2
    exit 1
  fi
fi
DEPLOYER_G=$(stellar keys public-key "$IDENTITY")
DEPLOYER_HEX=$(python3 -c "import base64,sys; print(base64.b32decode(sys.argv[1])[1:33].hex())" "$DEPLOYER_G")

if [ "$NETWORK" = mainnet ] && [ "${CONFIRM:-0}" != 1 ]; then
  echo "About to deploy to MAINNET from $IDENTITY ($DEPLOYER_G)."
  echo "Contracts: ${ONLY[*]:-all}. This spends real XLM and cannot be undone."
  read -r -p "Type 'mainnet' to continue: " answer
  [ "$answer" = mainnet ] || { echo "aborted"; exit 1; }
fi

echo "==> Building contracts"
stellar contract build

# Merge one entry into the manifest, creating the file on first use.
record() {
  python3 - "$DEPLOYMENTS" "$NETWORK" "$PASSPHRASE" "$RPC_URL" "$HORIZON_URL" "$@" <<'PY'
import json, os, sys, datetime
path, network, passphrase, rpc, horizon, name, field, value = sys.argv[1:9]
data = {}
if os.path.exists(path):
    with open(path) as f:
        data = json.load(f)
data.setdefault("network", network)
data.setdefault("network_passphrase", passphrase)
data.setdefault("rpc_url", rpc)
data.setdefault("horizon_url", horizon)
entry = data.setdefault("contracts", {}).setdefault(name, {})
stamp = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
if field == "id":
    # A fresh instance replaces any previous entry wholesale.
    data["contracts"][name] = {"id": value, "deployed_at": stamp}
else:
    entry["wasm_hash"] = value
    entry["uploaded_at"] = stamp
os.makedirs(os.path.dirname(path), exist_ok=True)
with open(path, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
PY
}

deploy() {
  local name=$1
  shift
  echo "==> Deploying $name"
  local id
  id=$(stellar contract deploy \
    --wasm "target/wasm32v1-none/release/${name}.wasm" \
    --source "$IDENTITY" \
    "${NET_ARGS[@]}" \
    --alias "${name}" \
    "$@" | tail -1)
  echo "    $name -> $id"
  record "$name" id "$id"
}

# Upload a contract's wasm without instantiating it, recording the wasm hash.
# For contracts that are deployed per user (one instance per smart account),
# the hash is what the server needs, not an instance ID.
upload() {
  local name=$1
  echo "==> Uploading $name wasm"
  local hash
  hash=$(stellar contract upload \
    --wasm "target/wasm32v1-none/release/${name}.wasm" \
    --source "$IDENTITY" \
    "${NET_ARGS[@]}" | tail -1)
  echo "    $name wasm_hash -> $hash"
  record "$name" wasm_hash "$hash"
}

# True when no subset was given, or when $1 is in the subset.
wanted() {
  [ ${#ONLY[@]} -eq 0 ] && return 0
  local n
  for n in "${ONLY[@]}"; do [ "$n" = "$1" ] && return 0; done
  return 1
}

# smart_account: minimal reference contract, testnet only. Constructor takes
# the initial ed25519 signer set; start with the deployer key so the account
# is immediately usable for flow testing.
if [ "$NETWORK" = testnet ]; then
  wanted smart_account && deploy smart_account -- --signers "[\"$DEPLOYER_HEX\"]"
fi

# stabl_passkey_verifier: stateless WebAuthn verifier, no constructor, no
# admin. One instance per network, shared by every passkey smart account.
wanted stabl_passkey_verifier && deploy stabl_passkey_verifier

# stabl_p256_verifier: stateless raw secp256r1 verifier for device-bound keys
# (Secure Enclave / StrongBox). No constructor, no admin, one per network.
wanted stabl_p256_verifier && deploy stabl_p256_verifier

# stabl_threshold_policy: M-of-N threshold policy, stateful but keyed per
# (account, rule). No constructor, no admin, one per network.
wanted stabl_threshold_policy && deploy stabl_threshold_policy

# stabl_multi_signer: instantiated per user by the server with that
# user's signers, so only the wasm is uploaded here.
wanted stabl_multi_signer && upload stabl_multi_signer

# stabl_account_factory: pins the multi signer wasm hash from the manifest and
# deploys per-user accounts on request. Redeploy it whenever the multi signer
# wasm changes.
if wanted stabl_account_factory; then
  ACCOUNT_WASM_HASH=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['contracts']['stabl_multi_signer']['wasm_hash'])" "$DEPLOYMENTS")
  deploy stabl_account_factory -- --wasm_hash "$ACCOUNT_WASM_HASH"
fi

echo
echo "Deployments written to $DEPLOYMENTS"
cat "$DEPLOYMENTS"
