use std::io::Write;

use lasr::{Inputs, Outputs};
use schemars::schema_for;
use serde::{Serialize, Deserialize};

fn main() -> std::io::Result<()> {
    let outputs_schema = schema_for!(Outputs);
    let inputs_schema = schema_for!(Inputs);

    let outputs_schema_string = serde_json::to_string_pretty(&outputs_schema).unwrap();
    let inputs_schema_string = serde_json::to_string_pretty(&inputs_schema).unwrap();

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/outputs_schema.json")?;

    file.write_all(outputs_schema_string.as_bytes());

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .read(true)
        .create(true)
        .open("./json_schema/inputs_schema.json")?;

    file.write_all(inputs_schema_string.as_bytes());

    Ok(())
}
