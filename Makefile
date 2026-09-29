.PHONY: build test deploy-testnet deploy-mainnet fund clean e2e

build:
	stellar contract build

# Factory tests import the built account wasm, so build first.
test: build
	cargo test -- --show-output

deploy-testnet:
	scripts/deploy.sh testnet

# Real XLM. Prompts for confirmation; identity `deployer-mainnet` must exist
# and be funded. Pass CONTRACTS="name ..." to redeploy a subset.
deploy-mainnet:
	scripts/deploy.sh mainnet deployer-mainnet $(CONTRACTS)

# Refill the deployer account from friendbot (testnet lumens run out).
fund:
	stellar keys fund deployer --network testnet

clean:
	cargo clean


# Browser end-to-end harness against testnet (see scripts/e2e/README.md).
e2e:
	@echo "open http://localhost:4100/"
	python3 -m http.server 4100 --directory scripts/e2e
