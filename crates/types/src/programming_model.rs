//! This file contains types the protocol uses to prepare data, structure it
//! and call out to a particular compute payload.
use crate::{
    Account, Address, Certificate, Namespace, ProgramField, ProgramFieldValue, TokenField,
    TokenFieldValue, TokenWitness, Transaction, TransactionFields, U256,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{hash_map::DefaultHasher, BTreeMap, HashMap},
    hash::{Hash, Hasher},
    io::ErrorKind,
};

/// The inputs type for a contract call. This is built from a combination of
/// transaction data and pre-requisite data the protocol acquires in accordance
/// with the developers program schema (WARNING: PROGRAM SCHEMAS ARE
/// EXPERIMENTAL AND NOT YET ENABLED). Inputs takes a protocol populated  
/// compute agent `version` which is a 32-bit signed integer, an optional
/// [`Account`] for the contract's account under the field `account_info`, a
/// [`Transaction`] under the `transaction` field and then an `op`, i.e. an
/// operation that will be called from *within* the contract, and the `inputs`
/// to that `op`. The `inputs` to an op are always a JSON string, it can be
/// an empty JSON string, and sometimes, developers may choose to use additional
/// data that is provided in the `Transaction`. The `Inputs` struct is
/// serialized into JSON when passed into the contract, and can be deserialized
/// with either JSON helper functions and/or custom JSON parsing. The developer
/// has the flexibility to do with the `Inputs`, represented by JSON as they
/// choose.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    rename(serialize = "computeInputs", deserialize = "computeInputs"),
    rename_all = "camelCase"
)]
pub struct Inputs {
    /// The compute agent version
    pub version: i32,
    /// An optional program/contract's account in the protocol
    pub account_info: Account,
    /// The transaction that made the original call
    pub transaction: Transaction,
    /// The operation in the program being called
    pub op: String,
    /// The inputs to the contract operation being called
    #[serde(rename(serialize = "contractInputs", deserialize = "contractInputs"))]
    pub inputs: String,
}

/// The pre-requisite instructions for a contract call.
///
/// In many instances a contract will need to read data from an account, or
/// have another contract called and return data that will then be used by
/// the contract being called by the user. As a result, the protocol needs to
/// be able to somehow know what contracts to call before calling this program
/// and what data to read, and where to put that data in the `inputs` field in
/// the [`Inputs`] struct, i.e. what the key should be and what the value
/// should be. This will all be determined by the program schema. Program
/// schema is a developer defined schema that informs the protocol of information
/// (WARNING: PROGRAM SCHEMAS ARE EXPERIMENTAL AND NOT YET ENABLED).
///
/// This describes the action to take when a contract may be dependent upon
/// the result of some other action. The [`PreRequisite`] is the action and
/// the `outputs` are how the protocol keeps track of the results, e.g.
/// what to get, what to execute beforehand, etc.
///
// TODO(asmith): Replace outputs with a proper type, instead of a vector of
// tuples
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ParamPreRequisite {
    pre_requisites: PreRequisite,
    outputs: Vec<(usize, OpParams)>,
}

/// The structure returned by a program or [`CallTransactionResult`].
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Outputs {
    #[serde(
        rename(serialize = "computeInputs", deserialize = "computeInputs"),
        alias = "inputs"
    )]
    inputs: Inputs,
    instructions: Vec<Instruction>,
}

/// This struct is a builder for the [`Outputs`] struct.
/// It provides methods to set the inputs and add instructions to the Outputs struct.
#[derive(Clone, Default)]
pub struct OutputsBuilder {
    pub inputs: Option<Inputs>,
    pub instructions: Vec<Instruction>,
}

impl OutputsBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn inputs(mut self, inputs: Inputs) -> Self {
        self.inputs = Some(inputs);
        self
    }

    pub fn add_instruction(mut self, instruction: Instruction) -> Self {
        self.instructions.push(instruction);
        self
    }

    pub fn extend_instructions(mut self, instructions: Vec<Instruction>) -> Self {
        self.instructions.extend(instructions);
        self
    }

    pub fn build(&self) -> std::io::Result<Outputs> {
        Ok(Outputs {
            inputs: self
                .inputs
                .clone()
                .ok_or(std::io::Error::new(ErrorKind::Other, "inputs is required"))?,
            instructions: self.instructions.clone(),
        })
    }
}

