use std::collections::{BTreeMap, BTreeSet};

use lasr_types::{Account, AccountBuilder, AccountType, Address, ArbitraryData, Metadata, U256};
use serde::{Deserialize, Serialize};
use tikv_client::RawClient;

// Structure for persistence store `Account` values
#[derive(Debug, Hash, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountValue {
    account: Account,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mock_user_account = AccountBuilder::default()
        .account_type(AccountType::User)
        .program_namespace(None)
        .owner_address(Address::new([0; 20]))
        .programs(BTreeMap::new())
        .nonce(U256::from(0))
        .program_account_data(ArbitraryData::new())
        .program_account_metadata(Metadata::new())
        .program_account_linked_programs(BTreeSet::new())
        .build()?;
    println!(
        "Mock user account built: {:?}",
        mock_user_account.owner_address().to_full_string()
    );

    let mock_program_account = AccountBuilder::default()
        .account_type(AccountType::Program([1; 20].into()))
        .program_namespace(None)
        .owner_address(Address::new([0; 20]))
        .programs(BTreeMap::new())
        .nonce(U256::from(0))
        .program_account_data(ArbitraryData::new())
        .program_account_metadata(Metadata::new())
        .program_account_linked_programs(BTreeSet::new())
        .build()?;

    if let AccountType::Program(program_addr) = mock_program_account.account_type() {
        println!(
            "Mock program account built: {:?}",
            program_addr.to_full_string()
        );
    }

    let client = RawClient::new(vec!["127.0.0.1:2379"])
        .await
        .expect("failed to connect to persistence store.");
    println!("Connected to TiKV client.");
    println!(" ");

    let user_data = mock_user_account.clone();
    println!("Raw User `Account` before serialization: {:?}", user_data);

    let program_data = mock_program_account.clone();
    println!(
        "Raw Program `Account` before serialization: {:?}",
        program_data
    );

    let user_acc_key = user_data.owner_address().to_full_string();
    let user_acc_val = AccountValue { account: user_data };

    let prgm_acc_val = AccountValue {
        account: program_data.clone(),
    };
    println!(" ");

    // Serialize User `Account` data to be stored.
    let user_val = bincode::serialize(&user_acc_val)
        .ok()
        .expect("failed to serialize user `Account` data.");

    println!("Serialized User `Account` data: {:?}", user_val.to_vec());

    // Serialize Program `Account` data to be stored.
    let prgm_val = bincode::serialize(&prgm_acc_val)
        .ok()
        .expect("failed to serialize program `Account` data.");

    println!("Serialized Program `Account` data: {:?}", prgm_val);
    println!(" ");

    // Push User `Account` data to persistence store
    client
        .put(user_acc_key.to_owned(), user_val)
        .await
        .expect("failed to insert account into persistence store.");

    println!(
        "Inserted User `Account` with Address: {:?}",
        user_acc_key.clone()
    );

    // Push Program `Account` data to persistence store
    if let AccountType::Program(program_addr) = program_data.account_type() {
        let prgm_acc_key = program_addr.to_full_string();
        client
            .put(prgm_acc_key.to_owned(), prgm_val)
            .await
            .expect("failed to insert program account into persistence store");

        println!(
            "Inserted Program `Account` with Address: {:?}",
            prgm_acc_key.clone()
        );
    }
    println!(" ");

    // Pull User `Account` data from persistence store
    let returned_user_data = client
        .get(user_acc_key.to_owned())
        .await
        .ok()
        .unwrap()
        .expect("failed to retrieve user account from persistence store.");

    println!(
        "Retrieved Serialized User `Account`: {:?}
    with Address: {:?}",
        returned_user_data, user_acc_key
    );

    // Pull Program `Account` data from persistence store
    if let AccountType::Program(program_addr) = program_data.account_type() {
        let prgm_acc_key = program_addr.to_full_string();
        let returned_prgm_data = client
            .get(prgm_acc_key.to_owned())
            .await
            .ok()
            .unwrap()
            .expect("failed to retrieve program account from persistence store.");

        println!(
            "Retrieved Serialized Program `Account`: {:?}
    with Address: {:?}",
            returned_prgm_data, prgm_acc_key
        );
        println!(" ");

        // Confirm Program data hasn't changed
        let deserialized_prgm_data: Account = bincode::deserialize(&returned_prgm_data)
            .expect("failed to deserialize Program `Account` data");
        println!(
            "Raw Program `Account` after deserialization: {:?}",
            deserialized_prgm_data
        );
    }

    // Confirm User data hasn't changed
    let deserialized_user_data: Account = bincode::deserialize(&returned_user_data)
        .expect("failed to deserialize User `Account` data.");
    println!(
        "Raw User `Account` after deserialization: {:?}",
        deserialized_user_data
    );

    Ok(())
}
