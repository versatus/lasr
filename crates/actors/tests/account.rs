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
use lasr_messages::{
    AccountCacheMessage, ActorName, ActorType, BatcherMessage, PendingTransactionMessage,
    SchedulerMessage,
};
use lasr_types::{
    Account, AccountBuilder, AccountType, Address, AddressOrNamespace, ArbitraryData,
    BurnInstructionBuilder, CreateInstructionBuilder, Inputs, Metadata, MockPersistenceStore,
    Namespace, OutputsBuilder, PayloadBuilder, PersistenceStore, RecoverableSignature, Status,
    TokenBuilder, TokenDistributionBuilder, TokenUpdateBuilder, TokenUpdateField, Transaction,
    TransactionType, TransferInstructionBuilder, UpdateInstructionBuilder, U256,
};

use eigenda_client::proof::BlobVerificationProof;
use futures::TryFutureExt;
use ractor::{
    concurrency::{JoinHandle, OneshotReceiver},
    Actor, ActorRef,
};
use serial_test::serial;
use tokio::sync::{mpsc, Mutex};

/// This is an account with nothing in it, with `Address([1; 20])`.
pub fn test_default_user_account() -> Account {
    const RECEIVER_ADDRESS: [u8; 20] = [1; 20];
    Account::new(
        AccountType::User,
        None,
        Address::new(RECEIVER_ADDRESS),
        None,
    )
}

pub fn receiver_test_account() -> Account {
    test_default_user_account()
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

// TEST TRANSACTIONS
/// A test bridge in transaction.
fn test_bridge_in(
    amount: u64,
    nonce: crate::U256,
    program_id: Address,
    to: Address,
) -> Transaction {
    PayloadBuilder::default()
        .transaction_type(TransactionType::BridgeIn(nonce))
        .from(to.into())
        .to(to.into())
        .program_id(program_id.into())
        .inputs(String::new())
        .op(String::new())
        .value(crate::U256::from(amount))
        .nonce(nonce)
        .build()
        .expect("failed to build payload")
        .into()
}
/// A test send transaction.
fn test_send(
    amount: u64,
    from: Address,
    nonce: crate::U256,
    program_id: Address,
    to: Address,
) -> Transaction {
    PayloadBuilder::default()
        .transaction_type(TransactionType::Send(nonce))
        .from(from.into())
        .to(to.into())
        .program_id(program_id.into())
        .inputs(String::new())
        .op(String::new())
        .value(crate::U256::from(amount))
        .nonce(nonce)
        .build()
        .expect("failed to build payload")
        .into()
}
fn test_register_program(nonce: crate::U256, from: Address, program_id: Address) -> Transaction {
    let payload = PayloadBuilder::default()
        .transaction_type(TransactionType::RegisterProgram(nonce))
        .from(from.into())
        .to(from.into())
        .program_id(program_id.into())
        .inputs(String::from("{ \"contentId\": \"test\"}"))
        .op(String::new())
        .value(crate::U256::from(0))
        .nonce(nonce)
        .build()
        .expect("failed to build payload");

    let msg = secp256k1::Message::from_digest_slice(&payload.hash())
        .expect("failed to create Message from payload");

    let secp = secp256k1::Secp256k1::new();
    let keypair = secp.generate_keypair(&mut secp256k1::rand::rngs::OsRng);

    let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, &keypair.0).into();

    (payload, sig.clone()).into()
}
fn test_call(nonce: crate::U256, from: Address, to: Address, program_id: Address) -> Transaction {
    PayloadBuilder::default()
        .transaction_type(TransactionType::Call(nonce))
        .from(from.into())
        .to(to.into())
        .program_id(program_id.into())
        .inputs(String::new())
        .op(String::new())
        .value(crate::U256::from(0))
        .nonce(nonce)
        .build()
        .expect("failed to build payload")
        .into()
}

/// Everything needed to run minimal tests for a node. This excludes most things
/// that use JRPC, IPFS, and compute related components.
pub struct MinimalNode {
    /// Receiver end of the channel must outlive the function if it's to be used
    /// in future tests that require it.
    _batcher_rx: mpsc::Receiver<OneshotReceiver<(String, BlobVerificationProof)>>,
    pub batcher: Arc<Mutex<Batcher>>,
    pub batcher_actor: (ActorRef<BatcherMessage>, JoinHandle<()>),

