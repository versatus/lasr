use std::{collections::{BTreeMap, BTreeSet}, hash::Hash, fmt::{Debug, LowerHex, Display}, str::FromStr};
use hex::{FromHexError, ToHex};
use schemars::JsonSchema;
use serde::{ Serialize, Deserialize, Deserializer, Serializer};
use serde::de::Visitor;
use secp256k1::PublicKey;
use sha3::{Digest, Keccak256};
use crate::{
    Transaction, Token, ToTokenError, ArbitraryData, Metadata, 
    MetadataValue, DataValue, AddressOrNamespace, ProgramUpdate, TokenBuilder, 
    TokenUpdateField, Status
};
use derive_builder::Builder;

pub type AccountError = std::io::Error;

pub type AccountResult<T> = Result<T, Box<dyn std::error::Error + Send>>;

impl Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> 
    where
        S: Serializer
    {
        let hex_string = hex::encode(self.inner());
        serializer.serialize_str(&format!("0x{}", hex_string))
    }
}

struct AddressVisitor;

impl<'de> Visitor<'de> for AddressVisitor {
    type Value = Address;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an address in either hex string or byte array format")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if value.starts_with("0x") {
            let bytes = hex::decode(&value[2..]).map_err(E::custom)?;
            if bytes.len() == 20 {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(&bytes);
                Ok(Address(arr))
            } else {
                Err(E::custom("Hex string does not represent a valid Address"))
            }
        } else if value.starts_with("[") && value.ends_with("]") {
            let bytes_str = &value[1..value.len() - 1];
            let bytes: Vec<u8> = bytes_str.split(',')
                .map(str::trim)
                .map(|s| s.parse::<u8>().map_err(E::custom))
                .collect::<Result<Vec<u8>, E>>()?;

            if bytes.len() == 20 {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(&bytes);
                Ok(Address(arr))
            } else {
                Err(E::custom("invalid length for address"))
            }
        } else {
            Err(E::custom("Invalid address format"))
        }
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Address, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(AddressVisitor)
    }
}