impl Outputs {
    pub fn new(inputs: Inputs, instructions: Vec<Instruction>) -> Self {
        Self {
            inputs,
            instructions,
        }
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
    witnesses: Vec<TokenWitness>,
}

/// The action that must be performed prior to some contract call.
///
/// In some cases a contract may depend on the result of another job
/// or contract that should execute prior to the present contract.
/// See [`ParamPreRequisite`].
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum PreRequisite {
    /// Call another contract prior to the present call
    CallContract(CallParams),
    /// Unlock a token prior to the present call
    UnlockToken(AddressPair),
    /// Read from an account prior to the present call
    ReadAccount(ReadParams),
    /// Lock a token prior to the present call
    LockToken(AddressPair),
}

/// Information necessary to make a [`PreRequisite`] contract call.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CallParams {
    pub calling_program: Address,
    pub original_caller: Address,
    pub program_id: Address,
    pub inputs: Inputs,
}

/// Information necessary to lock, or unlock a token via [`PreRequisite`].
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AddressPair {
    pub account_address: Address,
    pub token_address: Address,
}

/// Information necessary for reading from some account via [`PreRequisite`].
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadParams {
    pub items: Vec<(Address, Address, TokenField)>,
    pub contract_blobs: Vec<Address>,
}

/// Used in specifying location information that could either
/// point to an account, token or a program's name, or ID.
#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum AddressOrNamespace {
    /// The address pointing to an account, or token.
    Address(Address),
    /// The name of a [`ProgramAccount`], or program.
    Namespace(Namespace),
    /// Refers to the program that returned it.
    ///
    /// Example:
    /// ```rust, ignore
    /// CreateInstruction {
    ///    program_namespace: AddressOrNamespace::This,
    ///    program_id: AddressOrNamespace::Address(Address::from([0; 20])),
    ///    program_owner: Address::from([0; 20]),
    ///    total_supply: crate::U256::from(0),
    ///    initialized_supply: crate::U256::from(0),
    ///    distribution: vec![TokenDistribution::default()],
    /// }
    /// ```
    This,
}

/// Information used in token creation.
#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstruction {
    program_namespace: AddressOrNamespace,
    program_id: AddressOrNamespace,
    program_owner: Address,
    total_supply: U256,
    initialized_supply: U256,
    distribution: Vec<TokenDistribution>,
}

/// A builder struct for the [`CreationInstruction`].
#[derive(
    Clone, Debug, Serialize, Default, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct CreateInstructionBuilder {
    pub program_namespace: Option<AddressOrNamespace>,
    pub program_id: Option<AddressOrNamespace>,
    pub program_owner: Option<Address>,
    pub total_supply: Option<U256>,
    pub initialized_supply: Option<U256>,
    pub distribution: Vec<TokenDistribution>,
}

impl CreateInstructionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn program_namespace(mut self, program_namespace: AddressOrNamespace) -> Self {
        self.program_namespace = Some(program_namespace);
        self
    }

    pub fn program_id(mut self, program_id: AddressOrNamespace) -> Self {
        self.program_id = Some(program_id);
        self
    }

    pub fn program_owner(mut self, program_owner: Address) -> Self {
        self.program_owner = Some(program_owner);
        self
    }

    pub fn total_supply(mut self, total_supply: U256) -> Self {
        self.total_supply = Some(total_supply);
        self
    }

    pub fn initialized_supply(mut self, initialized_supply: U256) -> Self {
        self.initialized_supply = Some(initialized_supply);
        self
    }

    pub fn extend_token_distributions(mut self, distributions: Vec<TokenDistribution>) -> Self {
        self.distribution.extend(distributions);
        self
    }

    pub fn add_token_distribution(mut self, distribution: TokenDistribution) -> Self {
        self.distribution.push(distribution);
        self
    }

    pub fn build(&self) -> Result<CreateInstruction, std::io::Error> {
        Ok(CreateInstruction {
            program_namespace: self.program_namespace.clone().ok_or(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "programNamespace is required",
            ))?,
            program_id: self.program_id.clone().ok_or(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "programId is required",
            ))?,
            program_owner: self.program_owner.ok_or(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "programOwner is required",
            ))?,
            total_supply: self.total_supply.ok_or(U256::MAX).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "totalSupply default is U256::MAX",
                )
            })?,
            initialized_supply: self
                .initialized_supply
                .ok_or_else(|| U256::from(0))
                .map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "initSupply default is 0")
                })?,
            distribution: self.distribution.clone(),
        })
    }
}

impl Default for CreateInstruction {
    fn default() -> Self {
        CreateInstruction {
            program_namespace: AddressOrNamespace::This,
            program_id: AddressOrNamespace::Address(Address::from([0; 20])),
            program_owner: Address::from([0; 20]),
            total_supply: crate::U256::from(0),
            initialized_supply: crate::U256::from(0),
            distribution: vec![TokenDistribution::default()],
        }
    }
}