    /// An in-memory flavor of `PersistenceStore`, i.e. `HashMap`.
    pub mock_storage: MockPersistenceStore<String, Vec<u8>>,
    pub account_cache_actor: (ActorRef<AccountCacheMessage>, JoinHandle<()>),

    pub scheduler_actor: (ActorRef<SchedulerMessage>, JoinHandle<()>),

    pub pending_transaction_actor: (ActorRef<PendingTransactionMessage>, JoinHandle<()>),
}
impl MinimalNode {
    /// Create a new `MinimalNode` !!
    ///
    /// Example:
    /// ```rust, ignore
    /// #[cfg(test)]
    /// mod my_node_tests {
    ///     use super::MinimalNode;
    ///     use futures::TryFutureExt;
    ///     
    ///     #[test]
    ///     fn my_node_test() {
    ///         MinimalNode::new().and_then(|node| async move { ... }).await.unwrap();
    ///     }
    /// }
    /// ```
    async fn new() -> anyhow::Result<Self> {
        // Channel is not used, but is necessary to construct a `Batcher` and cannot be zero.
        const CHANNEL_BUFFER: usize = 1;
        let (receivers_thread_tx, _batcher_rx) = mpsc::channel(CHANNEL_BUFFER);
        let batcher = Arc::new(Mutex::new(Batcher::new(receivers_thread_tx)));
        let batcher_actor = BatcherActor::new();
        let batcher_actor =
            Actor::spawn(Some(batcher_actor.name()), batcher_actor, batcher.clone()).await?;

        let mock_storage =
            <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::new().await?;
        let account_cache_actor = AccountCacheActor::new();
        let account_cache_actor = Actor::spawn(
            Some(account_cache_actor.name()),
            account_cache_actor,
            mock_storage.clone(),
        )
        .await?;

        let scheduler_actor = TaskScheduler::new();
        let scheduler_actor =
            Actor::spawn(Some(scheduler_actor.name()), scheduler_actor, ()).await?;

        let pending_transaction_actor = PendingTransactionActor::new();
        let pending_transaction_actor = Actor::spawn(
            Some(pending_transaction_actor.name()),
            pending_transaction_actor,
            (),
        )
        .await?;

        Ok(MinimalNode {
            _batcher_rx,
            batcher,
            batcher_actor,
            mock_storage,
            account_cache_actor,
            scheduler_actor,
            pending_transaction_actor,
        })
    }

    /// Shutdown the node processes and wait.
    async fn shutdown_and_wait(self) -> anyhow::Result<()> {
        self.batcher_actor.0.stop_and_wait(None, None).await.ok();
        self.account_cache_actor
            .0
            .stop_and_wait(None, None)
            .await
            .ok();
        self.scheduler_actor.0.stop_and_wait(None, None).await.ok();
        self.pending_transaction_actor
            .0
            .stop_and_wait(None, None)
            .await
            .ok();
        std::thread::sleep(std::time::Duration::from_secs(5));
        Ok(())
    }
}

#[serial]
#[tokio::test]
async fn bridge_in_event() {
    MinimalNode::new()
        .and_then(|node| async move {
            // Insert account into storage & account cache.
            let to_account = receiver_test_account();
            let to_account_address = to_account.owner_address();
            assert!(
                <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                    &node.mock_storage,
                    to_account.owner_address().to_full_string(),
                    bincode::serialize(&to_account).expect("failed serialization of to account")
                )
                .await
                .is_ok()
            );
            assert!(get_actor_ref::<AccountCacheMessage, AccountCacheError>(
                ActorType::AccountCache
            )
            .and_then(|account_cache| account_cache
                .send_message(AccountCacheMessage::Write {
                    account: to_account.clone(),
                    who: ActorType::AccountCache,
                    location: "bridge_in_event test".into()
                })
                .ok())
            .is_some());

            const BRIDGE_IN_AMOUNT: u64 = 1;
            let program_id = ETH_ADDR;
            let bridge_in_transaction = test_bridge_in(
                BRIDGE_IN_AMOUNT,
                to_account.nonce(),
                program_id,
                to_account_address,
            );
            let res =
                Batcher::add_transaction_to_account(node.batcher.clone(), bridge_in_transaction)
                    .await;

            let to_account_with_updates = get_account(to_account_address, ActorType::AccountCache)
                .await
                .expect("could not find to account");

            assert!(res.is_ok());
            assert_eq!(
                to_account_with_updates.balance(&program_id),
                U256::from(BRIDGE_IN_AMOUNT)
            );

            MinimalNode::shutdown_and_wait(node).await
        })
        .await
        .unwrap();
}

