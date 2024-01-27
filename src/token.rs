use serde::{Serialize, Deserialize, Serializer, Deserializer, de::Visitor};
use ethereum_types::U256 as EthU256;
use std::collections::BTreeMap;
use std::fmt::{Display, Debug};
use std::ops::{AddAssign, SubAssign};
use schemars::JsonSchema;

use crate::{Address, RecoverableSignature, Transaction};

pub const TOKEN_WITNESS_VERSION: &'static str = "0.1.0";

#[derive(Copy, Clone, Default, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct U256(pub [u64; 4]);

impl Serialize for U256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_string = self.0.iter()
            .map(|&num| format!("{:016x}", num))  // Format each u64 as a 16-character hex string
            .collect::<String>();
        serializer.serialize_str(&hex_string)
    }
}

impl<'de> Deserialize<'de> for U256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(U256Visitor)
    }
}

struct U256Visitor;

impl<'de> Visitor<'de> for U256Visitor {
    type Value = U256;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a 64-character hex string")
    }

    fn visit_str<E>(self, v: &str) -> Result<U256, E>
    where
        E: serde::de::Error,
    {
        if v.len() != 64 {
            return Err(E::invalid_length(v.len(), &self));
        }

        let mut nums = [0u64; 4];
        for (i, chunk) in v.as_bytes().chunks(16).enumerate() {
            nums[i] = u64::from_str_radix(std::str::from_utf8(chunk).map_err(E::custom)?, 16)
                .map_err(E::custom)?;
        }

        Ok(U256(nums))
    }
}

impl Display for U256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
       let to_display: EthU256 = self.into();
       write!(f, "{}", to_display)
    }
}

impl Debug for U256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let to_display: EthU256 = self.into();
        write!(f, "{:?}", to_display)
    }
}

impl std::ops::Add for U256 {
    type Output = U256;
    fn add(self, rhs: Self) -> Self::Output {
        let new: EthU256 = EthU256::from(self) + EthU256::from(rhs);
        new.into()
    }
}

impl std::ops::Sub for U256 {
    type Output = U256;
    fn sub(self, rhs: Self) -> Self::Output {
        let new: EthU256 = EthU256::from(self) - EthU256::from(rhs);
        new.into()
    }
}

impl AddAssign for U256 {
    fn add_assign(&mut self, rhs: Self) {
        let new: EthU256 = EthU256::from(*self) + EthU256::from(rhs);
        *self = new.into();
    }
}

impl SubAssign for U256 {
    fn sub_assign(&mut self, rhs: Self) {
        let new: EthU256 = EthU256::from(*self) - EthU256::from(rhs);
        *self = new.into();
    }
}

impl AddAssign for &mut U256 {
    fn add_assign(&mut self, rhs: Self) {
        let new: EthU256 = EthU256::from(self.clone()) + EthU256::from(rhs);
        *self = new.into();
    }
}

impl SubAssign for &mut U256 {
    fn sub_assign(&mut self, rhs: Self) {
        let new: EthU256 = EthU256::from(self.clone()) - EthU256::from(rhs);
        *self = new.into();
    }
}

impl AddAssign for &U256 {
    fn add_assign(&mut self, rhs: Self) {
        let new: EthU256 = EthU256::from(*self) + EthU256::from(rhs);
        *self = new.into();
    }
}

impl SubAssign for &U256 {
    fn sub_assign(&mut self, rhs: Self) {
        let new: EthU256 = EthU256::from(*self) - EthU256::from(rhs);
        *self = new.into();
    }
}

impl From<EthU256> for &mut U256 {
    fn from(value: EthU256) -> Self {
        value.into()
    }
}

impl From<EthU256> for &U256 {
    fn from(value: EthU256) -> Self {
        value.into()
    }
}

impl From<&mut U256> for U256 {
    fn from(value: &mut U256) -> Self {
        value.clone()
    }
}

impl From<&mut U256> for EthU256 {
    fn from(value: &mut U256) -> Self {
        EthU256(value.0.clone())
    }
}

impl From<U256> for EthU256 {
    fn from(value: U256) -> Self {
        EthU256(value.0)
    }
}

impl From<EthU256> for U256 {
    fn from(value: EthU256) -> Self {
        U256(value.0)
    }
}

impl From<&U256> for EthU256 {
    fn from(value: &U256) -> Self {
        EthU256(value.0.clone())
    }
}

impl From<&EthU256> for U256 {
    fn from(value: &EthU256) -> Self {
        U256(value.0.clone())
    }
}


/// Represents a generic data container.
///
/// This structure is used to store arbitrary data as a vector of bytes (`Vec<u8>`).
/// It provides a default, cloneable, serializable, and debuggable interface. It is
/// typically used for storing data that doesn't have a fixed format or structure.
#[derive(Clone, Default, Debug, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct ArbitraryData(Vec<u8>);