impl CreateInstruction {
    pub fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
        let mut accounts_involved = vec![
            self.program_namespace.clone(),
            self.program_id.clone(),
            AddressOrNamespace::Address(self.program_owner),
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

    pub fn distribution(&self) -> &Vec<TokenDistribution> {
        &self.distribution
    }
}

/// Represents the distribution of tokens to a recipient.
#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct TokenDistribution {
    program_id: AddressOrNamespace,
    to: AddressOrNamespace,
    amount: Option<U256>,
    token_ids: Vec<U256>,
    update_fields: Vec<TokenUpdateField>,
}

/// A builder pattern implementation for creating instances of [`TokenDistribution`].
#[derive(
    Clone, Debug, Serialize, Default, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct TokenDistributionBuilder {
    pub program_id: Option<AddressOrNamespace>,
    pub to: Option<AddressOrNamespace>,
    pub amount: Option<U256>,
    pub token_ids: Vec<U256>,
    pub update_fields: Vec<TokenUpdateField>,
}

impl TokenDistributionBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn program_id(mut self, program_id: AddressOrNamespace) -> Self {
        self.program_id = Some(program_id);
        self
    }

    pub fn to(mut self, to: AddressOrNamespace) -> Self {
        self.to = Some(to);
        self
    }

    pub fn amount(mut self, amount: U256) -> Self {
        self.amount = Some(amount);
        self
    }

    pub fn add_token_id(mut self, token_id: U256) -> Self {
        self.token_ids.push(token_id);
        self
    }

    pub fn extend_token_ids(mut self, token_ids: Vec<U256>) -> Self {
        self.token_ids.extend(token_ids);
        self
    }

    pub fn add_update_field(mut self, update_field: TokenUpdateField) -> Self {
        self.update_fields.push(update_field);
        self
    }

    pub fn extend_update_fields(mut self, update_fields: Vec<TokenUpdateField>) -> Self {
        self.update_fields.extend(update_fields);
        self
    }

    pub fn build(&self) -> std::io::Result<TokenDistribution> {
        Ok(TokenDistribution {
            program_id: self.program_id.clone().ok_or(std::io::Error::new(
                ErrorKind::Other,
                "program id is required",
            ))?,
            to: self.to.clone().ok_or(std::io::Error::new(
                ErrorKind::Other,
                "to address is required",
            ))?,
            amount: self.amount,
            token_ids: self.token_ids.clone(),
            update_fields: self.update_fields.clone(),
        })
    }
}

impl Default for TokenDistribution {
    fn default() -> Self {
        TokenDistribution {
            program_id: AddressOrNamespace::This,
            to: AddressOrNamespace::Address(Address::from([0; 20])),
            amount: Some(crate::U256::from(0)),
            token_ids: vec![crate::U256::from(0)],
            update_fields: vec![TokenUpdateField::default()],
        }
    }
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

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum TokenOrProgramUpdateField {
    TokenUpdateField(TokenUpdateField),
    ProgramUpdateField(ProgramUpdateField),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TokenOrProgramUpdate {
    TokenUpdate(TokenUpdate),
    ProgramUpdate(ProgramUpdate),
}

impl Default for TokenOrProgramUpdate {
    fn default() -> Self {
        TokenOrProgramUpdate::TokenUpdate(TokenUpdate::default())
    }
}

/// Represents a field to be updated in a token.
#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct TokenUpdateField {
    field: TokenField,
    value: TokenFieldValue,
}

/// Builder pattern for creating instances of [`TokenUpdateField`].
#[derive(Clone, Default)]
pub struct TokenUpdateFieldBuilder {
    pub field: Option<TokenField>,
    pub value: Option<TokenFieldValue>,
}

impl TokenUpdateFieldBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn field(mut self, field: TokenField) -> Self {
        self.field = Some(field);
        self
    }

    pub fn value(mut self, value: TokenFieldValue) -> Self {
        self.value = Some(value);
        self
    }

    pub fn build(&self) -> std::io::Result<TokenUpdateField> {
        Ok(TokenUpdateField {
            field: self
                .field
                .clone()
                .ok_or(std::io::Error::new(ErrorKind::Other, "field is required"))?,
            value: self
                .value
                .clone()
                .ok_or(std::io::Error::new(ErrorKind::Other, "value is required"))?,
        })
    }
}

impl Default for TokenUpdateField {
    fn default() -> Self {
        let mut map = BTreeMap::new();
        map.insert("some".to_string(), "data".to_string());
        TokenUpdateField {
            field: TokenField::Data,
            value: TokenFieldValue::Metadata(crate::MetadataValue::Extend(map)),
        }
    }
}

impl TokenUpdateField {
    pub fn field(&self) -> &TokenField {
        &self.field
    }

    pub fn value(&self) -> &TokenFieldValue {
        &self.value
    }
}

