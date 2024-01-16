use crate::{Address, ContractBlob, TokenField, Transaction, Certificate, TokenWitness};
use ethereum_types::U256;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inputs {
    pub op: String,
    pub inputs: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outputs {
    instructions: Vec<Instruction>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallTransactionResult {
    transaction: Transaction,
    inputs: Inputs,
    outputs: Outputs,
    certificate: Option<Certificate>,
    witnesses: Vec<TokenWitness>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Instruction {
    Create {
        program_address: Address,
        program_id: Address,
        contract_blob: ContractBlob,
    },
    Read {
        items: Vec<(Address, Address, TokenField)>,
        contract_blobs: Vec<Address>,
    },
    Update {
        items: Vec<(Address, Address, TokenField)>,
    },
    Burn {
        owner_id: Address,
        program_id: Address,
        amount: U256,
    },
    Transfer {
        program: Address,
        from: Address,
        to: Address,
        token_ids: Vec<U256>,
        amount: Vec<U256>
    },
    Call {
        calling_program: Address,
        original_caller: Address,
        program_id: Address,
        inputs: Inputs,
        after: Box<Instruction>
    },
    Lock(Vec<(Address, Address)>),
    Unlock(Vec<(Address, Address)>)
}
