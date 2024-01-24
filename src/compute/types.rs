use std::{collections::{HashMap, BTreeMap, hash_map::DefaultHasher}, hash::{Hash, Hasher}};
use crate::{Address, TokenField, Transaction, Certificate, TokenWitness, TokenFieldValue, TransactionFields, ProgramAccount, Namespace, ProgramField, ProgramFieldValue};
use schemars::JsonSchema;
use serde::{Serialize, Deserialize};
use serde_json::{Map, Value};
use std::str::FromStr;

/// This file contains types the protocol uses to prepare data, structure it 
/// and call out to a particular compute payload.

/// The inputs type for a contract call
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Inputs {
    pub version: i32,
    pub account_info: Option<ProgramAccount>,
    pub op: String,
    pub inputs: String,
}

/// The pre-requisite instructions for a contract call 
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)] 
pub struct ParamPreRequisite {
    pre_requisites: PreRequisite,
    outputs: Vec<(usize, OpParams)>,
}

/// The structure returned by a program/call transaction.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Outputs {
    inputs: Inputs,
    instructions: Vec<Instruction>,
}

impl Outputs {
    pub fn new(inputs: Inputs, instructions: Vec<Instruction>) -> Self {
        Self { inputs, instructions }
    }
    
    pub fn instructions(&self) -> &Vec<Instruction> {
        &self.instructions
    }
}

