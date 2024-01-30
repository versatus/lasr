#![allow(unused)]
use std::io::Write;

use lasr::{
    Inputs, Outputs, Status, Instruction, CreateInstruction, UpdateInstruction, 
    TransferInstruction, BurnInstruction, LogInstruction,
    TokenDistribution, TokenOrProgramUpdate, TokenOrProgramUpdateField,
    TokenUpdate, ProgramUpdate, TokenUpdateField, ProgramUpdateField,
    TokenField, TokenFieldValue, ProgramField, ProgramFieldValue, BalanceValue,
    MetadataValue, AllowanceValue, ApprovalsValue, DataValue, StatusValue,
    LinkedProgramsValue, ContractLogType, Address, Namespace, AddressOrNamespace,
    U256
};
use schemars::schema_for;
use serde::{Serialize, Deserialize};

fn main() -> std::io::Result<()> {
    let outputs_schema = schema_for!(Outputs);
    let outputs_schema_string = serde_json::to_string_pretty(&outputs_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/outputs_schema.json")?;

    file.write_all(outputs_schema_string.as_bytes());

    let inputs_schema = schema_for!(Inputs);
    let inputs_schema_string = serde_json::to_string_pretty(&inputs_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/inputs_schema.json")?;

    file.write_all(inputs_schema_string.as_bytes());

    let instruction_schema = schema_for!(Instruction);
    let instruction_schema_string = serde_json::to_string_pretty(&instruction_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/instruction_schema.json")?;

    file.write_all(instruction_schema_string.as_bytes());

    let create_instruction_schema = schema_for!(CreateInstruction);
    let create_instruction_schema_string = serde_json::to_string_pretty(&create_instruction_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/create_instruction_schema.json")?;

    file.write_all(create_instruction_schema_string.as_bytes());



    let update_instruction_schema = schema_for!(UpdateInstruction);
    let update_instruction_schema_string = serde_json::to_string_pretty(&update_instruction_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/update_instruction_schema.json")?;

    file.write_all(update_instruction_schema_string.as_bytes());

    let transfer_instruction_schema = schema_for!(TransferInstruction);
    let transfer_instruction_schema_string = serde_json::to_string_pretty(&transfer_instruction_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/transfer_instruction_schema.json")?;

    file.write_all(transfer_instruction_schema_string.as_bytes());


    let burn_instruction_schema = schema_for!(BurnInstruction);
    let burn_instruction_schema_string = serde_json::to_string_pretty(&burn_instruction_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/burn_instruction_schema.json")?;

    file.write_all(burn_instruction_schema_string.as_bytes());

    let log_instruction_schema = schema_for!(LogInstruction);

    let token_distribution_schema = schema_for!(TokenDistribution);
    let token_distribution_schema_string = serde_json::to_string_pretty(&token_distribution_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/token_distribution_schema.json")?;

    file.write_all(token_distribution_schema_string.as_bytes());

    let token_or_program_update_schema = schema_for!(TokenOrProgramUpdate);
    let token_or_program_update_schema_string = serde_json::to_string_pretty(&token_or_program_update_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/token_or_program_update_schema.json")?;

    file.write_all(token_or_program_update_schema_string.as_bytes());

    let token_or_program_update_field_schema = schema_for!(TokenOrProgramUpdateField);
    let token_or_program_update_field_schema_string = serde_json::to_string_pretty(&token_or_program_update_field_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/token_or_program_update_field_schema.json")?;

    file.write_all(token_or_program_update_field_schema_string.as_bytes());

    let token_update_schema = schema_for!(TokenUpdate);
    let token_update_schema_string = serde_json::to_string_pretty(&token_update_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/token_update_schema.json")?;

    file.write_all(token_update_schema_string.as_bytes());

    let token_update_field_schema = schema_for!(TokenUpdateField);
    let token_update_field_schema_string = serde_json::to_string_pretty(&token_update_field_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/token_update_field_schema.json")?;

    file.write_all(token_update_field_schema_string.as_bytes());

    let token_field_schema = schema_for!(TokenField);
    let token_field_schema_string = serde_json::to_string_pretty(&token_field_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/token_field_schema.json")?;

    file.write_all(token_field_schema_string.as_bytes());

    let token_field_value_schema = schema_for!(TokenFieldValue);
    let token_field_value_schema_string = serde_json::to_string_pretty(&token_field_value_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/token_field_value_schema.json")?;

    file.write_all(token_field_value_schema_string.as_bytes());
    
    let program_update_schema = schema_for!(ProgramUpdate);
    let program_update_schema_string = serde_json::to_string_pretty(&program_update_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/program_update_schema.json")?;

    file.write_all(program_update_schema_string.as_bytes());

    let program_update_field_schema = schema_for!(ProgramUpdateField);
    let program_update_field_schema_string = serde_json::to_string_pretty(&program_update_field_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/program_update_field_schema.json")?;

    file.write_all(program_update_field_schema_string.as_bytes());

    let program_field_schema = schema_for!(ProgramField);
    let program_field_schema_string = serde_json::to_string_pretty(&program_field_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/program_field_schema.json")?;

    file.write_all(program_field_schema_string.as_bytes());

    let program_update_field_value_schema = schema_for!(ProgramFieldValue); 
    let program_field_value_schema_string = serde_json::to_string_pretty(&program_update_field_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/program_field_value_schema.json")?;

    file.write_all(program_field_value_schema_string.as_bytes());

    let balance_value_schema = schema_for!(BalanceValue);
    let balance_value_schema_string = serde_json::to_string_pretty(&balance_value_schema).unwrap();
    
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/balance_value_schema.json")?;

    file.write_all(balance_value_schema_string.as_bytes());

    let metadata_value_schema = schema_for!(MetadataValue);
    let metadata_value_schema_string = serde_json::to_string_pretty(&metadata_value_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/metadata_value_schema.json")?;

    file.write_all(metadata_value_schema_string.as_bytes());

    let data_value_schema = schema_for!(DataValue);
    let data_value_schema_string = serde_json::to_string_pretty(&data_value_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/data_value_schema.json")?;

    file.write_all(data_value_schema_string.as_bytes());

    let approvals_value_schema = schema_for!(ApprovalsValue);
    let approvals_value_schema_string = serde_json::to_string_pretty(&approvals_value_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/approvals_value_schema.json")?;

    file.write_all(approvals_value_schema_string.as_bytes());

    let allowance_value_schema = schema_for!(AllowanceValue); 
    let allowance_value_schema_string = serde_json::to_string_pretty(&allowance_value_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/allowance_value_schema.json")?;

    file.write_all(allowance_value_schema_string.as_bytes());

    let status_value_schema = schema_for!(StatusValue);
    let status_value_schema_string = serde_json::to_string_pretty(&status_value_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/status_value_schema.json")?;

    file.write_all(status_value_schema_string.as_bytes());

    let linked_programs_value_schema = schema_for!(LinkedProgramsValue);
    let linked_programs_value_schema_string = serde_json::to_string_pretty(&linked_programs_value_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/linked_programs_value_schema.json")?;

    file.write_all(linked_programs_value_schema_string.as_bytes());

    let address_schema = schema_for!(Address);
    let address_schema_string = serde_json::to_string_pretty(&address_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/address_schema.json")?;

    file.write_all(address_schema_string.as_bytes());

    let namespace_schema = schema_for!(Namespace);
    let namespace_schema_string = serde_json::to_string_pretty(&namespace_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/namespace_schema.json")?;

    file.write_all(namespace_schema_string.as_bytes());

    let address_or_namespace_schema = schema_for!(AddressOrNamespace);
    let address_or_namespace_schema_string = serde_json::to_string_pretty(&address_or_namespace_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/address_or_namespace_schema.json")?;

    file.write_all(address_or_namespace_schema_string.as_bytes());

    let u256_schema = schema_for!(lasr::U256);
    let u256_schema_string = serde_json::to_string_pretty(&u256_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/u256_schema.json")?;

    file.write_all(u256_schema_string.as_bytes());
    
    Ok(())
}
