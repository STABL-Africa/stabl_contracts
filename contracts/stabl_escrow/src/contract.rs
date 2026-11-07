use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype,
    Address, Env,
};

#[contracttype]
pub enum StablEscrowStorageKey {
    Operator,
    Settler,
    TransactionId,
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
}

#[contract]
pub struct StablEscrow;

#[contractimpl]
impl StablEscrow {
    pub fn __constructor(e: &Env, operator: Address) {
         e.storage().instance().set(&StablEscrowStorageKey::Operator, &operator);
    }

    pub fn get_operator(e: &Env) -> Result<Address, Error> {
        e.storage().instance().get(&StablEscrowStorageKey::Operator).ok_or(Error::NotInitialized)
    }
}