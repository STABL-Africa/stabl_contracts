#!/usr/bin/env bash
# Back-compat shim: scripts/deploy_testnet.sh [identity] [name...]
exec "$(dirname "$0")/deploy.sh" testnet "$@"