#[serial]
#[tokio::test]
async fn send_event() {
    MinimalNode::new()
        .and_then(|node| async move {
            // Insert accounts into storage & account cache.
            // This simulates an already registered program.
            let (from_account, from_program_account) = sender_test_account_pair();
            let from_account_address = from_account.owner_address();
            let AccountType::Program(from_program_address) = from_program_account.account_type()
            else {
                panic!("from_program_account is not a program account.")
            };
            let to_account = receiver_test_account();
            let to_account_address = to_account.owner_address();
            assert!(
                <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                    &node.mock_storage,
                    from_account.owner_address().to_full_string(),
                    bincode::serialize(&from_account)
                        .expect("failed serialization of from account")
                )
                .await
                .is_ok()
                    && <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                        &node.mock_storage,
                        from_program_address.to_full_string(),
                        bincode::serialize(&from_program_account)
                            .expect("failed serialization of from program account")
                    )
                    .await
                    .is_ok()
                    && <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                        &node.mock_storage,
                        to_account.owner_address().to_full_string(),
                        bincode::serialize(&to_account)
                            .expect("failed serialization of to account")
                    )
                    .await
                    .is_ok()
            );
            assert!(get_actor_ref::<AccountCacheMessage, AccountCacheError>(
                ActorType::AccountCache
            )
            .and_then(|account_cache| account_cache
                .send_message(AccountCacheMessage::Write {
                    account: from_account.clone(),
                    who: ActorType::AccountCache,
                    location: "send_event test".into(),
                })
                .ok()
                .zip(
                    account_cache
                        .send_message(AccountCacheMessage::Write {
                            account: from_program_account,
                            who: ActorType::AccountCache,
                            location: "send_event test".into()
                        })
                        .ok()
                )
                .zip(
                    account_cache
                        .send_message(AccountCacheMessage::Write {
                            account: to_account.clone(),
                            who: ActorType::AccountCache,
                            location: "send_event test".into()
                        })
                        .ok()
                ))
            .is_some());

            const TRANSFER_AMOUNT: u64 = 1;
            let send_transaction = test_send(
                TRANSFER_AMOUNT,
                from_account_address,
                from_account.nonce(),
                from_program_address,
                to_account_address,
            );
            let res =
                Batcher::add_transaction_to_account(node.batcher.clone(), send_transaction).await;

            let from_account_with_updates =
                get_account(from_account_address, ActorType::AccountCache)
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

            MinimalNode::shutdown_and_wait(node).await
        })
        .await
        .unwrap();
}

#[serial]
#[tokio::test]
async fn register_program_event() {
    MinimalNode::new()
        .and_then(|node| async move {
            // Insert account into storage & account cache.
            let from_account = test_default_user_account();
            let from_account_address = from_account.owner_address();
            assert!(
                <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                    &node.mock_storage,
                    from_account.owner_address().to_full_string(),
                    bincode::serialize(&from_account).expect("failed serialization of to account")
                )
                .await
                .is_ok()
            );
            assert!(get_actor_ref::<AccountCacheMessage, AccountCacheError>(
                ActorType::AccountCache
            )
            .and_then(|account_cache| account_cache
                .send_message(AccountCacheMessage::Write {
                    account: from_account.clone(),
                    who: ActorType::AccountCache,
                    location: "register_program_event test".into()
                })
                .ok())
            .is_some());

            const PROGRAM_ID: [u8; 20] = [2; 20];
            const CONTENT_ID: &str = "test";
            let program_id = Address::new(PROGRAM_ID);
            let register_program_transaction =
                test_register_program(from_account.nonce(), from_account_address, program_id);
            // The initial program_id is hashed with the transaction before being added to the cache
            // so here we re-create that bit so we have the correct address to the check the cache.
            let finalized_program_id = lasr_contract::create_program_id(
                CONTENT_ID.to_string(),
                &register_program_transaction,
            )
            .expect("failed to generate finalized program id");

            let registration_successful = Batcher::apply_program_registration(
                node.batcher.clone(),
                register_program_transaction,
            )
            .await
            .is_ok();
            let program_is_registered = get_account(finalized_program_id, ActorType::AccountCache)
                .await
                .is_some();

            assert!(registration_successful);
            assert!(program_is_registered);

            MinimalNode::shutdown_and_wait(node).await
        })
        .await
        .unwrap();
}

