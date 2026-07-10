#!/usr/bin/env bash
# Build all contracts and deploy them to Stellar testnet, recording the
# resulting contract IDs in deployments/testnet.json (which stabl_pay reads).
#
# Usage: scripts/deploy_testnet.sh [identity]
#   identity  Stellar CLI key alias to deploy from (default: deployer)
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

IDENTITY="${1:-deployer}"
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

# smart_account: constructor takes the initial ed25519 signer set; start with
# the deployer key so the account is immediately usable for flow testing.
deploy smart_account -- --signers "[\"$DEPLOYER_HEX\"]"

echo
echo "Deployments written to $DEPLOYMENTS"
cat "$DEPLOYMENTS"
