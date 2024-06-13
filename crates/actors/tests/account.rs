#![cfg(test)]
//! Test coverage for events that make changes to `Account`.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use lasr_actors::{
    get_account, get_actor_ref, AccountCacheActor, AccountCacheError, Batcher, BatcherActor,
    PendingTransactionActor, TaskScheduler, ETH_ADDR,
};
use lasr_messages::{AccountCacheMessage, ActorName, ActorType};
use lasr_types::{
    Account, AccountBuilder, AccountType, Address, AddressOrNamespace, ArbitraryData, Metadata,
    MockPersistenceStore, Namespace, PersistenceStore, Status, TokenBuilder, Transaction, U256,
};

use ractor::Actor;
use serial_test::serial;
use tokio::sync::Mutex;

/// This is an account with nothing in it, with `Address([1; 20])`.
pub fn receiver_test_account() -> Account {
    Account::test_default_user_account()
}

/// The canonical `from` user account for testing purposes, and its program account.
pub fn sender_test_account_pair() -> (Account, Account) {
    const SENDER_ADDRESS: [u8; 20] = [2; 20];
    const SENDER_PROGRAM_NAMESPACE: &str = "SENDER_PROGRAM_NAMESPACE";
    const SENDER_PROGRAM_ADDRESS: [u8; 20] = [3; 20];
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

#[serial]
#[tokio::test]
async fn bridge_in_event() {
    // Channel is not used, but is necessary to construct a `Batcher` and cannot be zero.
    const CHANNEL_BUFFER: usize = 1;
    let (receivers_thread_tx, _receivers_thread_rx) = tokio::sync::mpsc::channel(CHANNEL_BUFFER);
    let batcher = Arc::new(Mutex::new(Batcher::new(receivers_thread_tx)));
    let batcher_actor = BatcherActor::new();
    let (batcher_ref, _batcher_handle) =
        Actor::spawn(Some(batcher_actor.name()), batcher_actor, batcher.clone())
            .await
            .expect("failed to spawn batcher actor");

    let mock_storage = <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::new()
        .await
        .expect("failed to create mock persistence storage");
    let account_cache_actor = AccountCacheActor::new();
    let (account_cache_ref, _account_cache_handle) = Actor::spawn(
        Some(account_cache_actor.name()),
        account_cache_actor,
        mock_storage.clone(),
    )
    .await
    .expect("failed to spawn account cache actor");

    let scheduler_actor = TaskScheduler::new();
    let (scheduler_ref, _scheduler_handle) =
        Actor::spawn(Some(scheduler_actor.name()), scheduler_actor, ())
            .await
            .expect("failed to spawn scheduler actor");

    let pending_transaction_actor = PendingTransactionActor::new();
    let (pending_transaction_ref, _pending_transaction_handle) = Actor::spawn(
        Some(pending_transaction_actor.name()),
        pending_transaction_actor,
        (),
    )
    .await
    .expect("failed to spawn pending transaction actor");

    // Insert account into storage & account cache.
    let to_account = receiver_test_account();
    let to_account_address = to_account.owner_address();
    assert!(
        <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
            &mock_storage,
            to_account.owner_address().to_full_string(),
            bincode::serialize(&to_account).expect("failed serialization of to address")
        )
        .await
        .is_ok()
    );
    assert!(
        get_actor_ref::<AccountCacheMessage, AccountCacheError>(ActorType::AccountCache)
            .and_then(|account_cache| account_cache
                .send_message(AccountCacheMessage::Write {
                    account: to_account.clone(),
                    who: ActorType::AccountCache,
                    location: "bridge_in_event test".into()
                })
                .ok())
            .is_some()
    );

    const BRIDGE_IN_AMOUNT: u64 = 1;
    let program_id = ETH_ADDR;
    let bridge_in_transaction =
        Transaction::test_bridge_in(BRIDGE_IN_AMOUNT, program_id, to_account_address);
    let res = Batcher::add_transaction_to_account(batcher, bridge_in_transaction).await;

    let to_account_with_updates = get_account(to_account_address, ActorType::AccountCache)
        .await
        .expect("could not find to account");

    assert!(res.is_ok());
    assert_eq!(
        to_account_with_updates.balance(&program_id),
        U256::from(BRIDGE_IN_AMOUNT)
    );
    batcher_ref.stop_and_wait(None, None).await.ok();
    account_cache_ref.stop_and_wait(None, None).await.ok();
    scheduler_ref.stop_and_wait(None, None).await.ok();
    pending_transaction_ref.stop_and_wait(None, None).await.ok();
    std::thread::sleep(std::time::Duration::from_secs(5));
}

