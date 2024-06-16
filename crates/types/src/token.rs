use derive_builder::Builder;
use ethereum_types::U256 as EthU256;
use hex::FromHexError;
use schemars::JsonSchema;
use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt::{Debug, Display};
use std::ops::{AddAssign, SubAssign};
use uint::construct_uint;

use crate::{Address, RecoverableSignature, Transaction};

pub const TOKEN_WITNESS_VERSION: &str = "0.1.0";

construct_uint! {
    /// 256-bit unsigned integer.
    #[derive(JsonSchema)]
    #[serde(rename_all = "camelCase")]
    pub struct U256(4);
}

impl Serialize for U256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_str = format!("0x{:064x}", self);
        serializer.serialize_str(&hex_str)
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
        formatter.write_str("a 64-character hex string or [u64; 4] array as a string")
    }

    fn visit_str<E>(self, v: &str) -> Result<U256, E>
    where
        E: serde::de::Error,
    {
        let value = if let Some(value) = v.strip_prefix("0x") {
            value
        } else {
            v
        };
        if value.starts_with('[') && v.ends_with(']') {
            // Parse as a byte array
            let nums_str = &v[1..v.len() - 1];
            let nums: Vec<u64> = nums_str
                .split(',')
                .map(str::trim)
                .map(|s| s.parse::<u64>().map_err(E::custom))
                .collect::<Result<Vec<u64>, E>>()?;

            if nums.len() == 4 {
                let arr = [nums[0], nums[1], nums[2], nums[3]];
                Ok(U256(arr))
            } else {
                Err(E::custom("Array does not have 4 elements"))
            }
        } else if value.len() == 64 {
            // Parse as a hex string
            let decoded = hex::decode(value).map_err(E::custom)?;
            if decoded.len() == 32 {
                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&decoded[..]);
                Ok(U256::from(&bytes))
            } else {
                Err(E::custom("decoded result is improper length"))
            }
        } else {
            Err(E::custom("Invalid format for U256"))
        }
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
        *value
    }
}

impl From<&mut U256> for EthU256 {
    fn from(value: &mut U256) -> Self {
        EthU256(value.0)
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
        EthU256(value.0)
    }
}

impl From<&EthU256> for U256 {
    fn from(value: &EthU256) -> Self {
        U256(value.0)
    }
}

/// Represents a generic data container.
///
/// This structure is used to store arbitrary data as a vector of bytes (`Vec<u8>`).
/// It provides a default, cloneable, serializable, and debuggable interface. It is
/// typically used for storing data that doesn't have a fixed format or structure.
#[derive(
    Clone, Default, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct ArbitraryData(BTreeMap<String, String>);

impl Display for ArbitraryData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#?}", self.0)
    }
}

impl ArbitraryData {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.0.insert(key, value);
    }

    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.0.remove(key)
    }

    pub fn extend(&mut self, iter: BTreeMap<String, String>) {
        self.0.extend(iter);
    }

    pub fn inner(&self) -> &BTreeMap<String, String> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut BTreeMap<String, String> {
        &mut self.0
    }

    pub fn to_hex(&self) -> Result<String, serde_json::Error> {
        let bytes = self.to_bytes()?;
        Ok(hex::encode(bytes))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&self)
    }

    pub fn from_hex(hex: &str) -> Result<Self, FromHexError> {
        Ok(serde_json::from_slice(&hex::decode(hex)?)
            .map_err(|_| FromHexError::InvalidStringLength))?
    }
}

/// Represents metadata as a byte vector.
///
/// This structure is designed to encapsulate metadata, stored as a vector of bytes.
/// It supports cloning, serialization, and debugging. The metadata can be of any
/// form that fits into a byte array, making it a flexible container.
#[derive(
    Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct Metadata(BTreeMap<String, String>);