#[serial]
#[tokio::test]
async fn call_create_event() {
    MinimalNode::new()
        .and_then(|node| async move {
            // Insert accounts into storage & account cache.
            // This simulates an already registered program.
            let (from_account, from_program_account) = sender_test_account_pair();
            let from_account_address = from_account.owner_address();
            let AccountType::Program(from_program_address) = from_program_account.account_type()
            else {
                panic!("from_program_account is not a program account.")
            };
            assert!(
                <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                    &node.mock_storage,
                    from_account_address.to_full_string(),
                    bincode::serialize(&from_account).expect("failed serialization of to account")
                )
                .await
                .is_ok()
                    && <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                        &node.mock_storage,
                        from_program_address.to_full_string(),
                        bincode::serialize(&from_program_account)
                            .expect("failed serialization of from program account")
                    )
                    .await
                    .is_ok()
            );
            assert!(get_actor_ref::<AccountCacheMessage, AccountCacheError>(
                ActorType::AccountCache
            )
            .and_then(|account_cache| account_cache
                .send_message(AccountCacheMessage::Write {
                    account: from_account.clone(),
                    who: ActorType::AccountCache,
                    location: "call_create_event test".into()
                })
                .ok()
                .zip(
                    account_cache
                        .send_message(AccountCacheMessage::Write {
                            account: from_program_account,
                            who: ActorType::AccountCache,
                            location: "call_create_event test".into()
                        })
                        .ok()
                ))
            .is_some());

            const TOKEN_AMOUNT: u64 = 1;
            let create_transaction = test_call(
                from_account.nonce(),
                from_account_address,
                from_account_address,
                from_program_address,
            );
            let program_address = AddressOrNamespace::Address(from_program_address);
            let outputs = OutputsBuilder::new()
                .add_instruction(lasr_types::Instruction::Create(
                    CreateInstructionBuilder::new()
                        .program_namespace(program_address.clone())
                        .program_id(program_address.clone())
                        .program_owner(from_account.owner_address())
                        .total_supply(U256::from(100))
                        .initialized_supply(U256::from(100))
                        .add_token_distribution(
                            TokenDistributionBuilder::new()
                                .program_id(program_address)
                                .to(AddressOrNamespace::Address(from_account.owner_address()))
                                .amount(U256::from(TOKEN_AMOUNT))
                                .build()
                                .expect("failed to create distribution"),
                        )
                        .build()
                        .expect("failed to build create instruction"),
                ))
                .inputs(Inputs::default())
                .build()
                .expect("failed to build outputs");

            let res = Batcher::apply_instructions_to_accounts(
                node.batcher.clone(),
                create_transaction,
                outputs,
            )
            .await;

            let from_account_with_updates =
                get_account(from_account.owner_address(), ActorType::AccountCache)
                    .await
                    .expect("could not find from account");

            assert!(res.is_ok());
            // TODO: assert other expectations of token creation
            assert_eq!(
                from_account_with_updates.balance(&from_program_address),
                from_account.balance(&from_program_address) + TOKEN_AMOUNT
            );

            MinimalNode::shutdown_and_wait(node).await
        })
        .await
        .unwrap();
}

