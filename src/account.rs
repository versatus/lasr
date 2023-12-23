#![allow(unused)]
use std::{collections::BTreeMap, hash::Hash, fmt::{Debug, LowerHex}};
use ethereum_types::U256;
use serde::{Serialize, Deserialize};
use secp256k1::PublicKey;
use sha3::{Digest, Sha3_256, Keccak256};
use crate::certificate::{RecoverableSignature, Certificate};

/// Represents a 20-byte Ethereum Compatible address.
/// 
/// This structure is used to store Ethereum Compatible addresses, which are 
/// derived from the public key. It implements traits like Clone, Copy, Debug,
/// Serialize, Deserialize, etc., for ease of use across various contexts.

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct Address([u8; 20]);

impl LowerHex for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}
impl From<ethereum_types::H160> for Address {
    fn from(value: ethereum_types::H160) -> Self {
        Address::new(value.0)
    }
}

impl Address {
    fn new(bytes: [u8; 20]) -> Address {
        Address(bytes)
    }
}

/// Represents a 32-byte account hash.
///
/// This structure is used to store current state hash associated with an account
// It supports standard traits for easy handling and
/// comparison operations.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountHash([u8; 32]);

impl AccountHash {
    /// Creates a new `AccountHash` instance from a 32-byte array.
    ///
    /// This constructor is used to instantiate an `AccountHash` object with a given hash.
    pub fn new(hash: [u8; 32]) -> Self {
        Self(hash)
    }
}

impl From<PublicKey> for Address {
    /// Converts a `PublicKey` into an `Address`.
    ///
    /// This function takes a public key, serializes it, and then performs Keccak256
    /// hashing to derive the Ethereum address. It returns the last 20 bytes of the hash
    /// as the address.
    fn from(value: PublicKey) -> Self {
        let serialized_pk = value.serialize_uncompressed();

        let mut hasher = Keccak256::new();

        hasher.update(&serialized_pk[1..]);

        let result = hasher.finalize();
        let address_bytes = &result[result.len() - 20..];
        let mut address = [0u8; 20];
        address.copy_from_slice(address_bytes);

        Address(address)
    }
}

/// Represents an LASR account.
///
/// This structure contains details of an LASR account, including its address, associated
/// programs, nonce, signatures, hashes, and certificates. It implements traits for
/// serialization, hashing, and comparison.
#[derive(Builder, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct Account {
    address: Address,
    programs: BTreeMap<Address, Token>,
    nonce: U256,
}

impl Account {
    /// Constructs a new `Account` with the given address and optional program data.
    ///
    /// This function initializes an account with the provided address and an optional
    /// map of programs. It updates the account hash before returning.
    pub fn new(
        address: Address,
        programs: Option<BTreeMap<Address, Token>>
    ) -> Self {
        let mut account = Self {
            address,
            programs: BTreeMap::new(),
            nonce: U256::default(),
        };

        account
    }

    pub fn address(&self) -> Address {
        self.address.clone()
    }

    pub fn programs(&self) -> &BTreeMap<Address, Token> {
        &self.programs
    }

    pub fn balance(&self, program_id: &Address) -> U256 {
        if let Some(entry) = self.programs().get(program_id) {
            return entry.balance()
        }

        return 0.into()
    }

    /// Updates the program data for a specific program address.
    ///
    /// This method either updates the existing program data or inserts new data if
    /// it doesn't exist for the given program address.
    pub(crate) fn update_programs(
        &mut self,
        program_id: &Address,
        delta: &TokenDelta
    ) {
        match self.programs.get(&delta.token().program_id()) {
            Some(mut entry) => {
                match &mut entry {
                    Token::Fungible { program_id, owner_id, amount, .. } => {
                        let amount = entry.update_amount(*delta.receive(), *delta.send()).clone();
                        log::info!("after updating, entry amount: {}", amount);
                    }
                    _ => {
                        // entry = delta.token().clone();
                    }
                }
            },
            None => { 
                self.programs.insert(program_id.clone(), delta.token.clone());
            }
        }
    }
}

/// Represents a generic data container.
///
/// This structure is used to store arbitrary data as a vector of bytes (`Vec<u8>`).
/// It provides a default, cloneable, serializable, and debuggable interface. It is
/// typically used for storing data that doesn't have a fixed format or structure.
#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct ArbitraryData(Vec<u8>);

impl AsRef<[u8]> for ArbitraryData {
    /// Provides a reference to the internal byte array.
    ///
    /// This method enables the `ArbitraryData` struct to be easily converted into a
    /// byte slice reference, facilitating interoperability with functions expecting
    /// a byte slice.
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Represents metadata as a byte vector.
///
/// This structure is designed to encapsulate metadata, stored as a vector of bytes.
/// It supports cloning, serialization, and debugging. The metadata can be of any
/// form that fits into a byte array, making it a flexible container.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct Metadata(Vec<u8>);

impl Metadata {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8]> for Metadata {
    /// Provides a reference to the internal byte array.
    ///
    /// This implementation allows instances of `Metadata` to be passed to functions
    /// that require a reference to a byte slice, thereby facilitating easy access
    /// to the underlying data.
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub enum Token {
    Fungible {
        program_id: Address,
        owner_id: Address,
        amount: U256,
        metadata: Metadata,
        allowance: BTreeMap<Address, U256>,
        status: Status,
    },
    NonFungible {
        program_id: Address,
        owner_id: Address,
        content_address: [u8; 32],
        metadata: Metadata,
        approvals: BTreeMap<Address, [u8; 32]>,
        status: Status
    },
    Data {
        program_id: Address,
        owner_id: Address,
        data: ArbitraryData,
        status: Status
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct TokenDelta {
    token: Token,
    send: U256,
    receive: U256,
}

impl TokenDelta {
    pub fn new(token: Token, send: U256, receive: U256) -> Self {
        Self { token, send, receive }
    }

    pub fn program_id(&self) -> Address {
        self.token().program_id()
    }

    pub fn token(&self) -> &Token {
        &self.token
    }
    
    pub fn send(&self) -> &U256 {
        &self.send
    }
    
    pub fn receive(&self) -> &U256 {
        &self.receive
    }
}

impl Token {
    pub fn program_id(&self) -> Address {
        match self {
            Token::Fungible { program_id, .. } => {
                program_id.clone()
            }
            Token::NonFungible { program_id, .. } => {
                program_id.clone()
            }
            Token::Data { program_id, .. } => {
                program_id.clone()
            }
        }
    }

    pub fn update_amount(&self, receive: U256, send: U256) -> U256 {
        match self {
            Token::Fungible { program_id, mut amount, .. } => {
                log::info!("token_id: {:x} amount before update: {}", &program_id, &amount);
                amount += receive;
                log::info!("added {} to amount", receive); 
                amount -= send;
                log::info!("subtracted {} from amount", send);
                log::info!("updated {:x} amount: {}", &program_id, &amount);
                return amount 
            }
            _ => {}
        }

        return 0.into() 
    }

    pub fn balance(&self) -> U256 {
        match self {
            Token::Fungible { amount, .. } => { return *amount }
            _ => return 0.into()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub enum Status {
    Locked,
    Free,
}
