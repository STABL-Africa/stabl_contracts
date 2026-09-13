.PHONY: build test deploy-testnet fund clean

build:
	stellar contract build

test:
	cargo test -- --show-output

deploy-testnet:
	scripts/deploy_testnet.sh

# Refill the deployer account from friendbot (testnet lumens run out).
fund:
	stellar keys fund deployer --network testnet

clean:
	cargo clean