/// Represents a field to be updated in a program.
#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct ProgramUpdateField {
    field: ProgramField,
    value: ProgramFieldValue,
}

/// Builder pattern for creating instances of [`ProgramUpdateField`].
#[derive(Clone, Default)]
pub struct ProgramUpdateFieldBuilder {
    pub field: Option<ProgramField>,
    pub value: Option<ProgramFieldValue>,
}

impl ProgramUpdateFieldBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn field(mut self, field: ProgramField) -> Self {
        self.field = Some(field);
        self
    }

    pub fn value(mut self, value: ProgramFieldValue) -> Self {
        self.value = Some(value);
        self
    }

    pub fn build(&self) -> std::io::Result<ProgramUpdateField> {
        Ok(ProgramUpdateField {
            field: self
                .field
                .clone()
                .ok_or(std::io::Error::new(ErrorKind::Other, "field is required"))?,
            value: self
                .value
                .clone()
                .ok_or(std::io::Error::new(ErrorKind::Other, "value is required"))?,
        })
    }
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

/// A vector of updates to either a token, or program.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstruction {
    updates: Vec<TokenOrProgramUpdate>,
}

/// Builder pattern for creating instances of [`UpdateInstruction`].
#[derive(Clone, Default)]
pub struct UpdateInstructionBuilder {
    pub updates: Vec<TokenOrProgramUpdate>,
}

impl UpdateInstructionBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add_update(mut self, update: TokenOrProgramUpdate) -> Self {
        self.updates.push(update);
        self
    }

    pub fn extend_updates(mut self, updates: Vec<TokenOrProgramUpdate>) -> Self {
        self.updates.extend(updates);
        self
    }

    pub fn build(&self) -> UpdateInstruction {
        UpdateInstruction {
            updates: self.updates.clone(),
        }
    }
}

impl Default for UpdateInstruction {
    fn default() -> Self {
        UpdateInstruction {
            updates: vec![TokenOrProgramUpdate::default()],
        }
    }
}

impl UpdateInstruction {
    pub fn new(updates: Vec<TokenOrProgramUpdate>) -> Self {
        Self { updates }
    }

    pub fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
        let mut accounts_involved = Vec::new();
        for update in &self.updates {
            match update {
                TokenOrProgramUpdate::TokenUpdate(token_update) => {
                    accounts_involved.push(token_update.account.clone())
                }
                TokenOrProgramUpdate::ProgramUpdate(program_update) => {
                    accounts_involved.push(program_update.account.clone())
                }
            }
        }
        accounts_involved
    }

    pub fn updates(&self) -> &Vec<TokenOrProgramUpdate> {
        &self.updates
    }
}

/// Represents an update to a token, including the account, token address, and update fields.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TokenUpdate {
    account: AddressOrNamespace,
    token: AddressOrNamespace,
    updates: Vec<TokenUpdateField>,
}

impl Default for TokenUpdate {
    fn default() -> Self {
        TokenUpdate {
            account: AddressOrNamespace::Address(Address::from([0; 20])),
            token: AddressOrNamespace::This,
            updates: vec![TokenUpdateField::default()],
        }
    }
}

/// Builder pattern for creating instances of [`TokenUpdate`].
#[derive(Clone, Default)]
pub struct TokenUpdateBuilder {
    pub account: Option<AddressOrNamespace>,
    pub token: Option<AddressOrNamespace>,
    pub updates: Vec<TokenUpdateField>,
}

impl TokenUpdateBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn account(mut self, account: AddressOrNamespace) -> Self {
        self.account = Some(account);
        self
    }

    pub fn token(mut self, token: AddressOrNamespace) -> Self {
        self.token = Some(token);
        self
    }

    pub fn add_update(mut self, update: TokenUpdateField) -> Self {
        self.updates.push(update);
        self
    }

    pub fn extend_updates(mut self, updates: Vec<TokenUpdateField>) -> Self {
        self.updates.extend(updates);
        self
    }

    pub fn build(&self) -> std::io::Result<TokenUpdate> {
        Ok(TokenUpdate {
            account: self
                .account
                .clone()
                .ok_or(std::io::Error::new(ErrorKind::Other, "account is required"))?,
            token: self
                .token
                .clone()
                .ok_or(std::io::Error::new(ErrorKind::Other, "token is required"))?,
            updates: self.updates.clone(),
        })
    }
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

/// Represents an update to a program, including the account and update fields.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProgramUpdate {
    account: AddressOrNamespace,
    updates: Vec<ProgramUpdateField>,
}

/// Builder pattern for creating instances of [`ProgramUpdate`].
#[derive(Clone, Default)]
pub struct ProgramUpdateBuilder {
    pub account: Option<AddressOrNamespace>,
    pub updates: Vec<ProgramUpdateField>,
}

