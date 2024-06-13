#![cfg(test)]
//! Test coverage for events that make changes to `Account`.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use lasr_actors::{
    get_actor_ref, AccountCacheActor, AccountCacheError, Batcher, BatcherActor,
    PendingTransactionActor, TaskScheduler,
};
use lasr_messages::{AccountCacheMessage, ActorName, ActorType};
use lasr_types::{
    Account, AccountBuilder, AccountType, Address, AddressOrNamespace, ArbitraryData, Metadata,
    MockPersistenceStore, Namespace, PersistenceStore, Status, TokenBuilder, Transaction, U256,
};

use ractor::Actor;
use tokio::sync::Mutex;

/// This is an account with nothing in it, with `Address([0; 20])`.
/// Often this will be the `receiver_test_account`, and can be used
/// in conjunction with the rust update syntax, e.g. `..empty_account()`.
pub fn empty_account() -> Account {
    Default::default()
}

/// The canonical `from` user account for testing purposes, and its program account.
pub fn sender_test_account_pair() -> (Account, Account) {
    const SENDER_ADDRESS: [u8; 20] = [1; 20];
    const SENDER_PROGRAM_NAMESPACE: &str = "SENDER_PROGRAM_NAMESPACE";
    const SENDER_PROGRAM_ADDRESS: [u8; 20] = [2; 20];
    const STARTING_TOKEN_BALANCE: u32 = 1000;
    const TOKEN_ID_0: u32 = 100;

    let owner_address = Address::new(SENDER_ADDRESS);
    let namespace = Namespace::from(SENDER_PROGRAM_NAMESPACE.to_string());

    let program_namespace = AddressOrNamespace::Namespace(namespace);
    let mut test_program_set = BTreeSet::new();
    test_program_set.insert(program_namespace.clone());
    let program_address = Address::new(SENDER_PROGRAM_ADDRESS);
    let token = TokenBuilder::default()
        .program_id(program_address.clone())
        .owner_id(owner_address.clone())
        .balance(U256::from(STARTING_TOKEN_BALANCE))
        .metadata(Metadata::new())
        .token_ids(vec![U256::from(TOKEN_ID_0)])
        .allowance(BTreeMap::new())
        .approvals(BTreeMap::new())
        .data(ArbitraryData::new())
        .status(Status::Free)
        .build()
        .expect("failed to build test token");
    let mut programs = BTreeMap::new();
    programs.insert(program_address, token);

    (
        AccountBuilder::default()
            .account_type(AccountType::User)
            .program_namespace(Some(program_namespace.clone()))
            .owner_address(owner_address)
            .programs(programs)
            .nonce(U256::from(0))
            .program_account_data(ArbitraryData::new())
            .program_account_metadata(Metadata::new())
            .program_account_linked_programs(test_program_set)
            .build()
            .expect("failed to build test user account"),
        AccountBuilder::default()
            .account_type(AccountType::Program(program_address))
            .program_namespace(Some(program_namespace))
            .owner_address(owner_address)
            .programs(BTreeMap::new())
            .nonce(U256::from(0))
            .program_account_data(ArbitraryData::new())
            .program_account_metadata(Metadata::new())
            .program_account_linked_programs(BTreeSet::new())
            .build()
            .expect("failed to build test program account"),
    )
}

#[tokio::test]
async fn bridge_in_event() {
    // Channel is not used, but is necessary to construct a `Batcher` and cannot be zero.
    const CHANNEL_BUFFER: usize = 1;
    let (receivers_thread_tx, _receivers_thread_rx) = tokio::sync::mpsc::channel(CHANNEL_BUFFER);
    let batcher = Arc::new(Mutex::new(Batcher::new(receivers_thread_tx)));
    let batcher_actor = BatcherActor::new();
    let (_batcher_ref, _batcher_handle) =
        Actor::spawn(Some(batcher_actor.name()), batcher_actor, batcher.clone())
            .await
            .expect("failed to spawn batcher actor");

    let mock_storage = <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::new()
        .await
        .expect("failed to create mock persistence storage");
    let account_cache_actor = AccountCacheActor::new();
    let (_account_cache_ref, _account_cache_handle) = Actor::spawn(
        Some(account_cache_actor.name()),
        account_cache_actor,
        mock_storage.clone(),
    )
    .await
    .expect("failed to spawn account cache actor");

    let scheduler_actor = TaskScheduler::new();
    let (_scheduler_ref, _scheduler_handle) =
        Actor::spawn(Some(scheduler_actor.name()), scheduler_actor, ())
            .await
            .expect("failed to spawn scheduler actor");

    let pending_transaction_actor = PendingTransactionActor::new();
    let (_pending_transaction_ref, _pending_transaction_handle) = Actor::spawn(
        Some(pending_transaction_actor.name()),
        pending_transaction_actor,
        (),
    )
    .await
    .expect("failed to spawn pending transaction actor");

    // Insert from accounts into storage & account cache
    let (from_account, from_program_account) = sender_test_account_pair();
    let AccountType::Program(from_program_address) = from_program_account.account_type() else {
        panic!("from_program_account is not a program account.")
    };
    assert!(
        <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
            &mock_storage,
            from_account.owner_address().to_full_string(),
            bincode::serialize(&from_account)
                .expect("failed serialization of from address for bridge in event")
        )
        .await
        .is_ok()
            && <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                &mock_storage,
                from_program_account.owner_address().to_full_string(),
                bincode::serialize(&from_program_account)
                    .expect("failed serialization of from address for bridge in event")
            )
            .await
            .is_ok()
    );
    assert!(
        get_actor_ref::<AccountCacheMessage, AccountCacheError>(ActorType::AccountCache)
            .and_then(|account_cache| account_cache
                .send_message(AccountCacheMessage::Write {
                    account: from_account.clone(),
                    who: ActorType::AccountCache,
                    location: "bridge_in_event test".into(),
                })
                .ok()
                .zip(
                    account_cache
                        .send_message(AccountCacheMessage::Write {
                            account: from_program_account,
                            who: ActorType::AccountCache,
                            location: "bridge_in_event test".into()
                        })
                        .ok()
                ))
            .is_some()
    );

    let to_account = empty_account();

    const BRIDGE_IN_AMOUNT: u64 = 1;
    let bridge_in_transaction = Transaction::bridge_in(
        BRIDGE_IN_AMOUNT,
        from_account.owner_address(),
        from_program_address,
        to_account.owner_address(),
    );
    let res = Batcher::add_transaction_to_account(batcher, bridge_in_transaction).await;

    assert!(res.is_ok());
    assert_eq!(from_account.balance(&from_program_address), U256::from(999));
    assert_eq!(
        to_account.balance(&Default::default()),
        U256::from(BRIDGE_IN_AMOUNT)
    );
    // drop all actors, not strictly necessary.
    let (_, _, _, _) = (
        _batcher_handle,
        _account_cache_handle,
        _scheduler_handle,
        _pending_transaction_handle,
    );
}