impl Metadata {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.0.get(key)
    }

    pub fn extend(&mut self, iter: BTreeMap<String, String>) {
        self.0.extend(iter);
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.0.insert(key, value);
    }

    pub fn remove(&mut self, key: &String) -> Option<String> {
        self.0.remove(key)
    }

    pub fn inner_mut(&mut self) -> &mut BTreeMap<String, String> {
        &mut self.0
    }

    pub fn inner(&self) -> &BTreeMap<String, String> {
        &self.0
    }

    pub fn to_hex(&self) -> Result<String, Box<bincode::ErrorKind>> {
        let bytes = self.to_bytes()?;
        Ok(hex::encode(bytes))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, Box<bincode::ErrorKind>> {
        bincode::serialize(&self)
    }

    pub fn from_hex(hex: &str) -> Result<Self, FromHexError> {
        Ok(bincode::deserialize(&hex::decode(hex)?).map_err(|_| FromHexError::InvalidStringLength))?
    }
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum TokenType {
    Fungible,
    NonFungible,
    Data,
}

#[derive(
    Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
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
    pub(crate) fn set_metadata(&mut self, metadata: Metadata) {
        self.metadata = metadata;
    }

    pub(crate) fn set_data(&mut self, data: ArbitraryData) {
        self.data = data;
    }

    pub(crate) fn debit(&mut self, amount: &U256) -> Result<(), Box<dyn std::error::Error + Send>> {
        if amount > &self.balance {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "transfer amount exceeds balance".to_string(),
            )));
        }

        self.balance -= *amount;
        Ok(())
    }

    pub(crate) fn credit(
        &mut self,
        amount: &U256,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        self.balance += *amount;
        Ok(())
    }

    pub(crate) fn remove_token_ids(
        &mut self,
        token_ids: &Vec<U256>,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        let positions: Vec<usize> = {
            token_ids
                .iter()
                .filter_map(|nft| self.token_ids.iter().position(|i| i == nft))
                .collect()
        };

        if positions.len() != token_ids.len() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "one or more of the token ids is not owned by the from account",
            )));
        }

        self.token_ids.retain(|i| !token_ids.contains(i));

        Ok(())
    }

    pub(crate) fn add_token_ids(
        &mut self,
        token_ids: &Vec<U256>,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        self.token_ids.extend(token_ids);
        Ok(())
    }

    pub(crate) fn apply_token_update_field_values(
        &mut self,
        token_update_value: &TokenFieldValue,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        log::warn!("applying TokenFieldValue: {:?}", token_update_value);
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

    fn apply_data_update(
        &mut self,
        data_update: &DataValue,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        log::warn!("applying data update: {:?}", data_update);
        match data_update {
            DataValue::Insert(key, value) => {
                self.data.insert(key.clone(), value.clone());
            }
            DataValue::Extend(iter) => {
                self.data.inner_mut().extend(iter.clone());
            }
            DataValue::Remove(key) => {
                self.data.remove(key);
            }
        }

        Ok(())
    }

    fn apply_metadata_update(
        &mut self,
        metadata_update: &MetadataValue,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        log::warn!("applying metadata update: {:?}", metadata_update);
        match metadata_update {
            MetadataValue::Insert(key, value) => {
                self.metadata.inner_mut().insert(key.clone(), value.clone());
            }
            MetadataValue::Extend(iter) => {
                log::info!("found metadata_extend update, {:?} applying", &iter);
                self.metadata.inner_mut().extend(iter.clone());
            }
            MetadataValue::Remove(key) => {
                self.metadata.inner_mut().remove(key);
            }
        }

        Ok(())
    }

    fn apply_approvals_update(
        &mut self,
        approvals_update: &ApprovalsValue,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        log::warn!("applying approvals update: {:?}", approvals_update);
        match approvals_update {
            ApprovalsValue::Insert(key, value) => {
                if let Some(entry) = self.approvals.get_mut(key) {
                    entry.extend(value.clone());
                } else {
                    self.approvals.insert(*key, value.clone());
                }
            }
            ApprovalsValue::Extend(values) => {
                self.approvals.extend(values.iter().cloned());
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

    fn apply_allowance_update(
        &mut self,
        allowance_update: &AllowanceValue,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        log::warn!("applying approvals update: {:?}", allowance_update);
        match allowance_update {
            AllowanceValue::Insert(key, value) => {
                if let Some(entry) = self.allowance.get_mut(key) {
                    *entry += *value;
                } else {
                    self.allowance.insert(*key, *value);
                }
            }
            AllowanceValue::Extend(values) => {
                self.allowance.extend(values.iter().cloned());
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

    fn apply_status_update(
        &mut self,
        status_update: &StatusValue,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        log::warn!("applying status update: {:?}", status_update);
        match status_update {
            StatusValue::Lock => {
                self.status = Status::Locked;
            }
            StatusValue::Unlock => {
                self.status = Status::Free;
            }
            StatusValue::Reverse => match &self.status {
                Status::Locked => self.status = Status::Free,
                Status::Free => self.status = Status::Locked,
            },
        }
        Ok(())
    }
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
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

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum TokenFieldValue {
    Balance(BalanceValue),
    Metadata(MetadataValue),
    TokenIds(TokenIdValue),
    Allowance(AllowanceValue),
    Approvals(ApprovalsValue),
    Data(DataValue),
    Status(StatusValue),
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum BalanceValue {
    Credit(U256),
    Debit(U256),
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum MetadataValue {
    Insert(String, String),
    Extend(BTreeMap<String, String>),
    Remove(String),
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum TokenIdValue {
    Push(U256),
    Extend(Vec<U256>),
    Insert(usize, U256),
    Pop,
    Remove(U256),
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum AllowanceValue {
    Insert(Address, U256),
    Extend(Vec<(Address, U256)>),
    Remove(Address, U256),
    Revoke(Address),
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalsValue {
    Insert(Address, Vec<U256>),
    Extend(Vec<(Address, Vec<U256>)>),
    Remove(Address, Vec<U256>),
    Revoke(Address),
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum DataValue {
    Insert(String, String),
    Extend(BTreeMap<String, String>),
    Remove(String),
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum StatusValue {
    Reverse,
    Lock,
    Unlock,
}

impl Token {
    pub fn program_id(&self) -> Address {
        self.program_id
    }

    pub fn owner_id(&self) -> Address {
        self.owner_id
    }

    pub fn balance(&self) -> U256 {
        self.balance
    }

    pub fn balance_mut(&mut self) -> &mut U256 {
        &mut self.balance
    }

    pub fn metadata(&self) -> Metadata {
        self.metadata.clone()
    }

    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    pub fn token_ids(&self) -> Vec<U256> {
        self.token_ids.clone()
    }

    pub fn token_ids_mut(&mut self) -> &mut Vec<U256> {
        &mut self.token_ids
    }

    pub fn allowance(&self) -> BTreeMap<Address, U256> {
        self.allowance.clone()
    }

    pub fn allowance_mut(&mut self) -> &mut BTreeMap<Address, U256> {
        &mut self.allowance
    }

    pub fn approvals(&self) -> BTreeMap<Address, Vec<U256>> {
        self.approvals.clone()
    }

    pub fn approvals_mut(&mut self) -> &mut BTreeMap<Address, Vec<U256>> {
        &mut self.approvals
    }

    pub fn data(&self) -> ArbitraryData {
        self.data.clone()
    }

    pub fn data_mut(&mut self) -> &mut ArbitraryData {
        &mut self.data
    }

    pub fn status(&self) -> Status {
        self.status.clone()
    }

    pub fn status_mut(&mut self) -> &mut Status {
        &mut self.status
    }

    pub fn update_balance(&mut self, receive: U256, send: U256) {
        self.balance += receive;
        self.balance -= send;
    }
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
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

#[derive(
    Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct TokenWitness {
    user: Address,
    token: Address,
    init: Token,
    transactions: TransactionGraph,
    finalized: Box<Token>,
    sig: RecoverableSignature,
    version: String,
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct TransactionGraph {
    transactions: BTreeMap<[u8; 32], GraphEntry>,
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct GraphEntry {
    transaction: Transaction,
    dependencies: Vec<[u8; 32]>,
}