impl ProgramUpdateBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn account(mut self, account: AddressOrNamespace) -> Self {
        self.account = Some(account);
        self
    }

    pub fn add_update(mut self, update: ProgramUpdateField) -> Self {
        self.updates.push(update);
        self
    }

    pub fn extend_updates(mut self, updates: Vec<ProgramUpdateField>) -> Self {
        self.updates.extend(updates);
        self
    }

    pub fn build(&self) -> std::io::Result<ProgramUpdate> {
        Ok(ProgramUpdate {
            account: self
                .account
                .clone()
                .ok_or(std::io::Error::new(ErrorKind::Other, "account is required"))?,
            updates: self.updates.clone(),
        })
    }
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

    /// Check if the account address or namespace is the program itself
    pub fn is_self(&self, account: &AddressOrNamespace) -> bool {
        self.account() == account
    }
}

/// Information needed to make a transfer of assets from one account
/// to another.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferInstruction {
    token: Address,
    from: AddressOrNamespace,
    to: AddressOrNamespace,
    amount: Option<crate::U256>,
    ids: Vec<crate::U256>,
}

/// Builder pattern for creating instances of [`TransferInstruction`].
#[derive(Clone, Default)]
pub struct TransferInstructionBuilder {
    pub token: Option<Address>,
    pub from: Option<AddressOrNamespace>,
    pub to: Option<AddressOrNamespace>,
    pub amount: Option<crate::U256>,
    pub ids: Vec<crate::U256>,
}

impl Default for TransferInstruction {
    fn default() -> Self {
        TransferInstruction {
            token: Address::from([0; 20]),
            from: AddressOrNamespace::This,
            to: AddressOrNamespace::Address(Address::from([0; 20])),
            amount: Some(crate::U256::from(0)),
            ids: vec![crate::U256::from(0)],
        }
    }
}

impl TransferInstructionBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn token(mut self, token: Address) -> Self {
        self.token = Some(token);
        self
    }

    pub fn from(mut self, from: AddressOrNamespace) -> Self {
        self.from = Some(from);
        self
    }

    pub fn to(mut self, to: AddressOrNamespace) -> Self {
        self.to = Some(to);
        self
    }

    pub fn amount(mut self, amount: crate::U256) -> Self {
        self.amount = Some(amount);
        self
    }

    pub fn add_id(mut self, token_id: crate::U256) -> Self {
        self.ids.push(token_id);
        self
    }

    pub fn extend_ids(mut self, token_ids: Vec<crate::U256>) -> Self {
        self.ids.extend(token_ids);
        self
    }

    pub fn build(&self) -> std::io::Result<TransferInstruction> {
        Ok(TransferInstruction {
            token: self.token.ok_or(std::io::Error::new(
                ErrorKind::Other,
                "token address is required",
            ))?,
            from: self.from.clone().ok_or(std::io::Error::new(
                ErrorKind::Other,
                "from address is required",
            ))?,
            to: self.to.clone().ok_or(std::io::Error::new(
                ErrorKind::Other,
                "to address is required",
            ))?,
            amount: self.amount,
            ids: self.ids.clone(),
        })
    }
}

impl TransferInstruction {
    pub fn new(
        token: Address,
        from: AddressOrNamespace,
        to: AddressOrNamespace,
        amount: Option<crate::U256>,
        ids: Vec<crate::U256>,
    ) -> Self {
        Self {
            token,
            from,
            to,
            amount,
            ids,
        }
    }

    pub fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
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

    pub fn replace_this_with_to(
        &mut self,
        transaction: &Transaction,
        _this: &AddressOrNamespace,
        field: &str,
    ) -> Result<(), std::io::Error> {
        match field {
            "from" => {
                self.from = AddressOrNamespace::Address(transaction.to());
            }
            "to" => {
                self.to = AddressOrNamespace::Address(transaction.to());
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "received an invalid string when calling `replace_this_with_to`",
                ))
            }
        }
        Ok(())
    }
}

/// Information used in token destruction.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BurnInstruction {
    caller: Address,
    program_id: AddressOrNamespace,
    token: Address,
    from: AddressOrNamespace,
    amount: Option<crate::U256>,
    token_ids: Vec<crate::U256>,
}

/// Builder pattern for creating instances of [`BurnInstruction`].
#[derive(Clone, Default)]
pub struct BurnInstructionBuilder {
    pub caller: Option<Address>,
    pub program_id: Option<AddressOrNamespace>,
    pub token: Option<Address>,
    pub from: Option<AddressOrNamespace>,
    pub amount: Option<crate::U256>,
    pub token_ids: Vec<crate::U256>,
}

