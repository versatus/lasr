use serde::{Serialize, Deserialize};
use ethereum_types::U256 as EthU256;
use std::collections::BTreeMap;
use std::fmt::{Display, Debug};
use std::ops::{AddAssign, SubAssign};
use schemars::JsonSchema;

use crate::{Address, RecoverableSignature, Transaction};

pub const TOKEN_WITNESS_VERSION: &'static str = "0.1.0";

#[derive(Copy, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct U256(pub [u64; 4]);

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
#[derive(Clone, Default, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct ArbitraryData(Vec<u8>);

impl ArbitraryData {
    pub fn new() -> Self {
        Self(vec![])
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
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct Metadata(Vec<u8>);

impl Metadata {
    pub fn new() -> Self {
        Self(vec![])
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
    approvals: BTreeMap<Address, U256>,
    data: ArbitraryData,
    status: Status,
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
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApprovalsValue {
    Insert(Address, Option<U256>),
    Extend(Vec<(Address, Option<U256>)>),
    Remove(Address, Option<U256>),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataValue {
    ReplaceAll(ArbitraryData),
    ReplaceSlice(usize, usize, Vec<u8>),
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

    pub fn approvals(&self) -> BTreeMap<Address, U256> {
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
