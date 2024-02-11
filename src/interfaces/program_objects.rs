use std::collections::BTreeMap;
use crate::{
    U256,
    DataValue,
    TokenFieldValue,
    MetadataValue,
    ApprovalsValue,
    AllowanceValue,
    StatusValue,
    Address,
    ArbitraryData,
    Metadata,
    Status
};

pub trait ProgramObject {
    fn debit(&mut self, amount: &U256) -> Result<(), Box<dyn std::error::Error + Send>>;
    fn credit(&mut self, amount: &U256) -> Result<(), Box<dyn std::error::Error + Send>>;
    fn remove_token_ids(&mut self, token_ids: &Vec<U256>) -> Result<(), Box<dyn std::error::Error + Send>>;
    fn add_token_ids(&mut self, token_ids: &Vec<U256>) -> Result<(), Box<dyn std::error::Error + Send>>;
    fn apply_token_update_field_values(&mut self, token_update_value: &TokenFieldValue) -> Result<(), Box<dyn std::error::Error + Send>>;
    fn apply_data_update(&mut self, data_update: &DataValue) -> Result<(), Box<dyn std::error::Error + Send>>;
    fn apply_metadata_update(&mut self, metadata_update: &MetadataValue) -> Result<(), Box<dyn std::error::Error + Send>>;
    fn apply_approvals_update(&mut self, approvals_update: &ApprovalsValue) -> Result<(), Box<dyn std::error::Error + Send>>;
    fn apply_allowance_update(&mut self, allowance_update: &AllowanceValue) -> Result<(), Box<dyn std::error::Error + Send>>;
    fn apply_status_update(&mut self, status_update: &StatusValue) -> Result<(), Box<dyn std::error::Error + Send>>;
    fn program_id(&self) -> Address;
    fn owner_id(&self) -> Address;
    fn balance(&self) -> U256;
    fn balance_mut(&mut self) -> &mut U256;
    fn metadata(&self) -> Metadata;
    fn metadata_mut(&mut self) -> &mut Metadata;
    fn token_ids(&self) -> Vec<U256>;
    fn token_ids_mut(&mut self) -> &mut Vec<U256>;
    fn allowance(&self) -> BTreeMap<Address, U256>;
    fn allowance_mut(&mut self) -> &mut BTreeMap<Address, U256>;
    fn approvals(&self) -> BTreeMap<Address, Vec<U256>>;
    fn approvals_mut(&mut self) -> &mut BTreeMap<Address, Vec<U256>>;
    fn data(&self) -> ArbitraryData;
    fn data_mut(&mut self) -> &mut ArbitraryData;
    fn status(&self) -> Status;
    fn status_mut(&mut self) -> &mut Status;
    fn update_balance(&mut self, receive: U256, send: U256);
}