/// This type is constructed from the combination of the original transaction,
/// the constructed inputs, all outputs from contract call, and any 
/// pre-requisite contract call, witnesses, and an optional certificate
/// if the transaction results have been certified.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CallTransactionResult {
    transaction: Transaction,
    inputs: Inputs,
    outputs: Vec<Outputs>,
    certificate: Option<Certificate>,
    witnesses: Vec<TokenWitness>
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum PreRequisite {
    Call(CallParams),
    Unlock(AddressPair),
    Read(ReadParams), 
    Lock(AddressPair),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CallParams {
    pub calling_program: Address,
    pub original_caller: Address,
    pub program_id: Address,
    pub inputs: Inputs,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AddressPair {
    pub account_address: Address,
    pub token_address: Address,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadParams {
    pub items: Vec<(Address, Address, TokenField)>,
    pub contract_blobs: Vec<Address>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AddressOrNamespace {
    Address(Address),
    Namespace(Namespace),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CreateInstruction {
    program_namespace: AddressOrNamespace,
    program_id: AddressOrNamespace,
    program_owner: Address,
    total_supply: crate::U256,
    initialized_supply: crate::U256,
    distribution: Vec<TokenDistribution>
}

impl CreateInstruction {
    pub(crate) fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
        let mut accounts_involved = vec![
            self.program_namespace.clone(),
            self.program_id.clone(),
            AddressOrNamespace::Address(self.program_owner.clone()),
        ];

        for dist in &self.distribution {
            accounts_involved.push(dist.to.clone());
        }

        accounts_involved
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenDistribution {
    to: AddressOrNamespace,
    amount: crate::U256,
    update_fields: Vec<TokenOrProgramUpdateField>
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TokenOrProgramUpdateField {
    TokenUpdateField(TokenUpdateField),
    ProgramUpdateField(ProgramUpdateField)
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum TokenOrProgramUpdate {
    TokenUpdate(TokenUpdate),
    ProgramUpdate(ProgramUpdate),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenUpdateField {
    field: TokenField,
    value: TokenFieldValue
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProgramUpdateField {
    field: ProgramField,
    value: ProgramFieldValue 
}

impl ProgramUpdateField {
    pub fn new(field: ProgramField, value: ProgramFieldValue) -> Self {
        Self { field, value }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct UpdateInstruction {
    updates: Vec<TokenOrProgramUpdate>
}

impl UpdateInstruction {
    pub fn new(updates: Vec<TokenOrProgramUpdate>) -> Self {
        Self { updates }
    }

    pub(crate) fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
        let mut accounts_involved = Vec::new();
        for update in &self.updates {
            match update {
                TokenOrProgramUpdate::TokenUpdate(token_update) => accounts_involved.push(token_update.account.clone()),
                TokenOrProgramUpdate::ProgramUpdate(program_update) => accounts_involved.push(program_update.account.clone()),
            }
        }
        accounts_involved
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TokenUpdate {
    account: AddressOrNamespace,
    token: AddressOrNamespace,
    updates: Vec<TokenUpdateField>
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProgramUpdate {
    account: AddressOrNamespace,
    updates: Vec<ProgramUpdateField>
}

impl ProgramUpdate {
    pub fn new(account: AddressOrNamespace, updates: Vec<ProgramUpdateField>) -> Self {
        Self { account, updates }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TransferInstruction {
    token_namespace: AddressOrNamespace,
    from: AddressOrNamespace,
    to: AddressOrNamespace,
    amount: Option<crate::U256>,
    token_ids: Vec<crate::U256>
}

impl TransferInstruction {
    pub(crate) fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
        vec![self.from.clone(), self.to.clone()]
    }
}

impl TransferInstruction {
    pub fn new(
        token_namespace: AddressOrNamespace,
        from: AddressOrNamespace,
        to: AddressOrNamespace,
        amount: Option<crate::U256>,
        token_ids: Vec<crate::U256>
    ) -> Self {
        Self { token_namespace, from, to, amount, token_ids }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BurnInstruction {
    token_namespace: AddressOrNamespace,
    owner: Address,
    amount: Option<crate::U256>,
    token_ids: Vec<crate::U256>
}

impl BurnInstruction {
    pub(crate) fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
        vec![AddressOrNamespace::Address(self.owner.clone())]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LogInstruction(pub ContractLogType);

/// An enum representing the instructions that a program can return 
/// to the protocol. Also represent types that tell the protocol what  
/// the pre-requisites of a given function call are.
/// All enabled languages have equivalent types
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum Instruction {
    /// The return type created by the construction method of a contract 
    Create(CreateInstruction),
    /// Tells the protocol to update a field, should almost never be used  
    /// to add balance to a token or add a token id (for Non-fungible or Data tokens)
    /// should prrimarily be used to update approvals, allowances, metadata, arbitrary data
    /// etc. Transfer or burn should be used to add/subtract balance. Lock/Unlock should be used 
    /// to lock value
    Update(UpdateInstruction),
    /// Tells the protocol to subtract balance of one address/token pair and add to different
    /// address 
    Transfer(TransferInstruction), 
    /// Tells the protocol to burn a token (amount or id for NFT/Data tokens)
    Burn(BurnInstruction),
    /// Tells the protocol to log something
    Log(LogInstruction) 
}

impl Instruction {
    pub fn get_accounts_involved(&self) -> Vec<AddressOrNamespace> {
        match self {
            Self::Create(create) => create.accounts_involved(),
            Self::Update(update) => update.accounts_involved(),
            Self::Transfer(transfer) => transfer.accounts_involved(),
            Self::Burn(burn) => burn.accounts_involved(),
            Self::Log(log) => vec![] 
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum ContractLogType {
    Info(String),
    Error(String),
    Warn(String),
    Debug(String)
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum OpParamTypes {
    Address,
    String,
    Tuple,
    FixedArray(usize),
    Array,
    FixedBytes(usize),
    Bool,
    Uint,
    Int,
    BigUint,
    BigInt,
    GiantUint,
    GiantInt,
    Mapping,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProgramSchema {
    pub contract: Contract
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Contract {
    pub name: String,
    pub version: String,
    pub language: String,
    pub ops: HashMap<String, Ops>
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Ops {
    pub description: String,
    pub help: Option<String>,
    pub signature: OpSignature,
    pub required: Option<Vec<Required>> 
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OpSignature {
    #[serde(flatten)]
    pub op_signature: HashMap<String, String>,
    pub params_mapping: HashMap<String, ParamSource>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ParamSource {
    pub source: String,
    pub field: Option<String>,
    pub key: Option<String>,
    pub position: usize
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "details")]
pub enum Required {
    Call(CallMap),
    Read(ReadMap),
    Lock(LockPair),
    Unlock(LockPair),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CallMap {
    calling_program: TransactionFields,
    original_caller: TransactionFields,
    program_id: String, 
    op: String,
    inputs: String, 
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadMap {
    items: Vec<(String, String, String)>, 
    contract_blobs: Option<Vec<String>> 
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LockPair {
    account: String,
    token: String
}

impl ProgramSchema {
    pub fn contract(&self) -> &Contract {
        &self.contract
    }

    pub fn name(&self) -> String {
        self.contract().name.clone()
    }

    pub fn version(&self) -> String {
        self.contract().version.clone()
    }

    pub fn language(&self) -> String {
        self.contract().language.clone()
    }

    pub fn ops(&self) -> &HashMap<String, Ops> {
        &self.contract().ops
    }

    pub fn get_op(&self, name: &str) -> Option<&Ops> {
        self.ops().get(name)
    }

    pub(crate) fn get_prerequisites(&self, op: &str) -> std::io::Result<Vec<Required>> {
        let (_key, value) = self.contract.ops.get_key_value(op).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "Invalid `op`: Not defined in schema")
        )?;
        
        if let Some(reqs) = &value.required {
            return Ok(reqs.clone())
        }

        Ok(Vec::new())
    }

    #[allow(unused)]
    pub(crate) fn parse_op_inputs(&self, op: &str, json_inputs: &str) -> std::io::Result<()> {
        let (_key, value) = self.contract.ops.get_key_value(op).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "Invalid `op`: Not defined in schema")
        )?;
        
        let mut json: Map<String, Value> = serde_json::from_str(json_inputs)?;

        for (k, v) in &value.signature.params_mapping {
            dbg!(&k, &v);
        }

        Ok(())
    }
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
    use serde_json::json;

    use super::*;
    use std::fs;
    use std::path::Path;
    use std::str::FromStr;

    #[test]
    fn test_config_parsing() {
        let config_path = Path::new("./examples/escrow/schema.toml");
        let toml_str = fs::read_to_string(config_path).expect("Failed to read schema.toml");
        let schema: ProgramSchema = toml::from_str(&toml_str).expect("Failed to parse config.toml");
        let deposit_inputs = json!({
            "depositor": "0xabcdef012345678909876543210fedcba0123456",
            "redeemer": "0xfedcba09876532101234567890abcdef09876543",
            "deposit_token_address": "0x012345678909876543210abcdeffedcba0123456",
            "deposit": 12345,
            "conditions": [
                { 
                    "condition": "ContractTransfer",
                    "program_id": "0x1234567890abcdef1234567890abcdef12345678",
                    "token_ids": [ 5238 ],
                    "from": "redeemer",
                    "to": "depositor",
                }
            ]
        }); 

        let _ = schema.parse_op_inputs("deposit", &deposit_inputs.to_string());
    }

    #[test]
    fn test_parse_create_instruction() {
        let program_namespace = AddressOrNamespace::Namespace(Namespace("lasr.contract.FakeToken".to_string()));
        let program_id = AddressOrNamespace::Namespace(Namespace("lasr.contract.FakeToken.ProgramAccount".to_string()));
        let program_owner = Address::from_str("0x1234567890ABCDEF1234567890ABCDEF12345678").unwrap();
        let total_supply = crate::U256::from(ethereum_types::U256::from(10000000000000000000000000000 as u128));
        let initialized_supply = crate::U256::from(ethereum_types::U256::from(10000000000000000000000 as u128));
        let random_addresses = random_addresses();
        let (mut distribution, remaining) = {
            let mut total = crate::U256::from(ethereum_types::U256::from(0));
            let mut dist = Vec::new();
            for address in random_addresses {
                let amount = crate::U256::from(ethereum_types::U256::from(1000000 as u128));
                dist.push(TokenDistribution {
                    to: AddressOrNamespace::Address(address.clone()),
                    amount,
                    update_fields: vec![]
                });

                total += amount;
            }
            let remaining = initialized_supply - total;
            (dist, remaining)
        };

        distribution.push(
            TokenDistribution {
                to: AddressOrNamespace::Address(program_owner.clone()),
                amount: remaining,
                update_fields: vec![]
            }
        );

        let inst = CreateInstruction {
            program_namespace,
            program_id,
            program_owner,
            total_supply,
            initialized_supply,
            distribution
        };

        println!("{}", serde_json::to_string_pretty(&inst).unwrap());

        let create_instruction_json = json!({
          "Create": {                                                                                                                  
            "program_namespace": {                                                                                                     
              "Namespace": "lasr.contract.FakeToken"                                                                                   
            },                                                                                                                         
            "program_id": {                                                                                                            
              "Namespace": "lasr.contract.FakeToken.ProgramAccount"                                                                    
            },                                                                                                                         
            "program_owner": "0x1234567890abcdef1234567890abcdef12345678",                                                             
            "total_supply": "3e2502611000000000000000204fce5e00000000000000000000000000000000",                                        
            "initialized_supply": "19e0c9bab2400000000000000000021e00000000000000000000000000000000",                                  
            "distribution": [                                                                                                          
              {                                                                                                                        
                "to": {                                                                                                                
                  "Address": "0xa1b2c3d4e5f67890123456789abcdef012345678"                                                              
                },                                                                                                                     
                "amount": "00000000000f4240000000000000000000000000000000000000000000000000",                                          
                "update_fields": []                                                                                                    
              },                                                                                                                       
              {                                                                                                                        
                "to": {                                                                                                                
                  "Address": "0xb2c3d4e5f6789012a1b3456789abcdef01234567"                                                              
                },                                                                                                                     
                "amount": "00000000000f4240000000000000000000000000000000000000000000000000",                                          
                "update_fields": []
              },
              {
                "to": {
                  "Address": "0xc3d4e5f6789012a1b2c456789abcdef012345678"
                },                                                                                                                     
                "amount": "00000000000f4240000000000000000000000000000000000000000000000000",                                          
                "update_fields": []
              },
              {
                "to": {
                  "Address": "0xd4e5f6789012a1b2c3d56789abcdef0123456789"
                },
                "amount": "00000000000f4240000000000000000000000000000000000000000000000000",
                "update_fields": []
              },
              {
                "to": {
                  "Address": "0xe5f6789012a1b2c3d4e6789abcdef01234567890"
                },
                "amount": "00000000000f4240000000000000000000000000000000000000000000000000",
                "update_fields": []
              },
              {
                "to": {
                  "Address": "0xf6789012a1b2c3d4e5f789abcdef012345678901"
                },
                "amount": "00000000000f4240000000000000000000000000000000000000000000000000",
                "update_fields": []
              },
              {
                "to": {
                  "Address": "0x789012a1b2c3d4e5f6789abcdef0123456789012"
                },
                "amount": "00000000000f4240000000000000000000000000000000000000000000000000",
                        "update_fields": []
              },
              {
                "to": {
                  "Address": "0x89012a1b2c3d4e5f678901abcdef012345678901"
                },
                "amount": "00000000000f4240000000000000000000000000000000000000000000000000",
                "update_fields": []
              },
              {
                "to": {
                  "Address": "0x9012a1b2c3d4e5f6789012abcdef012345678901"
                },
                "amount": "00000000000f4240000000000000000000000000000000000000000000000000",
                "update_fields": []
              },
              {
                "to": {
                  "Address": "0x012a1b2c3d4e5f67890123abcdef012345678901"
                },
                "amount": "00000000000f4240000000000000000000000000000000000000000000000000",
                "update_fields": []
              },
              {
                "to": {
                  "Address": "0x1234567890abcdef1234567890abcdef12345678"
                },
                "amount": "19e0c9bab1a76980000000000000021e00000000000000000000000000000000",
                "update_fields": []
              }
            ]
          }
        }).to_string();

        let create_instruction: Instruction = serde_json::from_str(&create_instruction_json).unwrap();
        match create_instruction {
            Instruction::Create(create) => {
                assert_eq!(create, inst);
            } 
           _ => {
                panic!("Parsed the json into the wrong instruction type")
            }
        }
    }

    #[test]
    fn test_parse_update_instruction() {

//pub struct UpdateInstruction {
//    updates: Vec<TokenOrProgramUpdate>
//
//}
//
//pub enum TokenOrProgramUpdate {
//    TokenUpdate(TokenUpdate),
//    ProgramUpdate(ProgramUpdate),
//}
//
//#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
//pub struct TokenUpdate {
//    account: AddressOrNamespace,
//    token: AddressOrNamespace,
//    updates: Vec<TokenUpdateField>
//}

        let token_update_updates = vec![
            TokenUpdateField {
                field: TokenField::Data,
                value: TokenFieldValue::Data(
                crate::DataValue::Push(8)
                )
            }
        ];
    }

    #[test]
    fn test_parse_transfer_instruction() {
    }

    #[test]
    fn test_parse_burn_instruction() {
    }
}

fn random_addresses() -> Vec<Address> {
    vec![
        Address::from_str("0xA1B2C3D4E5F67890123456789ABCDEF012345678").unwrap(),
        Address::from_str("0xB2C3D4E5F6789012A1B3456789ABCDEF01234567").unwrap(),
        Address::from_str("0xC3D4E5F6789012A1B2C456789ABCDEF012345678").unwrap(),
        Address::from_str("0xD4E5F6789012A1B2C3D56789ABCDEF0123456789").unwrap(),
        Address::from_str("0xE5F6789012A1B2C3D4E6789ABCDEF01234567890").unwrap(),
        Address::from_str("0xF6789012A1B2C3D4E5F789ABCDEF012345678901").unwrap(),
        Address::from_str("0x789012A1B2C3D4E5F6789ABCDEF0123456789012").unwrap(),
        Address::from_str("0x89012A1B2C3D4E5F678901ABCDEF012345678901").unwrap(),
        Address::from_str("0x9012A1B2C3D4E5F6789012ABCDEF012345678901").unwrap(),
        Address::from_str("0x012A1B2C3D4E5F67890123ABCDEF012345678901").unwrap(),
    ]
}
