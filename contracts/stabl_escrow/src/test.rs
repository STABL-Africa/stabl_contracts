#![cfg(test)]
extern crate std;

use crate::contract::{StablEscrow, StablEscrowClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn constructor_stores_signers() {
    let env = Env::default();
    let operator = Address::generate(&env);
    std::dbg!(&operator);
    let contract_id = env.register(StablEscrow, (&operator,));
    let client = StablEscrowClient::new(&env, &contract_id);
    std::dbg!(client.get_operator());
}
