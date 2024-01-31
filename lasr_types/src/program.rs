use crate::account::Address;
use crate::certificate::{Certificate, RecoverableSignature};
use crate::{AddressOrNamespace, ProgramField, ProgramFieldValue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct Abi;

#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct ContractHash([u8; 32]);

#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct StaticTopicHash([u8; 32]);

#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct ConstantTopicHash([u8; 32]);

#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum StaticValue {
    String(String),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    U256(crate::U256),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    Bool(bool),
    Custom(Vec<u8>),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum ConstantValue {
    String(String),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    U256(crate::U256),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    Bool(bool),
    Custom(Vec<u8>),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct ContractBlob {
    owner_sig: RecoverableSignature,
    hash: ContractHash,
    abi: Abi,
    address: Address,
    registeration_certificate: Certificate,
    statics: BTreeMap<StaticTopicHash, StaticValue>,
    constants: BTreeMap<ConstantTopicHash, ConstantValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProgramUpdate {
    account: AddressOrNamespace,
    updates: Vec<ProgramUpdateField>,
}

impl ProgramUpdate {
    pub fn new(account: AddressOrNamespace, updates: Vec<ProgramUpdateField>) -> Self {
        Self { account, updates }
    }

    pub fn account(&self) -> &AddressOrNamespace {
        &self.account
    }

    pub fn updates(&self) -> &Vec<ProgramUpdateField> {
        &self.updates
    }
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct ProgramUpdateField {
    field: ProgramField,
    value: ProgramFieldValue,
}

impl ProgramUpdateField {
    pub fn new(field: ProgramField, value: ProgramFieldValue) -> Self {
        Self { field, value }
    }

    pub fn field(&self) -> &ProgramField {
        &self.field
    }

    pub fn value(&self) -> &ProgramFieldValue {
        &self.value
    }
}