#[serial]
#[tokio::test]
async fn call_update_event() {
    MinimalNode::new()
        .and_then(|node| async move {
            // Insert accounts into storage & account cache.
            // This simulates an already registered program.
            let (from_account, from_program_account) = sender_test_account_pair();
            let from_account_address = from_account.owner_address();
            let AccountType::Program(from_program_address) = from_program_account.account_type()
            else {
                panic!("from_program_account is not a program account.")
            };
            assert!(
                <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                    &node.mock_storage,
                    from_account_address.to_full_string(),
                    bincode::serialize(&from_account).expect("failed serialization of to account")
                )
                .await
                .is_ok()
                    && <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                        &node.mock_storage,
                        from_program_address.to_full_string(),
                        bincode::serialize(&from_program_account)
                            .expect("failed serialization of from program account")
                    )
                    .await
                    .is_ok()
            );
            assert!(get_actor_ref::<AccountCacheMessage, AccountCacheError>(
                ActorType::AccountCache
            )
            .and_then(|account_cache| account_cache
                .send_message(AccountCacheMessage::Write {
                    account: from_account.clone(),
                    who: ActorType::AccountCache,
                    location: "call_update_event test".into()
                })
                .ok()
                .zip(
                    account_cache
                        .send_message(AccountCacheMessage::Write {
                            account: from_program_account,
                            who: ActorType::AccountCache,
                            location: "call_update_event test".into()
                        })
                        .ok()
                ))
            .is_some());

            let update_transaction = test_call(
                from_account.nonce(),
                from_account_address,
                from_account_address,
                from_program_address,
            );
            let program_address = AddressOrNamespace::Address(from_program_address);
            let outputs = OutputsBuilder::new()
                .add_instruction(lasr_types::Instruction::Update(
                    UpdateInstructionBuilder::new()
                        .add_update(lasr_types::TokenOrProgramUpdate::TokenUpdate(
                            TokenUpdateBuilder::new()
                                .account(AddressOrNamespace::Address(from_account_address))
                                .token(program_address)
                                .add_update(TokenUpdateField::default())
                                .build()
                                .expect("failed to build token update"),
                        ))
                        .build(),
                ))
                .inputs(Inputs::default())
                .build()
                .expect("failed to build outputs");

            let res = Batcher::apply_instructions_to_accounts(
                node.batcher.clone(),
                update_transaction,
                outputs,
            )
            .await;

            let from_account_with_updates =
                get_account(from_account.owner_address(), ActorType::AccountCache)
                    .await
                    .expect("could not find from account");

            assert!(res.is_ok());
            // TODO: assert other expectations of token & program updates
            assert!(from_account_with_updates
                .programs()
                .get(&from_program_address)
                .unwrap()
                .metadata()
                .get("some") // TODO: create macro for producing test types
                .is_some());

            MinimalNode::shutdown_and_wait(node).await
        })
        .await
        .unwrap();
}

#[serial]
#[tokio::test]
async fn call_transfer_event() {
    MinimalNode::new()
        .and_then(|node| async move {
            // Insert accounts into storage & account cache.
            // This simulates an already registered program.
            let (from_account, from_program_account) = sender_test_account_pair();
            let from_account_address = from_account.owner_address();
            let AccountType::Program(from_program_address) = from_program_account.account_type()
            else {
                panic!("from_program_account is not a program account.")
            };
            let to_account = receiver_test_account();
            let to_account_address = to_account.owner_address();
            assert!(
                <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                    &node.mock_storage,
                    from_account_address.to_full_string(),
                    bincode::serialize(&from_account).expect("failed serialization of to account")
                )
                .await
                .is_ok()
                    && <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                        &node.mock_storage,
                        from_program_address.to_full_string(),
                        bincode::serialize(&from_program_account)
                            .expect("failed serialization of from program account")
                    )
                    .await
                    .is_ok()
                    && <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                        &node.mock_storage,
                        to_account_address.to_full_string(),
                        bincode::serialize(&to_account)
                            .expect("failed serialization of to account")
                    )
                    .await
                    .is_ok()
            );
            assert!(get_actor_ref::<AccountCacheMessage, AccountCacheError>(
                ActorType::AccountCache
            )
            .and_then(|account_cache| account_cache
                .send_message(AccountCacheMessage::Write {
                    account: from_account.clone(),
                    who: ActorType::AccountCache,
                    location: "call_transfer_event test".into()
                })
                .ok()
                .zip(
                    account_cache
                        .send_message(AccountCacheMessage::Write {
                            account: from_program_account,
                            who: ActorType::AccountCache,
                            location: "call_transfer_event test".into()
                        })
                        .ok()
                )
                .zip(
                    account_cache
                        .send_message(AccountCacheMessage::Write {
                            account: to_account.clone(),
                            who: ActorType::AccountCache,
                            location: "call_transfer_event".into()
                        })
                        .ok()
                ))
            .is_some());

            const TRANSFER_AMOUNT: u64 = 1;
            let update_transaction = test_call(
                from_account.nonce(),
                from_account_address,
                to_account_address,
                from_program_address,
            );
            let outputs = OutputsBuilder::new()
                .add_instruction(lasr_types::Instruction::Transfer(
                    TransferInstructionBuilder::new()
                        .token(from_program_address)
                        .from(AddressOrNamespace::Address(from_account_address))
                        .to(AddressOrNamespace::Address(to_account_address))
                        .amount(U256::from(TRANSFER_AMOUNT))
                        .build()
                        .expect("failed to build transfer instruction"),
                ))
                .inputs(Inputs::default())
                .build()
                .expect("failed to build outputs");

            let res = Batcher::apply_instructions_to_accounts(
                node.batcher.clone(),
                update_transaction,
                outputs,
            )
            .await;

            let from_account_with_updates =
                get_account(from_account_address, ActorType::AccountCache)
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

            MinimalNode::shutdown_and_wait(node).await
        })
        .await
        .unwrap();
}

