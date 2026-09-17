#!/usr/bin/env bash
# Build all contracts and deploy them to Stellar testnet, recording the
# resulting contract IDs in deployments/testnet.json (the manifest that
# off-chain clients read to find the current contract addresses).
#
# Usage: scripts/deploy_testnet.sh [identity] [name...]
#   identity  Stellar CLI key alias to deploy from (default: deployer)
#   name...   optional subset of contracts to (re)deploy; default is all.
#             Existing manifest entries for other contracts are left alone.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

IDENTITY="${1:-deployer}"
shift $(( $# > 0 ? 1 : 0 ))
ONLY=("$@")
NETWORK=testnet
DEPLOYMENTS=deployments/testnet.json

# Ensure the identity exists and is funded (friendbot is idempotent-safe).
if ! stellar keys public-key "$IDENTITY" >/dev/null 2>&1; then
  echo "Identity '$IDENTITY' not found — generating and funding via friendbot"
  stellar keys generate "$IDENTITY" --network "$NETWORK" --fund
fi
DEPLOYER_G=$(stellar keys public-key "$IDENTITY")
DEPLOYER_HEX=$(python3 -c "import base64,sys; print(base64.b32decode(sys.argv[1])[1:33].hex())" "$DEPLOYER_G")

echo "==> Building contracts"
stellar contract build

deploy() {
  local name=$1
  shift
  echo "==> Deploying $name"
  local id
  id=$(stellar contract deploy \
    --wasm "target/wasm32v1-none/release/${name}.wasm" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    --alias "$name" \
    "$@" | tail -1)
  echo "    $name -> $id"
  python3 - "$DEPLOYMENTS" "$name" "$id" <<'PY'
import json, os, sys, datetime
path, name, cid = sys.argv[1:4]
data = {}
if os.path.exists(path):
    with open(path) as f:
        data = json.load(f)
data.setdefault("network", "testnet")
data.setdefault("network_passphrase", "Test SDF Network ; September 2015")
data.setdefault("rpc_url", "https://soroban-testnet.stellar.org")
data.setdefault("horizon_url", "https://horizon-testnet.stellar.org")
data.setdefault("contracts", {})[name] = {
    "id": cid,
    "deployed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
}
os.makedirs(os.path.dirname(path), exist_ok=True)
with open(path, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
PY
}

# Upload a contract's wasm without instantiating it, recording the wasm hash.
# For contracts that are deployed per user (one instance per
# smart account), the hash is what the server needs, not an instance ID.
upload() {
  local name=$1
  echo "==> Uploading $name wasm"
  local hash
  hash=$(stellar contract upload \
    --wasm "target/wasm32v1-none/release/${name}.wasm" \
    --source "$IDENTITY" \
    --network "$NETWORK" | tail -1)
  echo "    $name wasm_hash -> $hash"
  python3 - "$DEPLOYMENTS" "$name" "$hash" <<'PY'
import json, os, sys, datetime
path, name, wasm_hash = sys.argv[1:4]
data = {}
if os.path.exists(path):
    with open(path) as f:
        data = json.load(f)
data.setdefault("network", "testnet")
data.setdefault("network_passphrase", "Test SDF Network ; September 2015")
data.setdefault("rpc_url", "https://soroban-testnet.stellar.org")
data.setdefault("horizon_url", "https://horizon-testnet.stellar.org")
entry = data.setdefault("contracts", {}).setdefault(name, {})
entry["wasm_hash"] = wasm_hash
entry["uploaded_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
with open(path, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
PY
}

# True when no subset was given, or when $1 is in the subset.
wanted() {
  [ ${#ONLY[@]} -eq 0 ] && return 0
  local n
  for n in "${ONLY[@]}"; do [ "$n" = "$1" ] && return 0; done
  return 1
}

# smart_account: constructor takes the initial ed25519 signer set; start with
# the deployer key so the account is immediately usable for flow testing.
wanted smart_account && deploy smart_account -- --signers "[\"$DEPLOYER_HEX\"]"

# stabl_passkey_verifier: stateless WebAuthn verifier, no constructor, no
# admin. One instance per network, shared by every passkey smart account.
wanted stabl_passkey_verifier && deploy stabl_passkey_verifier

# stabl_passkey_multi_signer: instantiated per user by stabl_pay with that
# user's signers, so only the wasm is uploaded here.
wanted stabl_passkey_multi_signer && upload stabl_passkey_multi_signer

# stabl_account_factory: pins the multi signer wasm hash from the manifest and
# deploys per-user accounts on request. Redeploy it whenever the multi signer
# wasm changes.
if wanted stabl_account_factory; then
  ACCOUNT_WASM_HASH=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['contracts']['stabl_passkey_multi_signer']['wasm_hash'])" "$DEPLOYMENTS")
  deploy stabl_account_factory -- --wasm_hash "$ACCOUNT_WASM_HASH"
fi

echo
echo "Deployments written to $DEPLOYMENTS"
cat "$DEPLOYMENTS"