impl Default for BurnInstruction {
    fn default() -> Self {
        BurnInstruction {
            caller: Address::from([0; 20]),
            program_id: AddressOrNamespace::This,
            token: Address::from([0; 20]),
            from: AddressOrNamespace::Address(Address::from([0; 20])),
            amount: Some(crate::U256::from(0)),
            token_ids: vec![crate::U256::from(0)],
        }
    }
}

impl BurnInstructionBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn caller(mut self, caller: Address) -> Self {
        self.caller = Some(caller);
        self
    }

    pub fn program_id(mut self, program_id: AddressOrNamespace) -> Self {
        self.program_id = Some(program_id);
        self
    }

    pub fn token(mut self, token: Address) -> Self {
        self.token = Some(token);
        self
    }

    pub fn from(mut self, from: AddressOrNamespace) -> Self {
        self.from = Some(from);
        self
    }

    pub fn amount(mut self, amount: crate::U256) -> Self {
        self.amount = Some(amount);
        self
    }

    pub fn add_token_id(mut self, token_id: crate::U256) -> Self {
        self.token_ids.push(token_id);
        self
    }

    pub fn extend_token_ids(mut self, token_ids: Vec<crate::U256>) -> Self {
        self.token_ids.extend(token_ids);
        self
    }

    pub fn build(&self) -> std::io::Result<BurnInstruction> {
        Ok(BurnInstruction {
            caller: self
                .caller
                .ok_or(std::io::Error::new(ErrorKind::Other, "caller is required"))?,
            program_id: self.program_id.clone().ok_or(std::io::Error::new(
                ErrorKind::Other,
                "program id is required",
            ))?,
            token: self
                .token
                .ok_or(std::io::Error::new(ErrorKind::Other, "token is required"))?,
            from: self
                .from
                .clone()
                .ok_or(std::io::Error::new(ErrorKind::Other, "from is required"))?,
            amount: self.amount,
            token_ids: self.token_ids.clone(),
        })
    }
}

impl BurnInstruction {
    pub fn accounts_involved(&self) -> Vec<AddressOrNamespace> {
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

/// Information used by the protcol to log something.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogInstruction(pub ContractLogType);

/// An enum representing the instructions that a program can return
/// to the protocol. Also represent types that tell the protocol what  
/// the pre-requisites of a given function call are.
/// All enabled languages have equivalent types.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Instruction {
    /// The return type created by the construction method of a contract
    Create(CreateInstruction),
    /// Tells the protocol to update a field, should almost never be used  
    /// to add balance to a token or add a token id (for Non-fungible or Data tokens)
    /// should primarily be used to update approvals, allowances, metadata, arbitrary data
    /// etc. Transfer or burn should be used to add/subtract balance. Lock/Unlock should be used
    /// to lock value
    Update(UpdateInstruction),
    /// Tells the protocol to subtract balance of one address/token pair and add to different
    /// address
    Transfer(TransferInstruction),
    /// Tells the protocol to burn a token (amount or id for NFT/Data tokens)
    Burn(BurnInstruction),
    /// Tells the protocol to log something
    Log(LogInstruction),
}

/// This trait serves as a marker trait for instruction types.
/// It doesn't define any methods or behavior itself but is implemented by instruction builders.
pub trait InnerInstruction {}

impl InnerInstruction for CreateInstructionBuilder {}
impl InnerInstruction for UpdateInstructionBuilder {}
impl InnerInstruction for TransferInstructionBuilder {}
impl InnerInstruction for BurnInstructionBuilder {}

/// Builder for constructing various [`Instruction`] types.
#[derive(Clone)]
pub struct InstructionBuilder<I: InnerInstruction> {
    pub inner: I,
}

impl Default for InstructionBuilder<CreateInstructionBuilder> {
    fn default() -> Self {
        Self {
            inner: CreateInstructionBuilder::default(),
        }
    }
}

impl InstructionBuilder<CreateInstructionBuilder> {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn program_namespace(mut self, program_namespace: AddressOrNamespace) -> Self {
        self.inner.program_namespace = Some(program_namespace);
        self
    }

    pub fn program_id(mut self, program_id: AddressOrNamespace) -> Self {
        self.inner.program_id = Some(program_id);
        self
    }

    pub fn program_owner(mut self, program_owner: Address) -> Self {
        self.inner.program_owner = Some(program_owner);
        self
    }

    pub fn total_supply(mut self, total_supply: U256) -> Self {
        self.inner.total_supply = Some(total_supply);
        self
    }

    pub fn initialized_supply(mut self, initialized_supply: U256) -> Self {
        self.inner.initialized_supply = Some(initialized_supply);
        self
    }

    pub fn extend_token_distributions(mut self, distributions: Vec<TokenDistribution>) -> Self {
        self.inner.distribution.extend(distributions);
        self
    }

