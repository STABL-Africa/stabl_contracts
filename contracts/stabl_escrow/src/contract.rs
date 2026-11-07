use soroban_sdk::{
    auth::{Context, CustomAccountInterface},
    contract, contractimpl,
    crypto::Hash,
    Address, BytesN, Env, Map, String, Val, Vec,
};

#[contract]
pub struct StablEscrow;

#[contractimpl]
impl StablEscrow {
    pub fn __constructor(e: &Env, operator: Address, disburser: Address) {
    }

}