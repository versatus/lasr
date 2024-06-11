#![cfg(test)]
//! Test events that make changes to `Account`s.

use std::collections::{BTreeMap, BTreeSet};

use lasr_types::{
    Account, AccountBuilder, AccountType, Address, AddressOrNamespace, ArbitraryData, Metadata,
    Namespace, Status, TokenBuilder, U256,
};

pub fn default_test_account(hex_address: String) -> Account {
    let owner_address =
        Address::from_hex(&hex_address).expect("failed to produce address from hex str");
    let namespace = Namespace::from("TEST_NAMESPACE".to_string());
    let program_namespace = AddressOrNamespace::Namespace(namespace);
    let mut test_program_set = BTreeSet::new();
    test_program_set.insert(program_namespace.clone());
    let program_address = Address::new([4; 20]);
    let token = TokenBuilder::default()
        .program_id(program_address.clone())
        .owner_id(owner_address.clone())
        .balance(U256::from(666))
        .metadata(Metadata::new())
        .token_ids(vec![U256::from(69)])
        .allowance(BTreeMap::new())
        .approvals(BTreeMap::new())
        .data(ArbitraryData::new())
        .status(Status::Free)
        .build()
        .expect("failed to build test token");
    let mut programs = BTreeMap::new();
    programs.insert(program_address, token);
    AccountBuilder::default()
        .account_type(AccountType::User)
        .program_namespace(Some(program_namespace))
        .owner_address(owner_address)
        .programs(programs)
        .nonce(U256::from(0))
        .program_account_data(ArbitraryData::new())
        .program_account_metadata(Metadata::new())
        .program_account_linked_programs(test_program_set)
        .build()
        .expect("failed to build test account")
}

#[test]
fn bridge_in_event() {
    let account = default_test_account(Address::default().to_full_string());
}