    pub fn add_token_distribution(mut self, distribution: TokenDistribution) -> Self {
        self.inner.distribution.push(distribution);
        self
    }

    pub fn build(&self) -> Result<Instruction, std::io::Error> {
        Ok(Instruction::Create(CreateInstruction {
            program_namespace: self
                .inner
                .program_namespace
                .clone()
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "programNamespace is required",
                ))?,
            program_id: self.inner.program_id.clone().ok_or(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "programId is required",
            ))?,
            program_owner: self.inner.program_owner.ok_or(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "programOwner is required",
            ))?,
            total_supply: self.inner.total_supply.ok_or(U256::MAX).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "totalSupply default is U256::MAX",
                )
            })?,
            initialized_supply: self
                .inner
                .initialized_supply
                .ok_or_else(|| U256::from(0))
                .map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "initSupply default is 0")
                })?,
            distribution: self.inner.distribution.clone(),
        }))
    }
}

impl Default for InstructionBuilder<UpdateInstructionBuilder> {
    fn default() -> Self {
        Self {
            inner: UpdateInstructionBuilder::default(),
        }
    }
}

impl InstructionBuilder<UpdateInstructionBuilder> {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add_update(mut self, update: TokenOrProgramUpdate) -> Self {
        self.inner.updates.push(update);
        self
    }

    pub fn extend_updates(mut self, updates: Vec<TokenOrProgramUpdate>) -> Self {
        self.inner.updates.extend(updates);
        self
    }

    pub fn build(&self) -> Instruction {
        Instruction::Update(UpdateInstruction {
            updates: self.inner.updates.clone(),
        })
    }
}

impl Default for InstructionBuilder<TransferInstructionBuilder> {
    fn default() -> Self {
        Self {
            inner: TransferInstructionBuilder::default(),
        }
    }
}

impl InstructionBuilder<TransferInstructionBuilder> {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn token(mut self, token: Address) -> Self {
        self.inner.token = Some(token);
        self
    }

    pub fn from(mut self, from: AddressOrNamespace) -> Self {
        self.inner.from = Some(from);
        self
    }

    pub fn to(mut self, to: AddressOrNamespace) -> Self {
        self.inner.to = Some(to);
        self
    }

    pub fn amount(mut self, amount: crate::U256) -> Self {
        self.inner.amount = Some(amount);
        self
    }

    pub fn add_id(mut self, token_id: crate::U256) -> Self {
        self.inner.ids.push(token_id);
        self
    }

    pub fn extend_ids(mut self, token_ids: Vec<crate::U256>) -> Self {
        self.inner.ids.extend(token_ids);
        self
    }

    pub fn build(&self) -> std::io::Result<Instruction> {
        Ok(Instruction::Transfer(TransferInstruction {
            token: self.inner.token.ok_or(std::io::Error::new(
                ErrorKind::Other,
                "token address is required",
            ))?,
            from: self.inner.from.clone().ok_or(std::io::Error::new(
                ErrorKind::Other,
                "from address is required",
            ))?,
            to: self.inner.to.clone().ok_or(std::io::Error::new(
                ErrorKind::Other,
                "to address is required",
            ))?,
            amount: self.inner.amount,
            ids: self.inner.ids.clone(),
        }))
    }
}

impl Default for InstructionBuilder<BurnInstructionBuilder> {
    fn default() -> Self {
        Self {
            inner: BurnInstructionBuilder::default(),
        }
    }
}

impl InstructionBuilder<BurnInstructionBuilder> {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn caller(mut self, caller: Address) -> Self {
        self.inner.caller = Some(caller);
        self
    }

    pub fn program_id(mut self, program_id: AddressOrNamespace) -> Self {
        self.inner.program_id = Some(program_id);
        self
    }

    pub fn token(mut self, token: Address) -> Self {
        self.inner.token = Some(token);
        self
    }

    pub fn from(mut self, from: AddressOrNamespace) -> Self {
        self.inner.from = Some(from);
        self
    }

    pub fn amount(mut self, amount: crate::U256) -> Self {
        self.inner.amount = Some(amount);
        self
    }

    pub fn add_token_id(mut self, token_id: crate::U256) -> Self {
        self.inner.token_ids.push(token_id);
        self
    }

    pub fn extend_token_ids(mut self, token_ids: Vec<crate::U256>) -> Self {
        self.inner.token_ids.extend(token_ids);
        self
    }

