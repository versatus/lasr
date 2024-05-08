use tikv_client::{RawClient, Key, Value};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to the TiKV store
    let client = RawClient::new(vec!["127.0.0.1:2379"]).await?;  // Use localhost and the port exposed by Docker
    
    // Put a key-value pair
    let key = Key::from("test_key".to_owned());
    let value = Value::from("Hello, World!".as_bytes().to_vec());
    client.put(key.clone(), value).await?;

    // Get the value back
    let returned_value = client.get(key).await?;
    println!("Retrieved value: {:?}", returned_value);
    
    Ok(())
}