#[serial]
#[tokio::test]
async fn call_burn_event() {
    MinimalNode::new()
        .and_then(|node| async move {
            // Insert accounts into storage & account cache.
            // This simulates an already registered program.
            let (from_account, from_program_account) = sender_test_account_pair();
            let from_account_address = from_account.owner_address();
            let AccountType::Program(from_program_address) = from_program_account.account_type()
            else {
                panic!("from_program_account is not a program account.")
            };
            assert!(
                <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                    &node.mock_storage,
                    from_account_address.to_full_string(),
                    bincode::serialize(&from_account).expect("failed serialization of to account")
                )
                .await
                .is_ok()
                    && <MockPersistenceStore<String, Vec<u8>> as PersistenceStore>::put(
                        &node.mock_storage,
                        from_program_address.to_full_string(),
                        bincode::serialize(&from_program_account)
                            .expect("failed serialization of from program account")
                    )
                    .await
                    .is_ok()
            );
            assert!(get_actor_ref::<AccountCacheMessage, AccountCacheError>(
                ActorType::AccountCache
            )
            .and_then(|account_cache| account_cache
                .send_message(AccountCacheMessage::Write {
                    account: from_account.clone(),
                    who: ActorType::AccountCache,
                    location: "call_burn_event test".into()
                })
                .ok()
                .zip(
                    account_cache
                        .send_message(AccountCacheMessage::Write {
                            account: from_program_account,
                            who: ActorType::AccountCache,
                            location: "call_burn_event test".into()
                        })
                        .ok()
                ))
            .is_some());

            const TOKEN_AMOUNT: u64 = 1;
            let burn_transaction = test_call(
                from_account.nonce(),
                from_account_address,
                from_account_address,
                from_program_address,
            );
            let program_address = AddressOrNamespace::Address(from_program_address);
            let outputs = OutputsBuilder::new()
                .add_instruction(lasr_types::Instruction::Burn(
                    BurnInstructionBuilder::new()
                        .caller(from_account_address)
                        .program_id(program_address.clone())
                        .token(from_program_address)
                        .from(AddressOrNamespace::Address(from_account_address))
                        .amount(U256::from(TOKEN_AMOUNT))
                        .build()
                        .expect("failed to build create instruction"),
                ))
                .inputs(Inputs::default())
                .build()
                .expect("failed to build outputs");

            let res = Batcher::apply_instructions_to_accounts(
                node.batcher.clone(),
                burn_transaction,
                outputs,
            )
            .await;

            let from_account_with_updates =
                get_account(from_account.owner_address(), ActorType::AccountCache)
                    .await
                    .expect("could not find from account");

            assert!(res.is_ok());
            // TODO: assert other expectations of token burn
            assert_eq!(
                from_account_with_updates.balance(&from_program_address),
                from_account.balance(&from_program_address) - TOKEN_AMOUNT
            );

            MinimalNode::shutdown_and_wait(node).await
        })
        .await
        .unwrap();
}
