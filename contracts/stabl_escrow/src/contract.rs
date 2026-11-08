use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, token, Address, BytesN,
    Env, String,
};

#[contracttype]
pub enum StablEscrowStorageKey {
    EscrowHolding(Address, String),
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NotFound = 1000,
    InvalidAmount = 2000,
    Exists = 3000,
    InvalidTransactionId = 4000,
    InvalidExpiry = 5000,
    Expired = 5001,
    InvalidState = 6000,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EscrowState {
    Locked = 1,
    Claimed = 2,
    Refunded = 3,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EscrowEntry {
    pub recipient: Address,
    pub params: EscrowParams,
    pub state: EscrowState,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EscrowParams {
    pub token: Address,
    pub amount: i128,
    pub expiry: u64,
    pub terms_hash: BytesN<32>,
}

#[contractevent]
pub struct EscrowStateChange {
    #[topic]
    pub recipient: Address,
    #[topic]
    pub sender: Address,
    pub transaction_id: String,
    pub new_state: EscrowState,
    pub params: EscrowParams,
}

#[contract]
pub struct StablEscrow;

#[contractimpl]
impl StablEscrow {
    /// Lock funds in escrow for a given sender and transaction_id pair. 
    pub fn lock_for_offramp(
        e: &Env,
        sender: Address,
        recipient: Address,
        transaction_id: String,
        params: EscrowParams,
    ) -> Result<EscrowEntry, Error> {
        sender.require_auth();

        if params.amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if !is_canonical_uuid(&transaction_id) {
            return Err(Error::InvalidTransactionId);
        }
        if params.expiry < e.ledger().timestamp() {
            return Err(Error::InvalidExpiry);
        }
        if escrow_key_exists(e, &sender, &transaction_id) {
            return Err(Error::Exists);
        }

        let escrow_entry = EscrowEntry {
            recipient: recipient.clone(),
            params: params.clone(),
            state: EscrowState::Locked,
        };
        Self::save(e, &sender, &transaction_id, &escrow_entry);
        let token_client = token::Client::new(e, &params.token);
        token_client.transfer(&sender, e.current_contract_address(), &params.amount);
        EscrowStateChange {
            recipient: recipient,
            transaction_id: transaction_id,
            sender: sender,
            new_state: EscrowState::Locked,
            params: params,
        }
        .publish(e);
        Ok(escrow_entry)
    }

    /// Release the escrow holding for the given sender and transaction_id to the bound recipient
    pub fn release_escrow(e: &Env, sender: Address, transaction_id: String) -> Result<EscrowEntry, Error> {
        let mut entry: EscrowEntry = Self::get_escrow_entry(e, sender.clone(), transaction_id.clone())?;
        if entry.state != EscrowState::Locked {
            return Err(Error::InvalidState)
        }
        if e.ledger().timestamp() > entry.params.expiry {
            return Err(Error::Expired)
        } 
        entry.recipient.require_auth();
        entry.state = EscrowState::Claimed;
        Self::save(e, &sender, &transaction_id, &entry);
        let recipient = entry.recipient.clone();
        let token_client = token::Client::new(e, &entry.params.token);
        token_client.transfer(&e.current_contract_address(), recipient.clone(), &entry.params.amount);
        EscrowStateChange {
            sender: sender,
            recipient: recipient.clone(),
            new_state: EscrowState::Claimed,
            transaction_id: transaction_id,
            params: entry.params.clone(),
        }.publish(e);

        Ok(entry)
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
            .ok_or(Error::NotFound)
    }

    fn save(e: &Env, sender: &Address, transaction_id: &String, escrow_entry: &EscrowEntry) {
        e.storage().persistent().set(
            &StablEscrowStorageKey::EscrowHolding(sender.clone(), transaction_id.clone()),
            &escrow_entry,
        );
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
/// True if `uuid` is a canonical UUID string: 36 bytes, lowercase hex digits
/// in the 8-4-4-4-12 layout separated by dashes. Any UUID version is
/// accepted; only the textual form is checked, so the same transaction ID
/// cannot be stored under different spellings (case, missing dashes).
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
