use serde::{Serialize, Deserialize};
use ethereum_types::U256;
use std::collections::BTreeMap;
use std::ops::{AddAssign, SubAssign};

use crate::Address;

/// Represents a generic data container.
///
/// This structure is used to store arbitrary data as a vector of bytes (`Vec<u8>`).
/// It provides a default, cloneable, serializable, and debuggable interface. It is
/// typically used for storing data that doesn't have a fixed format or structure.
#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
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

/// Represents metadata as a byte vector.
///
/// This structure is designed to encapsulate metadata, stored as a vector of bytes.
/// It supports cloning, serialization, and debugging. The metadata can be of any
/// form that fits into a byte array, making it a flexible container.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub enum TokenType {
    Fungible,
    NonFungible,
    Data
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub enum Status {
    Locked,
    Free,
}

impl AddAssign for Token {
    fn add_assign(&mut self, rhs: Self) {
        self.balance += rhs.balance();
    }
}

impl SubAssign for Token {
    fn sub_assign(&mut self, rhs: Self) {
        self.balance -= rhs.balance();
    }
}