/// Represents a 20-byte Ethereum Compatible address.
/// 
/// This structure is used to store Ethereum Compatible addresses, which are 
/// derived from the public key. It implements traits like Clone, Copy, Debug,
/// Serialize, Deserialize, etc., for ease of use across various contexts.
#[derive(Clone, Copy, Debug, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
#[serde(rename_all = "camelCase")]
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

    pub fn from_hex(hex_str: &str) -> Result<Self, FromHexError> {
        let hex_str = if hex_str.starts_with("0x") { &hex_str[2..] } else { hex_str };
        let bytes = hex::decode(hex_str)?;
        let mut addr_inner = [0u8; 20];
        if bytes.len() != 20 {
            return Err(FromHexError::OddLength)
        }

        addr_inner.copy_from_slice(&bytes[..]);
        return Ok(Address(addr_inner));
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct AccountNonce {
    bridge_nonce: crate::U256,
    send_nonce: crate::U256,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub struct ProgramNamespace(Namespace, Address);

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub enum ProgramField {
    LinkedPrograms,
    Metadata,
    Data,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ProgramFieldValue {
    LinkedPrograms(LinkedProgramsValue),
    Metadata(MetadataValue),
    Data(DataValue),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum LinkedProgramsValue {
    Insert(Address),
    Extend(Vec<Address>),
    Remove(Address),
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
#[serde(rename_all = "camelCase")]
pub enum AccountType {
    User,
    Program(Address),
}

/// Represents an LASR account.
///
/// This structure contains details of an LASR account, including its address, associated
/// programs, nonce, signatures, hashes, and certificates. It implements traits for
/// serialization, hashing, and comparison.
#[derive(Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
#[serde(rename_all = "camelCase")]
pub struct Account {
    account_type: AccountType,
    program_namespace: Option<AddressOrNamespace>,
    owner_address: Address,
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
        account_type: AccountType,
        program_namespace: Option<AddressOrNamespace>,
        owner_address: Address,
        _programs: Option<BTreeMap<Address, Token>>
    ) -> Self {
        let account = Self {
            account_type,
            program_namespace,
            owner_address,
            programs: BTreeMap::new(),
            nonce: crate::U256::default().into(),
            program_account_data: ArbitraryData::new(),
            program_account_metadata: Metadata::new(),
            program_account_linked_programs: BTreeSet::new()
        };

        account
    }
    
    pub fn account_type(&self) -> AccountType {
        self.account_type.clone()
    }

    pub fn program_namespace(&self) -> Option<AddressOrNamespace> {
        self.program_namespace.clone()
    }

    pub fn owner_address(&self) -> Address {
        self.owner_address.clone()
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

    pub fn program_account_data(&self) -> &ArbitraryData {
        &self.program_account_data
    }

    pub fn program_account_data_mut(&mut self) -> &mut ArbitraryData {
        &mut self.program_account_data
    }
    
    pub fn program_account_metadata(&self) -> &Metadata {
        &self.program_account_metadata
    }

    pub fn program_account_metadat_mut(&mut self) -> &mut Metadata {
        &mut self.program_account_metadata
    }

    pub fn program_account_linked_programs(&self) -> &BTreeSet<AddressOrNamespace> {
        &self.program_account_linked_programs
    }

    pub fn program_account_linked_programs_mut(&mut self) -> &mut BTreeSet<AddressOrNamespace> {
        &mut self.program_account_linked_programs
    }

    pub fn balance(&self, program_id: &Address) -> crate::U256 {
        if let Some(entry) = self.programs().get(program_id) {
            return entry.balance()
        }

        return crate::U256::from(0)
    }

    pub fn apply_send_transaction(
        &mut self,
        transaction: Transaction
    ) -> AccountResult<Token> {
        if let Some(token) = self.programs_mut().get_mut(&transaction.program_id()) {
            let new_token: Token = (token.clone(), transaction.clone()).try_into()?;
            *token = new_token;
            return Ok(token.clone())
        }
        
        if transaction.transaction_type().is_bridge_in() {
            let token: Token = transaction.into();
            self.insert_program(&token.program_id(), token.clone());
            return Ok(token)
        } 

        if transaction.to() == self.owner_address() {
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

    pub fn apply_transfer_to_instruction(
        &mut self,
        token_address: &Address,
        amount: &Option<crate::U256>,
        token_ids: &Vec<crate::U256>
    ) -> AccountResult<Token> {
        if let Some(entry) = self.programs_mut().get_mut(token_address) {
            if let Some(amt) = amount {
                entry.credit(amt)?;
            } 

            if !token_ids.is_empty() {
                entry.add_token_ids(&token_ids)?;
            }
            return Ok(entry.clone())
        } else {
            let mut token = TokenBuilder::default()
                .program_id(token_address.clone())
                .owner_id(self.owner_address.clone())
                .balance(crate::U256::from(0))
                .token_ids(vec![])
                .metadata(Metadata::new())
                .data(ArbitraryData::new())
                .approvals(BTreeMap::new())
                .allowance(BTreeMap::new())
                .status(crate::Status::Free)
                .build().map_err(|e| {
                    Box::new(e) as Box<dyn std::error::Error + Send>
                })?;

            if let Some(amt) = amount {
                token.credit(amt)?;
            }

            if !token_ids.is_empty() {
                token.add_token_ids(&token_ids)?;
            }
            self.programs_mut().insert(token.program_id(), token.clone());

            return Ok(token)
        }
    }

    pub fn apply_transfer_from_instruction(
        &mut self,
        token_address: &Address, 
        amount: &Option<crate::U256>,
        token_ids: &Vec<crate::U256>
    ) -> AccountResult<Token> {
        if let Some(entry) = self.programs_mut().get_mut(token_address) {
            if let Some(amt) = amount {
                entry.debit(amt)?;
            } 

            if !token_ids.is_empty() {
                entry.remove_token_ids(&token_ids)?;
            }
            return Ok(entry.clone())
        }

        return Err(
            Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "cannot transfer a token that the caller doesn't own".to_string()
                )
            )
        )
    }

    pub fn apply_burn_instruction(
        &mut self,
        token_address: &Address,
        amount: &Option<crate::U256>,
        token_ids: &Vec<crate::U256>
    ) -> AccountResult<Token> {
        // Check if caller is this address, if so, 
        if let Some(entry) = self.programs_mut().get_mut(token_address) {
            if let Some(amt) = amount {
                entry.debit(amt)?;
            }

            if !token_ids.is_empty() {
                entry.remove_token_ids(token_ids)?;
            }

            return Ok(entry.clone())
        }

        return Err(
            Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Account cannot have a token that it does not own burned"
                )
            )
        )
    }

    pub fn apply_token_distribution(
        &mut self,
        program_id: &Address,
        amount: &Option<crate::U256>,
        token_ids: &Vec<crate::U256>,
        token_updates: &Vec<TokenUpdateField>
    ) -> AccountResult<Token> {
        let token_owner = {
            if let AccountType::Program(program_account_address) = self.account_type() {
                log::warn!("applying distribution to program acocunt: {}", &program_account_address.to_full_string());
                program_account_address
            } else {
                self.owner_address()
            }
        };

        if let Some(token) = self.programs.get_mut(program_id) {

            if let Some(amt) = amount {
                log::warn!("applying {} to {}", &amt, &token_owner);
                token.credit(amt)?;
            }

            if !token_ids.is_empty() {
                token.add_token_ids(token_ids)?;
            }

            for update in token_updates {
                log::info!("Applying token update: {:?}", &update);
                token.apply_token_update_field_values(update.value())?;
            }

            return Ok(token.clone())

        } else {

            log::info!("creating token for token distribution");
            let token_owner = {
                if let AccountType::Program(program_account_address) = self.account_type() {
                    program_account_address
                } else {
                    self.owner_address()
                }
            };

            let mut token = TokenBuilder::default()
                .program_id(program_id.clone())
                .owner_id(token_owner.clone())
                .balance(crate::U256::from(0))
                .token_ids(vec![])
                .metadata(Metadata::new())
                .data(ArbitraryData::new())
                .approvals(BTreeMap::new())
                .allowance(BTreeMap::new())
                .status(Status::Free)
                .build()
                .map_err(|e| {
                    Box::new(
                        e
                    ) as Box<dyn std::error::Error + Send>
                })?;

            if let Some(amt) = amount {
                log::warn!("applying {} to {}", &amt, &token_owner);
                log::warn!("applying credits to token: {:?}", &token);
                token.credit(amt)?;
                log::warn!("applied credits from token distribution");
            }

            if !token_ids.is_empty() {
                token.add_token_ids(token_ids)?;
                log::warn!("applied token ids from token distribution");
            }

            log::warn!("token distribution includes token updates: {:?}", &token_updates);
            for update in token_updates {
                log::warn!("Applying token update: {:?}", &update);
                token.apply_token_update_field_values(update.value())?;
            }

            log::warn!("inserting token: {} into account {}", token.program_id(), token_owner);
            self.programs.insert(token.program_id(), token.clone());

            Ok(token.clone())
        }
    }

    pub fn apply_token_update(
        &mut self,
        program_id: &Address,
        updates: &Vec<TokenUpdateField>
    ) -> AccountResult<Token> {
        let owner_address = {
            if let AccountType::Program(program_account_address) = self.account_type() {
                program_account_address
            } else {
                self.owner_address()
            }
        };

        if let Some(token) = self.programs_mut().get_mut(&program_id) {
            for update in updates {
                token.apply_token_update_field_values(update.value())?;
            }
            return Ok(token.clone())
        } else {
            let mut token = TokenBuilder::default()
                .program_id(program_id.clone())
                .owner_id(owner_address.clone())
                .balance(crate::U256::from(0))
                .token_ids(vec![])
                .approvals(BTreeMap::new())
                .allowance(BTreeMap::new())
                .data(ArbitraryData::new())
                .metadata(Metadata::new())
                .status(crate::Status::Free)
                .build()
                .map_err(|e| {
                    Box::new(
                        e
                    ) as Box<dyn std::error::Error + Send>
                })?;
            
            for update in updates {
                if let AccountType::Program(program_addr) = self.account_type() {
                    log::warn!("applying {:?} to account: {}", &update, &program_addr.to_full_string());
                }
                token.apply_token_update_field_values(update.value())?;
            }

            self.programs_mut().insert(program_id.clone(), token.clone());
            Ok(token)
        }
    }

    fn apply_program_update_field_values(
        &mut self,
        update_field_value: &ProgramFieldValue
    ) -> AccountResult<()> {
        log::warn!("applying program update field value: {:?}", update_field_value);
        match update_field_value {
            ProgramFieldValue::LinkedPrograms(linked_programs_value) => {
                match linked_programs_value {
                    LinkedProgramsValue::Insert(linked_program) => {
                        self.program_account_linked_programs.insert(
                            AddressOrNamespace::Address(linked_program.clone())
                        );
                    }
                    LinkedProgramsValue::Extend(linked_programs) => {
                        self.program_account_linked_programs.extend(
                            linked_programs.into_iter()
                            .cloned()
                            .map(|lp| {
                                AddressOrNamespace::Address(lp.clone())
                            })
                        );
                    }
                    LinkedProgramsValue::Remove(linked_program) => {
                        self.program_account_linked_programs.remove(
                            &AddressOrNamespace::Address(linked_program.clone())
                        );
                    }
                }
            }
            ProgramFieldValue::Metadata(metadata_value) => {
                match metadata_value {
                    MetadataValue::Insert(key, value) => {
                        self.program_account_metadata
                            .inner_mut()
                            .insert(
                                key.clone(),
                                value.clone()
                            );
                    }
                    MetadataValue::Extend(iter) => {
                        log::warn!("extending metdata");
                        log::warn!("current metadata: {:?}", self.program_account_metadata);
                        self.program_account_metadata
                            .inner_mut()
                            .extend(iter.clone());
                        log::warn!("metadata after update: {:?}", self.program_account_metadata);
                    }
                    MetadataValue::Remove(key) => {
                        self.program_account_metadata
                            .inner_mut()
                            .remove(key);
                    }

                }
            }
            ProgramFieldValue::Data(data_value) => {
                match data_value {
                    DataValue::Insert(key, value) => {
                        self.program_account_data
                            .inner_mut()
                            .insert(
                                key.clone(),
                                value.clone()
                            );
                    }
                    DataValue::Extend(iter) => {
                        self.program_account_data
                            .inner_mut()
                            .extend(iter.clone())
                    }
                    DataValue::Remove(key) => {
                        self.program_account_data
                            .inner_mut()
                            .remove(key);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn apply_program_update(&mut self, update: &ProgramUpdate) -> AccountResult<()> {
        let _program_addr = if let AccountType::Program(program_addr) = self.account_type() {
            program_addr
        } else {
            return Err(
                Box::new(
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Account is not a program account and cannot accept a program update"
                    )
                ) as Box<dyn std::error::Error + Send>
            )
        };

        for update in update.updates() {
            self.apply_program_update_field_values(update.value())?;
        }

        Ok(())
    }

    pub fn insert_program(&mut self, program_id: &Address, token: Token) -> Option<Token> {
        self.programs.insert(program_id.clone(), token)
    }

    pub fn validate_program_id(&self, program_id: &Address) -> AccountResult<()> {
        if let Some(_token) = self.programs.get(program_id) {
            return Ok(())
        }

        return Err(
            Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::Other, 
                    format!(
                        "account does not have associated program: {}",
                        program_id.to_full_string()
                    )
                )
            )
        )
    }

    pub fn validate_balance(&self, program_id: &Address, amount: crate::U256) -> AccountResult<()> {
        if let Some(token) = self.programs.get(program_id) {
            if token.balance() >= amount {
                return Ok(())
            } else {
                return Err(
                    Box::new(
                        std::io::Error::new(
                            std::io::ErrorKind::Other, 
                            "account balance insufficient"
                        )
                    )
                )
            }
        }

        return Err(
            Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "account does not have associated program: {}",
                        program_id.to_full_string()
                    )
                )
            )
        )
    }

    pub fn validate_token_ownership(&self, program_id: &Address, token_ids: &Vec<crate::U256>) -> AccountResult<()> {
        if let Some(token) = self.programs.get(program_id) {
            for nft in token_ids {
                if !token.token_ids().contains(&nft) {
                    return Err(
                        Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("account does not own token_id: 0x{:x}", nft)
                            )
                        )
                    )
                }
            }
            return Ok(())
        }

        return Err(
            Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "account does not have associated program: {}",
                        program_id.to_full_string()
                    )
                )
            )
        )
    }

    pub fn validate_approved_spend(&self, program_id: &Address, spender: &Address, amount: &crate::U256) -> AccountResult<()> {
        if let Some(token) = self.programs.get(program_id) {
            if let Some(entry) = token.allowance().get(spender) {
                if entry > amount {
                    return Ok(())
                } else {
                    return Err(
                        Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "amount exceeds approved limit"
                            )
                        )
                    )
                }
            } else if let Some(entry) = token.approvals().get(spender) {
                if entry.is_empty() {
                    return Ok(())
                } else {
                    return Err(
                        Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "spender not approved by this account"
                            )
                        )
                    )
                }
            } else if let AccountType::Program(program_addr) = self.account_type() {
                if &program_addr == program_id {
                    return Ok(())
                } 
            }
        }

        return Err(
            Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "account does not have associated program: {}",
                        program_id.to_full_string()
                    )
                )
            )
        )
    }

    pub fn validate_approved_token_transfer(&self, program_id: &Address, spender: &Address, token_ids: &Vec<crate::U256>) -> AccountResult<()> {
        if let Some(token) = self.programs.get(program_id) {
            if let Some(entry) = token.approvals().get(spender) {
                if entry.is_empty() {
                    return Err(
                        Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "spender not approved by this account"
                            )
                        )
                    )
                } else {
                    for nft in token_ids {
                        if !entry.contains(nft) {
                            return Err(
                                Box::new(
                                    std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        format!("spender not approved to spend token_id: {}", nft)
                                    )
                                )
                            )
                        }

                        return Ok(())
                    }
                }
            } else if let AccountType::Program(program_addr) = self.account_type() {
                if &program_addr == program_id {
                    return Ok(())
                }
            }
        } 

        return Err(
            Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "account does not have associated program: {}",
                        program_id.to_full_string()
                    )
                )
            )
        )
    }

    pub fn validate_nonce(&self, nonce: crate::U256) -> AccountResult<()> {
        log::info!("checking nonce: {nonce} > {}", self.nonce);
        if self.nonce == crate::U256::from(0) && 
            nonce == crate::U256::from(0) {
            return Ok(())
        }
        if nonce > self.nonce {
            return Ok(())
        }

        return Err(
            Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "unable to validate nonce"
                )
            )
        )
    }

    pub fn increment_nonce(&mut self, new_nonce: &crate::U256) {
        self.nonce = *new_nonce;
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
        let hex_str = if s.starts_with("0x") {
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