#[serial]
#[tokio::test]
async fn send_event() {
    // Channel is not used, but is necessary to construct a `Batcher` and cannot be zero.
    const CHANNEL_BUFFER: usize = 1;
    let (receivers_thread_tx, _receivers_thread_rx) = tokio::sync::mpsc::channel(CHANNEL_BUFFER);
    let batcher = Arc::new(Mutex::new(Batcher::new(receivers_thread_tx)));
    let batcher_actor = BatcherActor::new();
    let (batcher_ref, _batcher_handle) =
        Actor::spawn(Some(batcher_actor.name()), batcher_actor, batcher.clone())
            .await
            .expect("failed to spawn batcher actor");

    let mock_storage = <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::new()
        .await
        .expect("failed to create mock persistence storage");
    let account_cache_actor = AccountCacheActor::new();
    let (account_cache_ref, _account_cache_handle) = Actor::spawn(
        Some(account_cache_actor.name()),
        account_cache_actor,
        mock_storage.clone(),
    )
    .await
    .expect("failed to spawn account cache actor");

    let scheduler_actor = TaskScheduler::new();
    let (scheduler_ref, _scheduler_handle) =
        Actor::spawn(Some(scheduler_actor.name()), scheduler_actor, ())
            .await
            .expect("failed to spawn scheduler actor");

    let pending_transaction_actor = PendingTransactionActor::new();
    let (pending_transaction_ref, _pending_transaction_handle) = Actor::spawn(
        Some(pending_transaction_actor.name()),
        pending_transaction_actor,
        (),
    )
    .await
    .expect("failed to spawn pending transaction actor");

    // Insert accounts into storage & account cache.
    // This simulates an already registered program.
    let (from_account, from_program_account) = sender_test_account_pair();
    let from_account_address = from_account.owner_address();
    let AccountType::Program(from_program_address) = from_program_account.account_type() else {
        panic!("from_program_account is not a program account.")
    };
    let to_account = receiver_test_account();
    let to_account_address = to_account.owner_address();
    assert!(
        <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
            &mock_storage,
            from_account.owner_address().to_full_string(),
            bincode::serialize(&from_account).expect("failed serialization of from address")
        )
        .await
        .is_ok()
            && <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                &mock_storage,
                from_program_account.owner_address().to_full_string(),
                bincode::serialize(&from_program_account)
                    .expect("failed serialization of from program address")
            )
            .await
            .is_ok()
            && <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                &mock_storage,
                to_account.owner_address().to_full_string(),
                bincode::serialize(&to_account).expect("failed serialization of to address")
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
                )
                .zip(
                    account_cache
                        .send_message(AccountCacheMessage::Write {
                            account: to_account.clone(),
                            who: ActorType::AccountCache,
                            location: "bridge_in_event test".into()
                        })
                        .ok()
                ))
            .is_some()
    );

    const TRANSFER_AMOUNT: u64 = 1;
    let send_transaction = Transaction::test_send(
        TRANSFER_AMOUNT,
        from_account_address,
        from_program_address,
        to_account_address,
    );
    let res = Batcher::add_transaction_to_account(batcher, send_transaction).await;

    let from_account_with_updates = get_account(from_account_address, ActorType::AccountCache)
        .await
        .expect("could not find from account");
    let to_account_with_updates = get_account(to_account_address, ActorType::AccountCache)
        .await
        .expect("could not find to account");

    assert!(res.is_ok());
    assert_eq!(
        from_account_with_updates.balance(&from_program_address),
        U256::from(from_account.balance(&from_program_address) - TRANSFER_AMOUNT)
    );
    assert_eq!(
        to_account_with_updates.balance(&from_program_address),
        U256::from(TRANSFER_AMOUNT)
    );
    batcher_ref.stop_and_wait(None, None).await.ok();
    account_cache_ref.stop_and_wait(None, None).await.ok();
    scheduler_ref.stop_and_wait(None, None).await.ok();
    pending_transaction_ref.stop_and_wait(None, None).await.ok();
    std::thread::sleep(std::time::Duration::from_secs(5));
}
