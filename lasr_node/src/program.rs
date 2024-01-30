
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;
use crate::account::Address;
use crate::certificate::{Certificate, RecoverableSignature};

#[derive(Clone,Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)] 
pub struct Abi;

#[derive(Clone,Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)] 
pub struct ContractHash([u8; 32]);

#[derive(Clone,Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)] 
pub struct StaticTopicHash([u8; 32]);

#[derive(Clone,Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)] 
pub struct ConstantTopicHash([u8; 32]);

#[derive(Clone,Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)] 
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
    Custom(Vec<u8>)
}


#[derive(Clone,Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)] 
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
    Custom(Vec<u8>)
}

#[derive(Clone,Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)] 
pub struct ContractBlob {
    owner_sig: RecoverableSignature,
    hash: ContractHash,
    abi: Abi,
    address: Address,
    registeration_certificate: Certificate,
    statics: BTreeMap<StaticTopicHash, StaticValue>,
    constants: BTreeMap<ConstantTopicHash, ConstantValue>
}
