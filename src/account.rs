#![allow(unused)]
use std::{collections::{BTreeMap, HashMap, HashSet, BTreeSet}, hash::Hash, fmt::{Debug, LowerHex, Display}, ops::{AddAssign, SubAssign}, str::FromStr};
use eigenda_client::batch::BatchHeaderHash;
use ethereum_types::U256;
use hex::{FromHexError, ToHex};
use schemars::JsonSchema;
use serde::{Serialize, Deserialize, Deserializer, Serializer};
use secp256k1::PublicKey;
use sha3::{Digest, Sha3_256, Keccak256};
use crate::{Transaction, RecoverableSignature, Certificate, RecoverableSignatureBuilder, AccountCacheError, ValidatorError, Token, ToTokenError, ArbitraryData, Metadata, MetadataValue, DataValue, AccountCache, AddressOrNamespace};

pub type AccountResult<T> = Result<T, Box<dyn std::error::Error + Send>>;

impl Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> 
    where
        S: Serializer
    {
        let hex = self.inner().iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let hex_repr = format!("0x{}", hex);
        serializer.serialize_str(&hex_repr)
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Address, D::Error> 
    where 
        D: Deserializer<'de>
    {
        let s = String::deserialize(deserializer)?;
        if s.len() != 42 {
            return Err(serde::de::Error::custom("Invalid length"))
        }

        if !s.starts_with("0x") {
            return Err(serde::de::Error::custom("'0x' prefix missing"))
        }

        let bytes = hex::decode(&s[2..]).map_err(serde::de::Error::custom)?;
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&bytes[0..20]);
        Ok(Address::from(arr))
    }
}


/// Represents a 20-byte Ethereum Compatible address.
/// 
/// This structure is used to store Ethereum Compatible addresses, which are 
/// derived from the public key. It implements traits like Clone, Copy, Debug,
/// Serialize, Deserialize, etc., for ease of use across various contexts.
#[derive(Clone, Copy, Debug, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct Address([u8; 20]);

impl Address {
    /// Creates a new address from a 20 byte array
    pub fn new(bytes: [u8; 20]) -> Address {
        Address(bytes)
    }

    /// Converts the inner Address to a full hexadecimal string
    /// this exists because in the Disply implementation we abbreviate the 
    /// address
    pub fn to_full_string(&self) -> String {
        format!("0x{:x}", self)
    }

    pub fn inner(&self) -> [u8; 20] {
        self.0.clone()
    }
}

/// Represents a 32-byte account hash.
///
/// This structure is used to store current state hash associated with an account
// It supports standard traits for easy handling and
/// comparison operations.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountHash([u8; 32]);

impl AccountHash {
    /// Creates a new `AccountHash` instance from a 32-byte array.
    ///
    /// This constructor is used to instantiate an `AccountHash` object with a given hash.
    pub fn new(hash: [u8; 32]) -> Self {
        Self(hash)
    }
}

/// This is currently not used
#[derive(Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct AccountNonce {
    bridge_nonce: crate::U256,
    send_nonce: crate::U256,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProgramNamespace(Namespace, Address);

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Namespace(pub String);

impl FromStr for Namespace {
    type Err = Box<dyn std::error::Error>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let string = s.to_string(); 
        Ok(Self(string))
    }
}

