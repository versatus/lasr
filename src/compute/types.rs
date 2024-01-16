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
    outputs: Vec<OpParams>,
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
    Call {
        calling_program: Address,
        original_caller: Address,
        program_id: Address,
        inputs: Inputs,
        next: Box<PreRequisite>
    },
    Read {
        items: Vec<(Address, Address, TokenField)>,
        contract_blobs: Vec<Address>,
        next: Box<PreRequisite>,
    },
}

/// An enum representing the instructions that a program can return 
/// to the protocol. Also represent types that tell the protocol what  
/// the pre-requisites of a given function call are.
/// All enabled languages have equivalent types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Instruction {
    /// The return type created by the constructor of a contract 
    Create {
        program_address: Address,
        program_id: Address,
        contract_blob: Option<ContractBlob>,
    },
    /// Tells the protocol to update a field, should almost never be used  
    /// to add balance to a token or add a token id (for Non-fungible or Data tokens)
    /// should prrimarily be used to update approvals, allowances, metadata, arbitrary data
    /// etc. Transfer or burn should be used to add/subtract balance. Lock/Unlock should be used 
    /// to lock value
    Update {
        items: Vec<(Address, Address, TokenField, TokenFieldValue)>,
    },
    /// Tells the protocol to burn a token (amount or id for NFT/Data tokens)
    Burn {
        owner_id: Address,
        program_id: Address,
        amount: Option<U256>,
        token_id: Option<U256>,
    },
    /// Tells the protocol to subtract balance of one address/token pair and add to different
    /// address 
    Transfer {
        program: Address,
        from: Address,
        to: Address,
        token_ids: Vec<U256>,
        amount: Vec<U256>
    },
    Lock(Vec<(Address, Address)>),
    Unlock(Vec<(Address, Address)>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
