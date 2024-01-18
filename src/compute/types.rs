use std::{collections::{HashMap, BTreeMap, hash_map::DefaultHasher}, hash::{Hash, Hasher}};

use crate::{Address, ContractBlob, TokenField, Transaction, Certificate, TokenWitness, TokenFieldValue};
use ethereum_types::U256;
use serde::{Serialize, Deserialize};

/// The inputs type for a contract call
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inputs {
    pub op: String,
    pub inputs: Vec<String>,
}

/// The pre-requisite instructions for a contract call 
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParamPreRequisite {
    pre_requisites: PreRequisite,
    outputs: Vec<(usize, OpParams)>,
}

/// The structure returned by a program/call transaction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outputs {
    instructions: Vec<Instruction>,
}

/// This type is constructed from the combination of the original transaction,
/// the constructed inputs, all outputs from contract call, and any 
/// pre-requisite contract call, witnesses, and an optional certificate
/// if the transaction results have been certified.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallTransactionResult {
    transaction: Transaction,
    inputs: Inputs,
    outputs: Vec<Outputs>,
    certificate: Option<Certificate>,
    witnesses: Vec<TokenWitness>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PreRequisite {
    Call(CallParams),
    Unlock(AddressPair),
    Read(ReadParams), 
    Lock(AddressPair),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallParams {
    pub calling_program: Address,
    pub original_caller: Address,
    pub program_id: Address,
    pub inputs: Inputs,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddressPair {
    pub account_address: Address,
    pub token_address: Address,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadParams {
    pub items: Vec<(Address, Address, TokenField)>,
    pub contract_blobs: Vec<Address>,
}
/// An enum representing the instructions that a program can return 
/// to the protocol. Also represent types that tell the protocol what  
/// the pre-requisites of a given function call are.
/// All enabled languages have equivalent types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Instruction {
    /// The return type created by the construction method of a contract 
    Create {
        program_namespace: Address,
        program_id: Address,
        program_owner: Address,
        total_supply: U256,
        initialized_supply: U256,
        distribution: Vec<(Address, U256, Vec<(TokenField, TokenFieldValue)>)>
    },
    /// Tells the protocol to update a field, should almost never be used  
    /// to add balance to a token or add a token id (for Non-fungible or Data tokens)
    /// should prrimarily be used to update approvals, allowances, metadata, arbitrary data
    /// etc. Transfer or burn should be used to add/subtract balance. Lock/Unlock should be used 
    /// to lock value
    Update {
        // AccountAddress, ProgramId, Vec<TokenField to update, TokenFieldValue to Provide>
        items: Vec<(Address, Address, Vec<(TokenField, TokenFieldValue)>)>,
    },
    /// Tells the protocol to subtract balance of one address/token pair and add to different
    /// address 
    Transfer {
        program_id: Address,
        from: Address,
        to: Address,
        amount: Option<U256>,
        token_ids: Vec<U256>,
    },
    /// Tells the protocol to burn a token (amount or id for NFT/Data tokens)
    Burn {
        program_id: Address,
        owner_address: Address,
        amount: Option<U256>,
        token_ids: Vec<U256>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum OpParams {
    Address([u8; 20]),
    String(String),
    Tuple(Vec<OpParams>),
    FixedArray(Vec<OpParams>, usize),
    Array(Vec<OpParams>),
    FixedBytes(Vec<u8>, usize),
    Bool(bool),
    Byte(u8),
    Uint(u32),
    Int(i32),
    BigUint([u64; 4]),
    BigInt([i64; 4]),
    GiantUint([u64; 8]),
    GiantInt([i64; 8]),
    Mapping(HashMap<String, OpParams>)
}

impl Ord for OpParams {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        let hashed_self = hasher.finish() as u128;
        let mut hasher = DefaultHasher::new();
        other.hash(&mut hasher);
        let hashed_other = hasher.finish() as u128;
        hashed_self.cmp(&hashed_other)
    }
}

impl PartialOrd for OpParams {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for OpParams {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            OpParams::Address(addr) => addr.hash(state),
            OpParams::String(str) => str.hash(state),
            OpParams::Tuple(tup) => tup.hash(state),
            OpParams::FixedArray(arr, size) => { 
                arr.hash(state);
                size.hash(state);
            },
            OpParams::Array(arr) => arr.hash(state),
            OpParams::FixedBytes(arr, size) => {
                arr.hash(state);
                size.hash(state);
            },
            OpParams::Bool(b) => b.hash(state),
            OpParams::Byte(b) => b.hash(state),
            OpParams::Uint(n) => n.hash(state),
            OpParams::Int(i) => i.hash(state),
            OpParams::BigUint(n) => n.hash(state),
            OpParams::BigInt(i) => i.hash(state),
            OpParams::GiantUint(n) => n.hash(state),
            OpParams::GiantInt(i) => i.hash(state),
            OpParams::Mapping(map) => {
                let sorted: BTreeMap<&String, &OpParams> = map.iter().collect();
                sorted.hash(state);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub contract: Contract
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contract {
    pub name: String,
    pub version: String,
    pub language: String,
    pub ops: HashMap<String, Ops>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ops {
    pub description: String,
    pub help: Option<String>,
    pub signature: OpSignature,
    pub required: Option<Vec<Required>> 
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpSignature {
    #[serde(flatten)]
    pub op_signature: HashMap<String, String>,
    pub params_mapping: HashMap<String, ParamSource>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParamSource {
    pub source: String,
    pub field: Option<String>,
    pub key: Option<String>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "details")]
pub enum Required {
    Call(CallMap),
    Read(ReadMap),
    Lock(LockPair),
    Unlock(LockPair),
}

//TODO(asmith) create a enum to represent the types without the inner
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallMap {
    calling_program: String,
    original_caller: String,
    program_id: String, 
    inputs: String, 
}

//TODO(asmith) create a enum to represent the types without the inner
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadMap {
    items: Vec<(String, String, String)>, 
    contract_blobs: Option<Vec<String>> 
}

//TODO(asmith) create a enum to represent the types without the inner
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LockPair {
    account: String,
    token: String
}

pub trait Parameterize {
    fn into_op_params(self) -> Vec<OpParams>;
}

pub trait Parameter {
    type Err;

    fn from_op_param(op_param: OpParams) -> Result<Self, Self::Err>
        where Self: Sized;

    fn into_op_param(self) -> OpParams;
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_config_parsing() {
        let config_path = Path::new("./examples/escrow/config.toml");
        let toml_str = fs::read_to_string(config_path).expect("Failed to read config.toml");
        let config: Config = toml::from_str(&toml_str).expect("Failed to parse config.toml");
        dbg!(config);
    }
}