impl ArbitraryData {
    pub fn new() -> Self {
        Self(vec![])
    }

    pub fn pop(&mut self) -> Option<u8> {
        self.0.pop()
    }

    pub fn inner(&self) -> &Vec<u8> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }
}

// Implementing the Serialize trait for Address
impl Serialize for ArbitraryData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_string = self.0.iter().map(|byte| format!("{:02x}", byte)).collect::<String>();
        serializer.serialize_str(&hex_string)
    }
}

// Implementing Deserialize for ArbitraryData
impl<'de> Deserialize<'de> for ArbitraryData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(ArbitraryDataVisitor)
    }
}

// Implementing a visitor for ArbitraryData
struct ArbitraryDataVisitor;

impl<'de> Visitor<'de> for ArbitraryDataVisitor {
    type Value = ArbitraryData;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a hex string")
    }

    fn visit_str<E>(self, v: &str) -> Result<ArbitraryData, E>
    where
        E: serde::de::Error,
    {
        let bytes = hex::decode(v).map_err(serde::de::Error::custom)?;
        Ok(ArbitraryData(bytes))
    }
}

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

impl From<Vec<u8>> for ArbitraryData {
    fn from(value: Vec<u8>) -> Self {
        Self(value.clone())
    }
}

/// Represents metadata as a byte vector.
///
/// This structure is designed to encapsulate metadata, stored as a vector of bytes.
/// It supports cloning, serialization, and debugging. The metadata can be of any
/// form that fits into a byte array, making it a flexible container.
#[derive(Clone, Debug, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct Metadata(Vec<u8>);

impl Metadata {
    pub fn new() -> Self {
        Self(vec![])
    }

    pub fn inner_mut(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }

    pub fn inner(&self) -> &Vec<u8> {
        &self.0
    }
}

// Implementing Serialize for Metadata
impl Serialize for Metadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_string = self.0.iter().map(|byte| format!("{:02x}", byte)).collect::<String>();
        serializer.serialize_str(&hex_string)
    }
}

// Implementing Deserialize for Metadata
impl<'de> Deserialize<'de> for Metadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(MetadataVisitor)
    }
}

// Implementing a visitor for Metadata
struct MetadataVisitor;

impl<'de> Visitor<'de> for MetadataVisitor {
    type Value = Metadata;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a hex string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Metadata, E>
    where
        E: serde::de::Error,
    {
        let bytes = hex::decode(v).map_err(serde::de::Error::custom)?;
        Ok(Metadata(bytes))
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

impl From<Vec<u8>> for Metadata {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}


#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub enum TokenType {
    Fungible,
    NonFungible,
    Data
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct Token {
    program_id: Address,
    owner_id: Address,
    balance: U256,
    metadata: Metadata,
    token_ids: Vec<U256>,
    allowance: BTreeMap<Address, U256>,
    approvals: BTreeMap<Address, Vec<U256>>,
    data: ArbitraryData,
    status: Status,
}

impl Token {
    pub(crate) fn debit(&mut self, amount: &U256) -> Result<(), Box<dyn std::error::Error + Send>> {
        if amount > &self.balance {
            return Err(
                Box::new(
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "transfer amount exceeds balance".to_string() 
                    )
                )
            )
        }

        self.balance -= *amount;
        return Ok(())
    }

    pub(crate) fn credit(&mut self, amount: &U256) -> Result<(), Box<dyn std::error::Error + Send>> {
        self.balance += *amount;
        return Ok(())
    }

    pub(crate) fn remove_token_ids(&mut self, token_ids: &Vec<U256>) -> Result<(), Box<dyn std::error::Error + Send>> {
        let positions: Vec<usize> = { 
            token_ids.iter().filter_map(|nft| {
                self.token_ids.iter().position(|i| i == nft)
            }).collect()
        };

        if positions.len() != token_ids.len() {
            return Err(
                Box::new(
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "one or more of the token ids is not owned by the from account"
                    )
                )
            )
        }

        self.token_ids.retain(|i| !token_ids.contains(i)); 

        Ok(())
    }

    pub(crate) fn add_token_ids(&mut self, token_ids: &Vec<U256>) -> Result<(), Box<dyn std::error::Error + Send>> {
        self.token_ids.extend(token_ids);
        Ok(())
    }