impl From<String> for Namespace {
    fn from(value: String) -> Self {
        Self(value.clone())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProgramField {
    LinkedPrograms,
    Metadata,
    Data,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProgramFieldValue {
    LinkedPrograms(LinkedProgramsValue),
    Metadata(MetadataValue),
    Data(DataValue),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkedProgramsValue {
    Insert(Address, Token),
    Extend(Vec<(Address, Token)>),
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProgramAccount {
    namespace: Namespace,
    linked_programs: BTreeMap<Address, Token>,
    //TODO(asmith): Store Metadata in the Namespace
    metadata: Metadata,
    data: ArbitraryData,
}

impl ProgramAccount {
    pub fn new(
        namespace: Namespace,
        linked_programs: Option<BTreeMap<Address, Token>>,
        metadata: Option<Metadata>,
        data: Option<ArbitraryData>
    ) -> Self {
        let linked_programs = if let Some(p) = linked_programs {
            p.clone()
        } else {
            BTreeMap::new()
        };

        let metadata = if let Some(m) = metadata {
            m.clone()
        } else {
            Metadata::new()
        };

        let data = if let Some(d) = data {
            d.clone()
        } else {
            ArbitraryData::new()
        };

        Self { namespace, linked_programs, metadata, data }
    }

    pub fn namespace(&self) -> Namespace {
        self.namespace.clone()
    }

    pub fn linked_programs(&self) -> BTreeMap<Address, Token> {
        self.linked_programs.clone()
    }

    pub fn metadata(&self) -> Metadata {
        self.metadata.clone()
    }

    pub fn data(&self) -> ArbitraryData {
        self.data.clone()
    }
}

/// Represents an LASR account.
///
/// This structure contains details of an LASR account, including its address, associated
/// programs, nonce, signatures, hashes, and certificates. It implements traits for
/// serialization, hashing, and comparison.
#[derive(Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct Account {
    namespace: Option<Namespace>,
    address: Address,
    programs: BTreeMap<Address, Token>,
    nonce: crate::U256,
    program_account_data: ArbitraryData,
    program_account_metadata: Metadata,
    program_account_linked_programs: BTreeSet<AddressOrNamespace>,
}

impl Account {
    /// Constructs a new `Account` with the given address and optional program data.
    ///
    /// This function initializes an account with the provided address and an optional
    /// map of programs. It updates the account hash before returning.
    pub fn new(
        namespace: Option<Namespace>,
        address: Address,
        programs: Option<BTreeMap<Address, Token>>
    ) -> Self {
        let mut account = Self {
            namespace,
            address,
            programs: BTreeMap::new(),
            nonce: U256::default().into(),
            program_account_data: ArbitraryData::new(),
            program_account_metadata: Metadata::new(),
            program_account_linked_programs: BTreeSet::new()
        };

        account
    }
    
    pub fn namespace(&self) -> Option<Namespace> {
        self.namespace.clone()
    }

    pub fn address(&self) -> Address {
        self.address.clone()
    }

    pub fn nonce(&self) -> crate::U256 {
        self.nonce
    }

    pub fn programs(&self) -> &BTreeMap<Address, Token> {
        &self.programs
    }

    pub fn programs_mut(&mut self) -> &mut BTreeMap<Address, Token> {
        &mut self.programs
    }

    pub fn program_account_data(&self) -> ArbitraryData {
        self.program_account_data.clone()
    }
    
    pub fn program_account_metadata(&self) -> Metadata {
        self.program_account_metadata.clone()
    }

    pub fn program_account_linked_programs(&self) -> BTreeSet<AddressOrNamespace> {
        self.program_account_linked_programs.clone()
    }

    pub fn balance(&self, program_id: &Address) -> crate::U256 {
        if let Some(entry) = self.programs().get(program_id) {
            return entry.balance()
        }

        return U256::from(0).into()
    }

    pub(crate) fn apply_transaction(
        &mut self,
        transaction: Transaction
    ) -> AccountResult<Token> {
        if let Some(mut token) = self.programs_mut().get_mut(&transaction.program_id()) {
            let new_token: Token = (token.clone(), transaction).try_into()?;
            *token = new_token;
            return Ok(token.clone())
        }
        
        if transaction.transaction_type().is_bridge_in() {
            let token: Token = transaction.into();
            self.insert_program(&token.program_id(), token.clone());
            return Ok(token)
        } 

        if transaction.to() == self.address() {
            let token: Token = transaction.into();
            self.insert_program(&token.program_id(), token.clone());
            return Ok(token) 
        }

        return Err(
            Box::new(
                ToTokenError::Custom(
                    "unable to convert transaction into token".to_string()
                )
            )
        )
    }

    pub(crate) fn insert_program(&mut self, program_id: &Address, token: Token) -> Option<Token> {
        self.programs.insert(program_id.clone(), token)
    }

    pub(crate) fn validate_program_id(&self, program_id: &Address) -> AccountResult<()> {
        if let Some(token) = self.programs.get(program_id) {
            return Ok(())
        }

        return Err(Box::new(AccountCacheError))
    }

    pub(crate) fn validate_balance(&self, program_id: &Address, amount: crate::U256) -> AccountResult<()> {
        if let Some(token) = self.programs.get(program_id) {
            if token.balance() >= amount {
                return Ok(())
            }
        }

        return Err(Box::new(AccountCacheError))
    }

    pub(crate) fn validate_token_ownership(&self, program_id: &Address, token_ids: &Vec<crate::U256>) -> AccountResult<()> {
        if let Some(token) = self.programs.get(program_id) {
            for nft in token_ids {
                if !token.token_ids().contains(&nft) {
                    return Err(Box::new(AccountCacheError))
                }
            }
            return Ok(())
        }

        return Err(Box::new(AccountCacheError))
    }

    pub(crate) fn validate_approved_spend(&self, program_id: &Address, spender: &Address, amount: &crate::U256) -> AccountResult<()> {
        if let Some(token) = self.programs.get(program_id) {
            if let Some(entry) = token.allowance().get(spender) {
                if entry > amount {
                    return Ok(())
                } else {
                    return Err(Box::new(AccountCacheError))
                }
            } else if let Some(entry) = token.approvals().get(spender) {
                if entry.is_empty() {
                    return Ok(())
                } else {
                    return Err(Box::new(AccountCacheError))
                }
            }
        } 

        return Err(Box::new(AccountCacheError))
    }

    pub(crate) fn validate_approved_token_transfer(&self, program_id: &Address, spender: &Address, token_ids: &Vec<crate::U256>) -> AccountResult<()> {
        if let Some(token) = self.programs.get(program_id) {
            if let Some(entry) = token.approvals().get(spender) {
                if entry.is_empty() {
                    return Err(Box::new(AccountCacheError))
                } else {
                    for nft in token_ids {
                        if !entry.contains(nft) {
                            return Err(Box::new(AccountCacheError))
                        }

                        return Ok(())
                    }
                }
            }
        }

        return Err(Box::new(AccountCacheError))
    }

    pub(crate) fn validate_nonce(&self, nonce: crate::U256) -> AccountResult<()> {
        log::info!("checking nonce: {nonce} > {}", self.nonce);
        if nonce > self.nonce {
            return Ok(())
        }
        return Err(Box::new(AccountCacheError))
    }

    pub(crate) fn increment_nonce(&mut self) {
        let new_nonce = U256::from(self.nonce) + U256::from(1);
        self.nonce = new_nonce.into();
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hex_str: String = self.encode_hex();
        write!(f, "0x{}...{}", &hex_str[0..4], &hex_str[hex_str.len() - 4..])
    }
}

impl From<[u8; 20]> for Address {
    fn from(value: [u8; 20]) -> Self {
        Address(value)
    }
}

impl From<&[u8; 20]> for Address {
    fn from(value: &[u8; 20]) -> Self {
        Address(*value)
    }
}


impl FromStr for Address {
    type Err = FromHexError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut hex_str = if s.starts_with("0x") {
            &s[2..]
        } else {
            s
        };

        if hex_str == "0" {
            return Ok(Address::new([0u8; 20]))
        }

        if hex_str == "1" {
            let mut inner: [u8; 20] = [0; 20];
            inner[19] = 1;
            return Ok(Address::new(inner))
        }

        let decoded = hex::decode(hex_str)?;
        if decoded.len() != 20 {
            return Err(FromHexError::InvalidStringLength);
        }

        let mut inner: [u8; 20] = [0; 20];
        inner.copy_from_slice(&decoded);
        Ok(Address::new(inner))
    }
}

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl From<Address> for [u8; 20] {
    fn from(value: Address) -> Self {
        value.0
    }
}

impl From<&Address> for [u8; 20] {
    fn from(value: &Address) -> Self {
        value.0.to_owned()
    }
}

impl From<Address> for ethereum_types::H160 {
    fn from(value: Address) -> Self {
        ethereum_types::H160(value.0)
    }
}

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

impl From<[u8; 32]> for Address {
    fn from(value: [u8; 32]) -> Self {
        let mut hasher = Keccak256::new();

        hasher.update(&value[0..]);

        let result = hasher.finalize();
        let address_bytes = &result[result.len() - 20..];
        let mut address = [0u8; 20];
        address.copy_from_slice(address_bytes);

        Address(address)
    }
}