    pub fn build(&self) -> std::io::Result<Instruction> {
        Ok(Instruction::Burn(BurnInstruction {
            caller: self
                .inner
                .caller
                .ok_or(std::io::Error::new(ErrorKind::Other, "caller is required"))?,
            program_id: self.inner.program_id.clone().ok_or(std::io::Error::new(
                ErrorKind::Other,
                "program id is required",
            ))?,
            token: self
                .inner
                .token
                .ok_or(std::io::Error::new(ErrorKind::Other, "token is required"))?,
            from: self
                .inner
                .from
                .clone()
                .ok_or(std::io::Error::new(ErrorKind::Other, "from is required"))?,
            amount: self.inner.amount,
            token_ids: self.inner.token_ids.clone(),
        }))
    }
}

impl Instruction {
    pub fn get_accounts_involved(&self) -> Vec<AddressOrNamespace> {
        match self {
            Self::Create(create) => create.accounts_involved(),
            Self::Update(update) => update.accounts_involved(),
            Self::Transfer(transfer) => transfer.accounts_involved(),
            Self::Burn(burn) => burn.accounts_involved(),
            Self::Log(_log) => vec![],
        }
    }
}

/// Indicates the level of priority of the message being logged.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum ContractLogType {
    Info(String),
    Error(String),
    Warn(String),
    Debug(String),
}

/// The name for each type that can be accepted by an operation as a parameter.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum OpParamTypes {
    Address,
    String,
    Tuple,
    /// The size of the array.
    FixedArray(usize),
    Array,
    /// The size of the byte array.
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

/// Defines the type system. Types that can be accepted by an operation as parameters.
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
    Mapping(HashMap<String, OpParams>),
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
            }
            OpParams::Array(arr) => arr.hash(state),
            OpParams::FixedBytes(arr, size) => {
                arr.hash(state);
                size.hash(state);
            }
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

/// Serves as a high-level representation of the structure and content of a program.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProgramSchema {
    pub contract: Contract,
}

/// Holds information about the contract defined within a program.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Contract {
    pub name: String,
    pub version: String,
    pub language: String,
    pub ops: HashMap<String, Op>,
}

/// Represents an operation in a contract.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Op {
    pub description: String,
    pub help: Option<String>,
    pub signature: Option<OpSignature>,
    pub required: Option<Vec<Required>>,
}

/// Represents the signature of an operation ([`Op`]).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OpSignature {
    #[serde(flatten)]
    pub op_signature: HashMap<String, String>,
    pub params_mapping: HashMap<String, ParamSource>,
}

/// Information pertaining to the source of a parameter in the [`OpSignature`] of an [`Op`] in a [`Contract`].
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ParamSource {
    pub source: String,
    pub field: Option<String>,
    pub key: Option<String>,
    pub position: usize,
}

/// Represents the prerequisites for an operation ([`Op`]).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "details")]
pub enum Required {
    /// Represents a call mapping in the prerequisites.
    Call(CallMap),
    /// Represents a read mapping in the prerequisites.
    Read(ReadMap),
    Lock(LockPair),
    Unlock(LockPair),
}

/// Represents a call mapping in the prerequisites.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CallMap {
    calling_program: TransactionFields,
    original_caller: TransactionFields,
    program_id: String,
    op: String,
    inputs: String,
}

/// Represents a read mapping in the prerequisites.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadMap {
    items: Vec<(String, String, String)>,
    contract_blobs: Option<Vec<String>>,
}

/// Represents a lock pair in the prerequisites.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LockPair {
    account: String,
    token: String,
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

    pub fn ops(&self) -> &HashMap<String, Op> {
        &self.contract().ops
    }

    pub fn get_op(&self, name: &str) -> Option<&Op> {
        self.ops().get(name)
    }

    pub fn get_prerequisites(&self, op: &str) -> std::io::Result<Vec<Required>> {
        let (_key, value) = self
            .contract
            .ops
            .get_key_value(op)
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Invalid `op`: Not defined in schema",
            ))?;

        if let Some(reqs) = &value.required {
            return Ok(reqs.clone());
        }

        Ok(Vec::new())
    }

    #[allow(unused)]
    pub fn parse_op_inputs(&self, op: &str, json_inputs: &str) -> std::io::Result<()> {
        let (_key, value) = self
            .contract
            .ops
            .get_key_value(op)
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Invalid `op`: Not defined in schema",
            ))?;

        let mut json: Map<String, Value> = serde_json::from_str(json_inputs)?;

        if let Some(function_signature) = &value.signature {
            for (k, v) in &function_signature.params_mapping {
                dbg!(&k, &v);
            }
        }

        Ok(())
    }
}

/// Trait for converting types into operation parameters.
pub trait Parameterize {
    fn into_op_params(self) -> Vec<OpParams>;
}

/// Trait for converting operation parameters.
pub trait Parameter {
    type Err;

    fn from_op_param(op_param: OpParams) -> Result<Self, Self::Err>
    where
        Self: Sized;

    fn into_op_param(self) -> OpParams;
}