    pub(crate) fn apply_token_update_field_values(&mut self, token_update_value: &TokenFieldValue) -> Result<(), Box<dyn std::error::Error + Send>> {
        match token_update_value { 
            TokenFieldValue::Data(data_update) => {
                self.apply_data_update(data_update)?;
            }
            TokenFieldValue::Metadata(metadata_update) => {
                self.apply_metadata_update(metadata_update)?;
            }
            TokenFieldValue::Approvals(approvals_update) => {
                self.apply_approvals_update(approvals_update)?;
            }
            TokenFieldValue::Allowance(allowance_update) => {
                self.apply_allowance_update(allowance_update)?;
            }
            TokenFieldValue::Status(status_update) => {
                self.apply_status_update(status_update)?;
            }
            TokenFieldValue::TokenIds(_token_ids_update) => {
                return Err(
                    Box::new(
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "Updating token_ids through a Create should be done via the `token_ids` field not as a TokenFieldValue"
                        )
                    ) as Box<dyn std::error::Error + Send>
                )
            }
            TokenFieldValue::Balance(_balance_update) => {
                return Err(
                    Box::new(
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "Updating balance through a Create should be done via the `amount` field not as a TokenFieldValue"
                        )
                    ) as Box<dyn std::error::Error + Send>
                )
            }
        }

        Ok(())
    }

    fn apply_data_update(&mut self, data_update: &DataValue) -> Result<(), Box<dyn std::error::Error + Send>> {
        match data_update {
            DataValue::Pop => {
                self.data.inner_mut().pop();
            }
            DataValue::Push(byte) => {
                self.data.inner_mut().push(*byte);
            }
            DataValue::Extend(bytes) => {
                self.data.inner_mut().extend(bytes.0.clone());
            }
            DataValue::ReplaceAll(bytes) => {
                self.data = ArbitraryData::from(bytes.0.clone());
            }
            DataValue::ReplaceByte(index, byte) => {
                if self.data().0.len() < index + 1 {
                    return Err(
                        Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Index out of range trying to replace a single byte in Arbitrary data in a token"
                            )
                        ) as Box<dyn std::error::Error + Send> 
                    )
                }
                self.data.inner_mut()[*index] = *byte;
            }
            DataValue::ReplaceSlice(start, end, bytes) => {
                if bytes.0.len() != (end - start) {
                    return Err(
                        Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "bytes do not equal the length of the slice to replace in Arbitrary data in a token"
                            )
                        ) as Box<dyn std::error::Error + Send> 
                    )
                }
                if bytes.0.len() < *start || bytes.0.len() < *end {
                    return Err(
                        Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Index out of range trying to replace a slice of bytes in Arbitrary data in a token"
                            )
                        ) as Box<dyn std::error::Error + Send> 
                    )
                }
                self.data.inner_mut().splice(start..end, bytes.0.clone());
            }
        }

        Ok(())
    }

    fn apply_metadata_update(&mut self, metadata_update: &MetadataValue) -> Result<(), Box<dyn std::error::Error + Send>> {
        match metadata_update {
            MetadataValue::Pop => {
                self.metadata.inner_mut().pop();
            }
            MetadataValue::Extend(bytes) => {
                self.metadata.inner_mut().extend(bytes.0.clone());
            }
            MetadataValue::Push(byte) => {
                self.metadata.inner_mut().push(*byte);
            }
            MetadataValue::ReplaceAll(bytes) => {
                self.metadata = bytes.clone();
            }
            MetadataValue::ReplaceByte(index, byte) => {
                if self.metadata().0.len() < index + 1 {
                    return Err(
                        Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Index out of range trying to replace a single byte in Metadata in a token"
                            )
                        ) as Box<dyn std::error::Error + Send> 
                    )
                }
                self.data.inner_mut()[*index] = *byte;
            }
            MetadataValue::ReplaceSlice(start, end, bytes) => {
                if bytes.len() != (end - start) {
                    return Err(
                        Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "bytes do not equal the length of the slice to replace in Metadata in a token"
                            )
                        ) as Box<dyn std::error::Error + Send> 
                    )
                }
                if bytes.len() < *start || bytes.len() < *end {
                    return Err(
                        Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Index out of range trying to replace a slice of bytes in Metadata in a token"
                            )
                        ) as Box<dyn std::error::Error + Send> 
                    )
                }
                self.data.inner_mut().splice(start..end, bytes.clone());
            }
        }

        Ok(())
    }

    fn apply_approvals_update(&mut self, approvals_update: &ApprovalsValue) -> Result<(), Box<dyn std::error::Error + Send>> {
        match approvals_update {
            ApprovalsValue::Insert(key, value) => {
                if let Some(entry) = self.approvals.get_mut(key) {
                    entry.extend(value.clone());
                } else {
                    self.approvals.insert(key.clone(), value.clone());
                }
            }
            ApprovalsValue::Extend(values) => {
                self.approvals.extend(values.into_iter().cloned());
            }
            ApprovalsValue::Remove(key, values) => {
                let mut is_empty = false;
                if let Some(entry) = self.approvals.get_mut(key) {
                    entry.retain(|v| !values.contains(v));
                    is_empty = entry.is_empty(); 
                }
                if is_empty {
                    self.approvals.remove(key);
                }
            }
            ApprovalsValue::Revoke(key) => {
                self.approvals.remove(key);
            }
        }
        Ok(())
    }

    fn apply_allowance_update(&mut self, allowance_update: &AllowanceValue) -> Result<(), Box<dyn std::error::Error + Send>> {
        match allowance_update {
            AllowanceValue::Insert(key, value) => {
                if let Some(entry) = self.allowance.get_mut(key) {
                    *entry += *value;
                } else {
                    self.allowance.insert(key.clone(), value.clone());
                }
            }
            AllowanceValue::Extend(values) => {
                self.allowance.extend(values.into_iter().cloned());
            }
            AllowanceValue::Remove(key, value) => {
                let mut is_empty = false;
                if let Some(entry) = self.allowance.get_mut(key) {
                    *entry -= *value;
                    is_empty = *entry == U256::from(EthU256::from(0)); 
                }
                if is_empty {
                    self.approvals.remove(key);
                }
            }
            AllowanceValue::Revoke(key) => {
                self.allowance.remove(key);
            }
        }
        Ok(())

    }

    fn apply_status_update(&mut self, status_update: &StatusValue) -> Result<(), Box<dyn std::error::Error + Send>> {
        match status_update {
            StatusValue::Lock => {
                self.status = Status::Locked;
            }
            StatusValue::Unlock => {
                self.status = Status::Free;
            }
            StatusValue::Reverse => {
                match &self.status {
                    Status::Locked => self.status = Status::Free,
                    Status::Free => self.status = Status::Locked,
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TokenField {
    ProgramId,
    OwnerId,
    Balance,
    Metadata,
    TokenIds,
    Allowance,
    Approvals,
    Data,
    Status,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TokenFieldValue {
    Balance(BalanceValue),
    Metadata(MetadataValue),
    TokenIds(TokenIdValue),
    Allowance(AllowanceValue),
    Approvals(ApprovalsValue),
    Data(DataValue),
    Status(StatusValue),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BalanceValue {
    Credit(U256),
    Debit(U256),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetadataValue {
    ReplaceAll(Metadata),
    ReplaceSlice(usize, usize, Vec<u8>),
    ReplaceByte(usize, u8),
    Extend(Metadata),
    Push(u8),
    Pop,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TokenIdValue {
    Push(U256),
    Extend(Vec<U256>),
    Insert(usize, U256),
    Pop,
    Remove(U256),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AllowanceValue {
    Insert(Address, U256),
    Extend(Vec<(Address, U256)>),
    Remove(Address, U256),
    Revoke(Address),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApprovalsValue {
    Insert(Address, Vec<U256>),
    Extend(Vec<(Address, Vec<U256>)>),
    Remove(Address, Vec<U256>),
    Revoke(Address),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataValue {
    ReplaceAll(ArbitraryData),
    ReplaceSlice(usize, usize, ArbitraryData),
    ReplaceByte(usize, u8),
    Extend(ArbitraryData),
    Push(u8),
    Pop,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatusValue {
    Reverse,
    Lock,
    Unlock,
}

impl Token {
    pub fn program_id(&self) -> Address {
        self.program_id.clone()
    }

    pub fn owner_id(&self) -> Address {
        self.owner_id.clone()

    } 

    pub fn balance(&self) -> U256 {
        self.balance.clone()

    }

    pub fn metadata(&self) -> Metadata {
        self.metadata.clone()

    }

    pub fn token_ids(&self) -> Vec<U256> {
        self.token_ids.clone()

    }

    pub fn allowance(&self) -> BTreeMap<Address, U256> {
        self.allowance.clone()

    }

    pub fn approvals(&self) -> BTreeMap<Address, Vec<U256>> {
        self.approvals.clone()

    }

    pub fn data(&self) -> ArbitraryData {
        self.data.clone()

    }

    pub fn status(&self) -> Status {
        self.status.clone()

    }

    pub fn update_balance(&mut self, receive: U256, send: U256) {
        self.balance += receive;
        self.balance -= send;
    }

}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub enum Status {
    Locked,
    Free,
}

impl AddAssign for Token {
    fn add_assign(&mut self, rhs: Self) {
        let new_balance = EthU256::from(self.balance) + EthU256::from(rhs.balance());
        self.balance = new_balance.into();
    }
}

impl SubAssign for Token {
    fn sub_assign(&mut self, rhs: Self) {
        let new_balance: EthU256 = EthU256::from(self.balance) - EthU256::from(rhs.balance());
        self.balance = new_balance.into();
    }
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenWitness {
    user: Address,
    token: Address,
    init: Token,
    transactions: TransactionGraph,
    finalized: Box<Token>,
    sig: RecoverableSignature,
    version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransactionGraph {
    transactions: BTreeMap<[u8; 32], GraphEntry>
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphEntry {
    transaction: Transaction,
    dependencies: Vec<[u8; 32]> 
}
