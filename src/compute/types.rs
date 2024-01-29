use std::{collections::{HashMap, BTreeMap, hash_map::DefaultHasher}, hash::{Hash, Hasher}};
use crate::{Address, TokenField, Transaction, Certificate, TokenWitness, TokenFieldValue, TransactionFields, Namespace, ProgramField, ProgramFieldValue, Account};
use schemars::JsonSchema;
use serde::{Serialize, Deserialize};
use serde_json::{Map, Value};

/// This file contains types the protocol uses to prepare data, structure it 
/// and call out to a particular compute payload.

/// The inputs type for a contract call
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Inputs {
    pub version: i32,
    pub account_info: Option<Account>,
    pub transaction: Transaction,
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
    This,
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
    pub fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
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

    pub fn program_namespace(&self) -> &AddressOrNamespace {
        &self.program_namespace
    }

    pub fn program_id(&self) -> &AddressOrNamespace {
        &self.program_id
    }

    pub fn program_owner(&self) -> &Address {
        &self.program_owner
    }

    pub fn total_supply(&self) -> &crate::U256 {
        &self.total_supply
    }

    pub fn initialized_supply(&self) -> &crate::U256 {
        &self.initialized_supply
    }

    pub(crate) fn distribution(&self) -> &Vec<TokenDistribution> {
        &self.distribution
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenDistribution {
    program_id: AddressOrNamespace,
    to: AddressOrNamespace,
    amount: Option<crate::U256>,
    token_ids: Vec<crate::U256>,
    update_fields: Vec<TokenUpdateField>
}
impl TokenDistribution { 
    pub fn program_id(&self) -> &AddressOrNamespace {
        &self.program_id
    }

    pub fn to(&self) -> &AddressOrNamespace {
        &self.to
    }

    pub fn amount(&self) -> &Option<crate::U256> {
        &self.amount
    }

    pub fn token_ids(&self) -> &Vec<crate::U256> {
        &self.token_ids
    }

    pub fn update_fields(&self) -> &Vec<TokenUpdateField> {
        &self.update_fields
    }
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

impl TokenUpdateField {
    pub fn field(&self) -> &TokenField {
        &self.field
    } 

    pub(crate) fn value(&self) -> &TokenFieldValue {
        &self.value
    }
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
    
    pub fn field(&self) -> &ProgramField {
        &self.field
    }

    pub fn value(&self) -> &ProgramFieldValue {
        &self.value
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

    pub fn updates(&self) -> &Vec<TokenOrProgramUpdate> {
        &self.updates
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TokenUpdate {
    account: AddressOrNamespace,
    token: AddressOrNamespace,
    updates: Vec<TokenUpdateField>
}

impl TokenUpdate {
    pub fn account(&self) -> &AddressOrNamespace {
        &self.account
    }

    pub fn token(&self) -> &AddressOrNamespace {
        &self.token
    }

    pub fn updates(&self) -> &Vec<TokenUpdateField> {
        &self.updates
    }
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

    pub fn account(&self) -> &AddressOrNamespace {
        &self.account
    }

    pub fn updates(&self) -> &Vec<ProgramUpdateField> {
        &self.updates
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TransferInstruction {
    token: Address,
    from: AddressOrNamespace,
    to: AddressOrNamespace,
    amount: Option<crate::U256>,
    ids: Vec<crate::U256>
}

impl TransferInstruction {
    pub(crate) fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
        vec![self.from.clone(), self.to.clone()]
    }

    pub fn token(&self) -> &Address {
        &self.token
    }

    pub fn from(&self) -> &AddressOrNamespace {
        &self.from
    }

    pub fn to(&self) -> &AddressOrNamespace {
        &self.to
    }

    pub fn amount(&self) -> &Option<crate::U256> {
        &self.amount
    }

    pub fn ids(&self) -> &Vec<crate::U256> {
        &self.ids
    }

    pub fn replace_this_with_to(&mut self, transaction: &Transaction, _this: &AddressOrNamespace, field: &str) -> Result<(), std::io::Error> {
        match field {
            "from" => {
                self.from = AddressOrNamespace::Address(transaction.to());
            }
            "to" => {
                self.to = AddressOrNamespace::Address(transaction.to());
            }
            _ => {
                return Err(
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "received an invalid string when calling `replace_this_with_to`"
                    )
                )
            }
        }
        Ok(())
    }
}

impl TransferInstruction {
    pub fn new(
        token: Address,
        from: AddressOrNamespace,
        to: AddressOrNamespace,
        amount: Option<crate::U256>,
        ids: Vec<crate::U256>
    ) -> Self {
        Self { token, from, to, amount, ids }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BurnInstruction {
    caller: Address,
    program_id: AddressOrNamespace,
    token: Address,
    from: AddressOrNamespace,
    amount: Option<crate::U256>,
    token_ids: Vec<crate::U256>
}

impl BurnInstruction {
    pub(crate) fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
        vec![self.from.clone()]
    }

    pub fn caller(&self) -> &Address {
        &self.caller
    }

    pub fn program_id(&self) -> &AddressOrNamespace {
        &self.program_id
    }
    
    pub fn token(&self) -> &Address {
        &self.token
    }

    pub fn from(&self) -> &AddressOrNamespace {
        &self.from
    }

    pub fn amount(&self) -> &Option<crate::U256> {
        &self.amount
    }

    pub fn token_ids(&self) -> &Vec<crate::U256> {
        &self.token_ids
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
            Self::Log(_log) => vec![] 
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
