use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, token, Address, BytesN,
    Env, String,
};

#[contracttype]
pub enum StablEscrowStorageKey {
    Operator,
    EscrowHolding(Address, String),
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    InvalidAmount = 2,
    AlreadySet = 3,
    InvalidTransactionId = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EscrowState {
    Locked = 1,
    Claimed = 2,
    Expired = 3,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EscrowEntry {
    pub params: EscrowParams,
    pub state: EscrowState,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EscrowParams {
    pub token: Address,
    pub amount: i128,
    pub recipient: Address,
    pub expiry: u64,
    pub terms_hash: BytesN<32>,
}

#[contract]
pub struct StablEscrow;

#[contractimpl]
impl StablEscrow {
    pub fn lock_for_offramp(
        e: &Env,
        sender: Address,
        transaction_id: String,
        params: EscrowParams,
    ) -> EscrowEntry {
        sender.require_auth();
        if params.amount <= 0 {
            panic_with_error!(e, Error::InvalidAmount);
        }
        if escrow_key_exists(e, &sender, &transaction_id) {
            panic_with_error!(e, Error::AlreadySet);
        }
        if !is_canonical_uuid(&transaction_id) {
            panic_with_error!(e, Error::InvalidTransactionId);
        }
        let escrow_entry = EscrowEntry {
            params: params.clone(),
            state: EscrowState::Locked,
        };
        e.storage().persistent().set(
            &StablEscrowStorageKey::EscrowHolding(sender.clone(), transaction_id),
            &escrow_entry,
        );
        let token_client = token::Client::new(e, &params.token);
        token_client.transfer(&sender, e.current_contract_address(), &params.amount);
        escrow_entry
    }

    pub fn get_escrow_entry(
        e: &Env,
        sender: Address,
        transaction_id: String,
    ) -> Result<EscrowEntry, Error> {
        e.storage()
            .persistent()
            .get(&StablEscrowStorageKey::EscrowHolding(
                sender,
                transaction_id,
            ))
            .ok_or(Error::NotInitialized)
    }
}

fn escrow_key_exists(e: &Env, sender: &Address, transaction_id: &String) -> bool {
    e.storage()
        .persistent()
        .has(&StablEscrowStorageKey::EscrowHolding(
            sender.clone(),
            transaction_id.clone(),
        ))
}
/// Validates uuid strings are correct:
/// - UTF8
/// - lowercase
/// - UUIDV4 - 32 hex digits and 4 dashes
fn is_canonical_uuid(uuid: &String) -> bool {
    if uuid.len() != 36 {
        return false;
    }
    let mut chars = [0u8; 36];
    uuid.copy_into_slice(&mut chars);
    chars.iter().enumerate().all(|(index, &char)| match index {
        8 | 13 | 18 | 23 => char == b'-',
        _ => char.is_ascii_digit() || (b'a'..=b'f').contains(&char),
    })
}
