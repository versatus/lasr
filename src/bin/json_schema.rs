#![allow(unused)]
use std::io::Write;

use lasr::{Inputs, Outputs, Status, Instruction};
use schemars::schema_for;
use serde::{Serialize, Deserialize};

fn main() -> std::io::Result<()> {
    let outputs_schema = schema_for!(Outputs);
    let inputs_schema = schema_for!(Inputs);

    let outputs_schema_string = serde_json::to_string_pretty(&outputs_schema).unwrap();
    let inputs_schema_string = serde_json::to_string_pretty(&inputs_schema).unwrap();

    //let mut file = std::fs::OpenOptions::new()
    //    .write(true)
    //    .append(true)
    //    .read(true)
    //    .create(true)
    //    .open("./json_schema/outputs_schema.json")?;

    //file.write_all(outputs_schema_string.as_bytes());

    //let mut file = std::fs::OpenOptions::new()
    //    .write(true)
    //    .append(true)
    //    .read(true)
    //    .create(true)
    //    .open("./json_schema/inputs_schema.json")?;

    //file.write_all(inputs_schema_string.as_bytes());

    let status_schema = schema_for!(Status);
    let status_schema_string = serde_json::to_string_pretty(&status_schema).unwrap();
    println!("{}", status_schema_string);
    let instructions_schema = schema_for!(Instruction);
    let instruction_schema_string = serde_json::to_string_pretty(&instructions_schema).unwrap();
    println!("{}", instruction_schema_string);

    Ok(())
}
