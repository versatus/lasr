use crate::{
    AccountType,
    AddressOrNamespace,
    Address,
    ArbitraryData,
    Metadata,
    U256,
    Transaction,
    ProgramFieldValue,
    TokenUpdateField,
    ProgramUpdate
};

use std::collections::{BTreeMap, BTreeSet};

pub trait ProtocolAccount {
    type SendResult;
    type ProgramUpdateResult;
    type Token;
    /// Constructs a new `Account` with the given address and optional program data.
    ///
    /// This function initializes an account with the provided address and an optional
    /// map of programs. It updates the account hash before returning.
    fn new(account_type: AccountType, program_namespace: Option<AddressOrNamespace>, owner_address: Address, _programs: Option<BTreeMap<Address, Self::Token>>) -> Self;
    fn account_type(&self) -> AccountType;
    fn program_namespace(&self) -> Option<AddressOrNamespace>; 
    fn owner_address(&self) -> Address; 
    fn nonce(&self) -> U256;
    fn programs(&self) -> &BTreeMap<Address, Self::Token>;
    fn programs_mut(&mut self) -> &mut BTreeMap<Address, Self::Token>; 
    fn program_account_data(&self) -> &ArbitraryData;
    fn program_account_data_mut(&mut self) -> &mut ArbitraryData;
    fn program_account_metadata(&self) -> &Metadata;
    fn program_account_metadat_mut(&mut self) -> &mut Metadata;
    fn program_account_linked_programs(&self) -> &BTreeSet<AddressOrNamespace>;
    fn program_account_linked_programs_mut(&mut self) -> &mut BTreeSet<AddressOrNamespace>;
    fn balance(&self, program_id: &Address) -> U256; 
    fn apply_send_transaction(&mut self, transaction: Transaction) -> Self::SendResult;
    fn apply_transfer_to_instruction(&mut self, token_address: &Address, amount: &Option<U256>, token_ids: &Vec<U256>) -> Self::SendResult;
    fn apply_transfer_from_instruction(&mut self, token_address: &Address, amount: &Option<U256>, token_ids: &Vec<U256>) -> Self::SendResult;
    fn apply_burn_instruction(&mut self, token_address: &Address, amount: &Option<U256>, token_ids: &Vec<U256>) -> Self::SendResult;
    fn apply_token_distribution(&mut self, program_id: &Address, amount: &Option<U256>, token_ids: &Vec<U256>, token_updates: &Vec<TokenUpdateField>) -> Self::SendResult; 
    fn apply_token_update(&mut self, program_id: &Address, updates: &Vec<TokenUpdateField>) -> Self::SendResult; 
    fn apply_program_update_field_values(&mut self, update_field_value: &ProgramFieldValue) -> Self::ProgramUpdateResult; 
    fn apply_program_update(&mut self, update: &ProgramUpdate) -> Self::ProgramUpdateResult;
    fn insert_program(&mut self, program_id: &Address, token: Self::Token) -> Option<Self::Token>;
    fn validate_program_id(&self, program_id: &Address) -> Self::ProgramUpdateResult;
    fn validate_balance(&self, program_id: &Address, amount: U256) -> Self::ProgramUpdateResult; 
    fn validate_token_ownership(&self, program_id: &Address, token_ids: &Vec<U256>) -> Self::ProgramUpdateResult;
    fn validate_approved_spend(&self, program_id: &Address, spender: &Address, amount: &U256) -> Self::ProgramUpdateResult;
    fn validate_approved_token_transfer(&self, program_id: &Address, spender: &Address, token_ids: &Vec<crate::U256>) -> Self::ProgramUpdateResult;
    fn validate_nonce(&self, nonce: crate::U256) -> Self::ProgramUpdateResult;
    fn increment_nonce(&mut self);
}
