use std::collections::{BTreeMap, BTreeSet};

use lasr_types::{Account, AccountBuilder, AccountType, Address, ArbitraryData, Metadata, U256};
use serde::{Deserialize, Serialize};
use tikv_client::RawClient;

// Structure for persistence layer `Account` values
#[derive(Debug, Hash, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountValue {
    account: Account,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mock_account = AccountBuilder::default()
        .account_type(AccountType::User)
        .program_namespace(None)
        .owner_address(Address::new([0; 20]))
        .programs(BTreeMap::new())
        .nonce(U256::from(0))
        .program_account_data(ArbitraryData::new())
        .program_account_metadata(Metadata::new())
        .program_account_linked_programs(BTreeSet::new())
        .build()?;
    println!("Mock account built: {:?}", mock_account.owner_address());

    let client = RawClient::new(vec!["127.0.0.1:2379"])
        .await
        .expect("failed to connect to persistence layer.");
    println!("Connected to TiKV client.");

    let data = mock_account.clone();
    println!("Raw `Account` before serialization: {:?}", data);

    let acc_key = data.owner_address().to_string();
    let acc_val = AccountValue { account: data };

    // Serialize `Account` data to be stored.
    let val = bincode::serialize(&acc_val)
        .ok()
        .expect("failed to serialize `Account` data.");

    println!("Serialized `Account` data: {:?}", val);

    // Push `Account` data to persistence store
    client
        .put(acc_key.to_owned(), val)
        .await
        .expect("failed to insert account into persistence store.");

    println!("Inserted Account with Address: {:?}", acc_key.clone());

    // Pull `Account` data from persistence store
    let returned_data = client
        .get(acc_key.to_owned())
        .await
        .ok()
        .unwrap()
        .expect("failed to retrieve account from persistence store.");

    println!(
        "Retrieved Serialized Account: {:?} with Address: {:?}",
        returned_data, acc_key
    );

    // Confirm data hasn't changed
    let deserialized_data: Account =
        bincode::deserialize(&returned_data).expect("failed to deserialize `Account` data.");
    println!(
        "Raw `Account` after deserialization: {:?}",
        deserialized_data
    );

    Ok(())
}
