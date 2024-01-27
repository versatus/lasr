use std::process::{Command, Stdio};
use std::io::Write;
use serde_json::Result;
use lasr::Outputs;

fn main() -> Result<()> {
    let python_script_path = "./payload/testContainerPy/src/hello-world.py";

    let input_data = r#"{"version":1, "account_info":null, "op": "getName", "inputs": "{\"first_name\": \"Andrew\", \"last_name\": \"Smith\"}"}"#;

    let mut child = Command::new("python3")
        .arg(python_script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to spawn child process");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin.write_all(input_data.as_bytes()).expect("Failed to write to stdin");
    }

    let output = child
        .wait_with_output()
        .expect("Failed to read stdout");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("Python script failed: {}", stderr);
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    println!("{}", stdout);

    let parsed_output: Outputs = serde_json::from_str(&stdout)?;
    println!("Parsed output: {:?}", parsed_output);

    Ok(())
}

