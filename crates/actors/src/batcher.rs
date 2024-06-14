#![allow(unused)]
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    error::Error as StdError,
    fmt::{Debug, Display},
    sync::Arc,
};

use async_trait::async_trait;
use eigenda_client::{batch, proof::BlobVerificationProof, response::BlobResponse};
use ethereum_types::H256;
use flate2::{
    write::{ZlibDecoder, ZlibEncoder},
    Compression,
};
use futures::{
    future::BoxFuture,
    stream::{FuturesUnordered, StreamExt},
    FutureExt,
};
use ractor::{
    concurrency::{oneshot, OneshotReceiver},
    errors::MessagingErr,
    factory::CustomHashFunction,
    pg::GroupChangeMessage,
    Actor, ActorCell, ActorProcessingErr, ActorRef, SupervisionEvent,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha3::{Digest, Keccak256};
use std::io::Write;
use thiserror::Error;
use tikv_client::RawClient as TikvClient;
use tokio::{
    sync::{
        mpsc::{Receiver, Sender, UnboundedSender},
        Mutex,
    },
    task::JoinHandle,
};
use web3::types::BlockNumber;

use crate::{
    account_cache, get_account, get_actor_ref, handle_actor_response, process_group_changed,
    AccountCacheError, ActorExt, Coerce, DaClientError, EoClientError, PendingTransactionError,
    SchedulerError, StaticFuture, UnorderedFuturePool,
};
use lasr_messages::{
    AccountCacheMessage, ActorName, ActorType, BatcherMessage, DaClientMessage, EoMessage,
    PendingTransactionMessage, SchedulerMessage, SupervisorType,
};

use lasr_contract::create_program_id;

use lasr_types::{
    Account, AccountBuilder, AccountType, Address, AddressOrNamespace, ArbitraryData,
    BurnInstruction, ContractLogType, CreateInstruction, Instruction, Metadata, MetadataValue,
    Namespace, Outputs, ProgramAccount, ProgramUpdate, TokenDistribution, TokenOrProgramUpdate,
    TokenUpdate, Transaction, TransactionType, TransferInstruction, UpdateInstruction, U256,
};

use derive_builder::Builder;

pub const VERSE_ADDR: Address = Address::verse_addr();
pub const ETH_ADDR: Address = Address::eth_addr();
// const BATCH_INTERVAL: u64 = 180;
pub type PendingReceivers = FuturesUnordered<OneshotReceiver<(String, BlobVerificationProof)>>;

#[derive(Debug, Error, Default)]
pub enum BatcherError {
    #[error(transparent)]
    AccountCacheMessage(#[from] MessagingErr<AccountCacheMessage>),

    #[error(transparent)]
    SchedulerMessage(#[from] MessagingErr<SchedulerMessage>),

    #[error(transparent)]
    PendingTransactionMessage(#[from] MessagingErr<PendingTransactionMessage>),

    #[error(transparent)]
    BatcherMessage(#[from] MessagingErr<BatcherMessage>),

    #[error(transparent)]
    Stdio(#[from] std::io::Error),

    #[error("{msg}")]
    FailedTransaction { msg: String, txn: Box<Transaction> },

    #[default]
    #[error("failed to acquire BatcherActor from registry")]
    RactorRegistryError,

    #[error("{0}")]
    Custom(String),
}

#[derive(Clone, Debug, Default)]
pub struct BatcherActor {
    future_pool: UnorderedFuturePool<StaticFuture<Result<(), BatcherError>>>,
}
impl BatcherActor {
    pub fn new() -> Self {
        Self {
            future_pool: Arc::new(Mutex::new(FuturesUnordered::new())),
        }
    }
}
impl ActorName for BatcherActor {
    fn name(&self) -> ractor::ActorName {
        ActorType::Batcher.to_string()
    }
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize, Default)]
pub struct Batch {
    transactions: HashMap<String, Transaction>,
    accounts: HashMap<String, Account>,
}

// Structure for persistence store `Account` values
#[derive(Debug, Hash, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountValue {
    pub account: Account,
}

// // Structure for persistence store `Transaction` values
// #[derive(Debug, Hash, Clone, Serialize, Deserialize, PartialEq, Eq)]
// pub struct TransactionValue {
//     transaction: Transaction,
// }

impl Batch {
    pub fn new() -> Self {
        Self {
            transactions: HashMap::new(),
            accounts: HashMap::new(),
        }
    }

    pub fn empty(&self) -> bool {
        self.transactions().is_empty() && self.accounts().is_empty()
    }

    pub fn get_user_account(&self, address: Address) -> Option<Account> {
        if let Some(ua) = self.accounts().get(&address.to_full_string()) {
            return Some(ua.clone());
        }

        None
    }

    pub fn get_transaction(&self, id: String) -> Option<Transaction> {
        if let Some(transaction) = self.transactions().get(&id) {
            return Some(transaction.clone());
        }

        None
    }

    pub fn get_program_account(&self, account_type: AccountType) -> Option<Account> {
        if let AccountType::Program(program_address) = account_type {
            if let Some(program_account) = self.accounts().get(&program_address.to_full_string()) {
                return Some(program_account.clone());
            }
        }

        None
    }

    pub(super) fn serialize_batch(&self) -> Option<Vec<u8>> {
        Some(
            serde_json::to_string(&self)
                .typecast()
                .log_err(|e| {
                    BatcherError::Custom(format!(
                        "ERROR: failed to serialize batch to json string: {e:?}"
                    ))
                })?
                .as_bytes()
                .to_vec(),
        )
    }

    pub(super) fn deserialize_batch(bytes: Vec<u8>) -> Option<Self> {
        let decompressed = Batch::decompress_batch(bytes)?;
        serde_json::from_str(&String::from_utf8_lossy(&decompressed))
            .typecast()
            .log_err(|e| BatcherError::Custom(format!("ERROR: failed to deserialize batch: {e:?}")))
    }

    pub(super) fn compress_batch(&self) -> Option<Vec<u8>> {
        if let Some(serialized_batch) = &self.serialize_batch() {
            let mut compressor = ZlibEncoder::new(Vec::new(), Compression::best());
            compressor
                .write_all(serialized_batch)
                .typecast()
                .log_err(|e| BatcherError::Custom(e.to_string()));
            return compressor
                .finish()
                .typecast()
                .log_err(|e| BatcherError::Custom(e.to_string()));
        }
        None
    }

    pub(super) fn decompress_batch(bytes: Vec<u8>) -> Option<Vec<u8>> {
        let mut decompressor = ZlibDecoder::new(Vec::new());
        decompressor.write_all(&bytes[..]).typecast().log_err(|e| {
            BatcherError::Custom(format!(
                "Batcher Error: failed to write bytes to decoder: {e:?}"
            ))
        })?;
        decompressor.finish().typecast().log_err(|e| {
            BatcherError::Custom(format!(
                "Batcher Error: decoder failed to finalize decompressed batch: {e:?}"
            ))
        })
    }

    pub fn encode_batch(&self) -> Option<String> {
        if let Some(compressed_batch) = &self.compress_batch() {
            let encoded =
                base64::encode(kzgpad_rs::convert_by_padding_empty_byte(compressed_batch));
            tracing::info!("encoded batch: {:?}", &encoded);
            return Some(encoded);
        }
        None
    }

    pub fn decode_batch(batch: &str) -> Option<Self> {
        Self::deserialize_batch(kzgpad_rs::remove_empty_byte_from_padded_bytes(
            &base64::decode(batch).typecast().log_err(|e| {
                BatcherError::Custom(format!(
                    "Batcher Error: failed to decode batch data to base64: {e:?}"
                ))
            })?,
        ))
    }

    pub(super) fn check_size(&self) -> Option<usize> {
        let encoded = self.encode_batch()?;

        Some(encoded.as_bytes().len())
    }

    pub(super) fn transaction_would_exceed_capacity(
        &self,
        transaction: Transaction,
    ) -> Option<bool> {
        let mut test_batch = self.clone();
        test_batch
            .transactions
            .insert(transaction.hash_string(), transaction.clone());
        test_batch.at_capacity()
    }

    pub(super) fn account_would_exceed_capacity(&self, account: Account) -> Option<bool> {
        let mut test_batch = self.clone();
        test_batch
            .accounts
            .insert(account.owner_address().to_full_string(), account);
        test_batch.at_capacity()
    }

    pub(super) fn at_capacity(&self) -> Option<bool> {
        Some(self.check_size()? >= 512 * 1024)
    }

    pub fn insert_transaction(&mut self, transaction: Transaction) -> Result<(), BatcherError> {
        if self
            .transaction_would_exceed_capacity(transaction.clone())
            .is_some_and(|at_cap| !at_cap)
        {
            self.transactions
                .insert(transaction.hash_string(), transaction.clone());
            return Ok(());
        }

        Err(BatcherError::Custom("transactions at capacity".to_string()))
    }

    pub fn insert_account(&mut self, account: Account) -> Result<(), BatcherError> {
        if self
            .clone()
            .account_would_exceed_capacity(account.clone())
            .is_some_and(|at_cap| !at_cap)
        {
            tracing::info!("inserting account into batch");
            match account.account_type() {
                AccountType::Program(address) => {
                    self.accounts
                        .insert(address.clone().to_full_string(), account.clone());
                }
                AccountType::User => {
                    self.accounts
                        .insert(account.owner_address().to_full_string(), account.clone());
                }
            }
            return Ok(());
        }

        Err(BatcherError::Custom("accounts at capacity".to_string()))
    }

    pub fn transactions(&self) -> HashMap<String, Transaction> {
        self.transactions.clone()
    }

    pub fn accounts(&self) -> HashMap<String, Account> {
        self.accounts.clone()
    }
}

pub struct Batcher {
    parent: Batch,
    children: VecDeque<Batch>,
    cache: HashMap<String /* request_id*/, Batch>,
    receiver_thread_tx: Sender<OneshotReceiver<(String, BlobVerificationProof)>>,
}

impl Batcher {
    pub async fn run_receivers(
        mut receiver: Receiver<OneshotReceiver<(String, BlobVerificationProof)>>,
    ) -> Result<(), BatcherError> {
        let mut pending_receivers: PendingReceivers = FuturesUnordered::new();
        println!("in run receivers");
        tracing::info!("starting batch receivers");
        loop {
            tokio::select! {
                new_pending = receiver.recv() => {
                    if let Some(pending_rx) = new_pending {
                        tracing::info!("batcher received a new receiver for a pending blob");
                        pending_receivers.push(pending_rx);
                    }
                },
                next_proof = pending_receivers.next() => {
                    if let Some(Ok((request_id, proof))) = next_proof {
                        tracing::info!("batcher received blob verification proof");
                        if let Some(batcher) = get_actor_ref::<BatcherMessage, BatcherError>(ActorType::Batcher) {
                            let message = BatcherMessage::BlobVerificationProof {
                                request_id,
                                proof
                            };

                            batcher.cast(message).typecast().log_err(|err|  {
                                BatcherError::Custom(format!("failed to cast blob verification proof: {err:?}"))
                            });
                        }
                    }
                },
            }
        }
    }

    pub fn new(
        receiver_thread_tx: Sender<OneshotReceiver<(String, BlobVerificationProof)>>,
    ) -> Self {
        Self {
            parent: Batch::new(),
            children: VecDeque::new(),
            cache: HashMap::new(),
            receiver_thread_tx,
        }
    }

    pub(super) async fn cache_account(account: &Account, location: String) {
        tracing::info!("Attempting to acquire account cache actor");
        if let Some(account_cache) =
            get_actor_ref::<AccountCacheMessage, AccountCacheError>(ActorType::AccountCache)
        {
            if let AccountType::Program(program_address) = account.account_type() {
                tracing::warn!("caching account: {}", program_address.to_full_string());
            }
            let message = AccountCacheMessage::Write {
                account: account.clone(),
                who: ActorType::Batcher,
                location,
            };
            if let Err(err) = account_cache.cast(message) {
                tracing::error!("failed to cast write message to account cache: {err:?}");
            }
        }
    }

    pub(super) async fn add_transaction_to_batch(
        batcher: Arc<Mutex<Batcher>>,
        transaction: Transaction,
    ) {
        let mut guard = batcher.lock().await;
        let mut new_batch = false;
        let mut res = guard.parent.insert_transaction(transaction.clone());
        let mut iter = guard.children.iter_mut();
        while let Err(ref mut e) = res {
            tracing::error!("{e}");
            if let Some(mut child) = iter.next() {
                res = child.insert_transaction(transaction.clone());
            } else {
                new_batch = true;
            }
        }

        if new_batch {
            let mut batch = Batch::new();
            batch
                .insert_transaction(transaction)
                .typecast()
                .log_err(|e| e);
            guard.children.push_back(batch);
        }
    }

    pub(super) async fn add_account_to_batch(
        batcher: &Arc<Mutex<Batcher>>,
        account: Account,
        location: String,
    ) -> Result<(), BatcherError> {
        Batcher::cache_account(&account, location).await;
        let mut guard = batcher.lock().await;
        let mut new_batch = false;
        let mut res = guard.parent.insert_account(account.clone());
        let mut iter = guard.children.iter_mut();
        while let Err(ref mut e) = res {
            tracing::error!("{e}");
            if let Some(mut child) = iter.next() {
                res = child.insert_account(account.clone());
            } else {
                new_batch = true;
            }
        }

        if new_batch {
            let mut batch = Batch::new();
            batch.insert_account(account).typecast().log_err(|e| e);
            guard.children.push_back(batch);
        }

        Ok(())
    }

    pub(super) async fn add_transaction_to_account(
        batcher: Arc<Mutex<Batcher>>,
        transaction: Transaction,
    ) -> Result<(), BatcherError> {
        let mut batch_buffer = HashMap::new();
        tracing::warn!(
            "checking account cache for account associated with address {:?} to add transaction: {:?}",
            transaction.from(),
            transaction
        );
        let mut from_account = get_account(transaction.from(), ActorType::Batcher).await;
        let (from_account, token) = if let Some(mut account) = from_account {
            tracing::warn!("found account, token pair");
            account.increment_nonce();
            let token = account
                .apply_send_transaction(transaction.clone(), None)
                .map_err(|e| BatcherError::FailedTransaction {
                    msg: e.to_string(),
                    txn: Box::new(transaction.clone()),
                })?;
            batch_buffer.insert(transaction.from().to_full_string(), account.clone());
            (account, token)
        } else {
            if !transaction.transaction_type().is_bridge_in() {
                return Err(BatcherError::FailedTransaction {
                    msg: "sender account does not exist".to_string(),
                    txn: Box::new(transaction.clone()),
                });
            }

            tracing::warn!(
                "transaction is first for account {:?} bridge_in, building account",
                transaction.from()
            );
            let mut account = AccountBuilder::default()
                .account_type(AccountType::User)
                .program_namespace(None)
                .owner_address(transaction.from())
                .programs(BTreeMap::new())
                .nonce(U256::from(0))
                .program_account_data(ArbitraryData::new())
                .program_account_metadata(Metadata::new())
                .program_account_linked_programs(BTreeSet::new())
                .build()
                .map_err(|e| BatcherError::FailedTransaction {
                    msg: e.to_string(),
                    txn: Box::new(transaction.clone()),
                })?;

            if let Some(program_account) =
                get_account(transaction.program_id(), ActorType::Batcher).await
            {
                let token = account
                    .apply_send_transaction(transaction.clone(), Some(&program_account))
                    .map_err(|e| BatcherError::FailedTransaction {
                        msg: e.to_string(),
                        txn: Box::new(transaction.clone()),
                    })?;

                batch_buffer.insert(transaction.from().to_full_string(), account.clone());
                (account, token)
            } else {
                let token = account
                    .apply_send_transaction(transaction.clone(), None)
                    .map_err(|e| BatcherError::FailedTransaction {
                        msg: e.to_string(),
                        txn: Box::new(transaction.clone()),
                    })?;

                batch_buffer.insert(transaction.from().to_full_string(), account.clone());
                (account, token)
            }
        };

        tracing::info!(
            "applied transaction {} to account {:x}, informing scheduler",
            transaction.clone().hash_string(),
            from_account.owner_address()
        );

        if transaction.to() != transaction.from() {
            tracing::warn!(
                "checking account cache for account: {}",
                transaction.to().to_full_string()
            );
            let mut to_account = get_account(transaction.to(), ActorType::Batcher).await;
            let to_account = if let Some(mut account) = to_account {
                tracing::warn!("found `to` account: {}", transaction.to().to_full_string());
                if let Some(program_account) =
                    get_account(transaction.program_id(), ActorType::Batcher).await
                {
                    let _ =
                        account.apply_send_transaction(transaction.clone(), Some(&program_account));
                    tracing::warn!(
                        "applied send transaction, account {} now has new token",
                        account.owner_address().to_full_string()
                    );
                    tracing::warn!(
                        "token_entry: {:?}",
                        &account.programs().get(&transaction.program_id())
                    );
                    account
                } else if transaction.program_id() == ETH_ADDR {
                    tracing::warn!(
                        "applying ETH to account {}",
                        transaction.to().to_full_string()
                    );
                    let _ = account.apply_send_transaction(transaction.clone(), None);
                    account
                } else if transaction.program_id() == VERSE_ADDR {
                    tracing::warn!(
                        "applying VERSE to account {}",
                        transaction.to().to_full_string()
                    );
                    let _ = account.apply_send_transaction(transaction.clone(), None);
                    account
                } else {
                    return Err(BatcherError::FailedTransaction {
                        msg: format!(
                            "program account {} does not exist",
                            transaction.program_id().to_full_string()
                        ),
                        txn: Box::new(transaction.clone()),
                    });
                }
            } else {
                tracing::warn!(
                    "first transaction send to account {} building account",
                    transaction.to().to_full_string()
                );
                let mut account = AccountBuilder::default()
                    .account_type(AccountType::User)
                    .program_namespace(None)
                    .owner_address(transaction.to())
                    .programs(BTreeMap::new())
                    .nonce(U256::from(0))
                    .program_account_data(ArbitraryData::new())
                    .program_account_metadata(Metadata::new())
                    .program_account_linked_programs(BTreeSet::new())
                    .build()
                    .map_err(|e| BatcherError::FailedTransaction {
                        msg: e.to_string(),
                        txn: Box::new(transaction.clone()),
                    })?;

                tracing::warn!("applying transaction to `to` account");
                if let Some(program_account) =
                    get_account(transaction.program_id(), ActorType::Batcher).await
                {
                    let _ =
                        account.apply_send_transaction(transaction.clone(), Some(&program_account));
                    tracing::warn!(
                        "applied send transaction, account {} now has new token",
                        account.owner_address().to_full_string()
                    );
                    tracing::warn!(
                        "token_entry: {:?}",
                        &account.programs().get(&transaction.program_id())
                    );
                    account
                } else if transaction.program_id() == ETH_ADDR {
                    account.apply_send_transaction(transaction.clone(), None);
                    account
                } else if transaction.program_id() == VERSE_ADDR {
                    let _ = account.apply_send_transaction(transaction.clone(), None);
                    account
                } else {
                    return Err(BatcherError::FailedTransaction {
                        msg: format!(
                            "program account {} does not exist",
                            transaction.program_id().to_full_string()
                        ),
                        txn: Box::new(transaction.clone()),
                    });
                }
            };

            batch_buffer.insert(transaction.to().to_full_string(), to_account.clone());
        } else {
            let to_account = if let Some(mut account) =
                batch_buffer.get_mut(&transaction.to().to_full_string())
            {
                if let Some(program_account) =
                    get_account(transaction.program_id(), ActorType::Batcher).await
                {
                    let _ =
                        account.apply_send_transaction(transaction.clone(), Some(&program_account));
                    tracing::warn!(
                        "applied send transaction, account {} now has new token",
                        account.owner_address().to_full_string()
                    );
                    tracing::warn!(
                        "token_entry: {:?}",
                        &account.programs().get(&transaction.program_id())
                    );
                    account.clone()
                } else if transaction.program_id() == ETH_ADDR {
                    account.apply_send_transaction(transaction.clone(), None);
                    account.clone()
                } else if transaction.program_id() == VERSE_ADDR {
                    let _ = account.apply_send_transaction(transaction.clone(), None);
                    account.clone()
                } else {
                    return Err(BatcherError::FailedTransaction {
                        msg: format!(
                            "program account {} does not exist",
                            transaction.program_id().to_full_string()
                        ),
                        txn: Box::new(transaction.clone()),
                    });
                }
            } else if let Some(mut account) =
                get_account(transaction.to(), ActorType::Batcher).await
            {
                if let Some(program_account) =
                    get_account(transaction.program_id(), ActorType::Batcher).await
                {
                    let _ =
                        account.apply_send_transaction(transaction.clone(), Some(&program_account));
                    tracing::warn!(
                        "applied send transaction, account {} now has new token",
                        account.owner_address().to_full_string()
                    );
                    tracing::warn!(
                        "token_entry: {:?}",
                        &account.programs().get(&transaction.program_id())
                    );
                    account.clone()
                } else if transaction.program_id() == ETH_ADDR {
                    account.apply_send_transaction(transaction.clone(), None);
                    account.clone()
                } else if transaction.program_id() == VERSE_ADDR {
                    let _ = account.apply_send_transaction(transaction.clone(), None);
                    account.clone()
                } else {
                    return Err(BatcherError::FailedTransaction {
                        msg: format!(
                            "program account {} does not exist",
                            transaction.program_id().to_full_string()
                        ),
                        txn: Box::new(transaction.clone()),
                    });
                }
            } else {
                return Err(BatcherError::FailedTransaction {
                    msg: "account sending to itself does not exist".to_string(),
                    txn: Box::new(transaction.clone()),
                });
            };

            batch_buffer.insert(transaction.to().to_full_string(), to_account.clone());
        }

        for (_, account) in batch_buffer {
            tracing::info!("adding account to batch");
            Batcher::add_account_to_batch(
                &batcher,
                account,
                "add_transaction_to_account".to_string(),
            )
            .await
            .map_err(|e| BatcherError::FailedTransaction {
                msg: e.to_string(),
                txn: Box::new(transaction.clone()),
            })?;
        }

        tracing::info!("adding transaction to batch");
        Batcher::add_transaction_to_batch(batcher, transaction.clone()).await;

        if let Some(scheduler) =
            get_actor_ref::<SchedulerMessage, SchedulerError>(ActorType::Scheduler)
        {
            let message = SchedulerMessage::TransactionApplied {
                transaction_hash: transaction.clone().hash_string(),
                token: token.clone(),
            };

            scheduler
                .cast(message)
                .map_err(|e| BatcherError::FailedTransaction {
                    msg: e.to_string(),
                    txn: Box::new(transaction.clone()),
                })?;
        } else {
            return Err(BatcherError::FailedTransaction {
                msg: "failed to acquire SchedulerActor".to_string(),
                txn: Box::new(transaction.clone()),
            });
        }

        if let Some(pending_tx) = get_actor_ref::<PendingTransactionMessage, PendingTransactionError>(
            ActorType::PendingTransactions,
        ) {
            let message = PendingTransactionMessage::Valid {
                transaction: transaction.clone(),
                cert: None,
            };

            pending_tx
                .cast(message)
                .map_err(|e| BatcherError::FailedTransaction {
                    msg: e.to_string(),
                    txn: Box::new(transaction),
                })?;
        } else {
            return Err(BatcherError::FailedTransaction {
                msg: "failed to acquire PendingTransactionActor".to_string(),
                txn: Box::new(transaction.clone()),
            });
        }

        Ok(())
    }

    async fn get_transfer_from_account(
        transaction: &Transaction,
        from: &AddressOrNamespace,
        batch_buffer: &mut HashMap<Address, Account>,
    ) -> Result<Account, BatcherError> {
        match from {
            AddressOrNamespace::This => {
                let account_address = transaction.clone().to();
                if let Some(account) = batch_buffer.get(&account_address) {
                    Ok(account.clone())
                } else {
                    tracing::info!(
                        "requesting account: {:?}",
                        &account_address.to_full_string()
                    );
                    get_account(account_address, ActorType::Batcher)
                        .await
                        .ok_or(BatcherError::Custom(
                            "the `from` account in a transfer must exist".to_string(),
                        ))
                }
            }
            AddressOrNamespace::Address(address) => {
                if let Some(account) = batch_buffer.get(address) {
                    Ok(account.clone())
                } else {
                    tracing::info!("requesting account: {:?}", &address.to_full_string());
                    get_account(*address, ActorType::Batcher)
                        .await
                        .ok_or(BatcherError::Custom(
                            "the `from` account in a transfer must exist".to_string(),
                        ))
                }
            }
            AddressOrNamespace::Namespace(namespace) => Err(BatcherError::Custom(
                "Transfers from namespaces are not yet supported, use address for {:?} instead"
                    .to_string(),
            )),
        }
    }

    async fn get_transfer_to_account(
        transaction: &Transaction,
        to: &AddressOrNamespace,
        batch_buffer: &mut HashMap<Address, Account>,
    ) -> Option<Account> {
        match to {
            AddressOrNamespace::This => {
                let account_address = transaction.clone().to();
                if let Some(account) = batch_buffer.get(&account_address) {
                    Some(account.clone())
                } else {
                    tracing::info!(
                        "requesting account: {:?}",
                        &account_address.to_full_string()
                    );
                    get_account(account_address, ActorType::Batcher).await
                }
            }
            AddressOrNamespace::Address(address) => {
                if let Some(account) = batch_buffer.get(address) {
                    Some(account.clone())
                } else {
                    tracing::info!("requesting account: {:?}", &address.to_full_string());
                    get_account(*address, ActorType::Batcher).await
                }
            }
            AddressOrNamespace::Namespace(namespace) => None,
        }
    }

    async fn apply_transfer_from(
        transaction: &Transaction,
        transfer: &TransferInstruction,
        batch_buffer: &mut HashMap<Address, Account>,
    ) -> Result<Account, BatcherError> {
        let from = transfer.from().clone();
        tracing::warn!("instruction indicates a transfer from {:?}", &from);
        let mut account =
            Batcher::get_transfer_from_account(transaction, &from, batch_buffer).await?;
        account
            .apply_transfer_from_instruction(transfer.token(), transfer.amount(), transfer.ids())
            .map_err(|e| BatcherError::Custom(e.to_string()))?;
        Ok(account)
    }

    async fn apply_transfer_to(
        transaction: &Transaction,
        transfer: &TransferInstruction,
        batch_buffer: &mut HashMap<Address, Account>,
    ) -> Result<Account, BatcherError> {
        let to = transfer.to().clone();
        tracing::warn!("instruction indicates a transfer from {:?}", &to);
        if let Some(mut account) =
            Batcher::get_transfer_to_account(transaction, &to, batch_buffer).await
        {
            if let Some(program_account) = get_account(*transfer.token(), ActorType::Batcher).await
            {
                account
                    .apply_transfer_to_instruction(
                        transfer.token(),
                        transfer.amount(),
                        transfer.ids(),
                        Some(&program_account),
                    )
                    .map_err(|e| BatcherError::Custom(e.to_string()))?;

                Ok(account)
            } else if transfer.token() == &ETH_ADDR || transfer.token() == &VERSE_ADDR {
                account
                    .apply_transfer_to_instruction(
                        transfer.token(),
                        transfer.amount(),
                        transfer.ids(),
                        None,
                    )
                    .map_err(|e| BatcherError::Custom(e.to_string()))?;

                Ok(account)
            } else {
                return Err(BatcherError::Custom(format!(
                    "token {} program account does not exist",
                    transfer.token().to_full_string()
                )));
            }
        } else {
            match to {
                AddressOrNamespace::Address(address) => {
                    let mut account = AccountBuilder::default()
                        .account_type(AccountType::User)
                        .program_namespace(None)
                        .owner_address(address)
                        .nonce(U256::from(0))
                        .programs(BTreeMap::new())
                        .program_account_linked_programs(BTreeSet::new())
                        .program_account_metadata(Metadata::new())
                        .program_account_data(ArbitraryData::new())
                        .build().map_err(|e| {
                            BatcherError::Custom(e.to_string())
                        })?;

                    if let Some(program_account) = get_account(*transfer.token(), ActorType::Batcher).await {
                        account.apply_transfer_to_instruction(
                            transfer.token(), transfer.amount(), transfer.ids(), Some(&program_account)
                        ).map_err(|e| {
                            BatcherError::Custom(
                                e.to_string()
                            )
                        })?;

                        Ok(account)
                    } else if transfer.token() == &ETH_ADDR || transfer.token() == &VERSE_ADDR {
                        account.apply_transfer_to_instruction(
                            transfer.token(), transfer.amount(), transfer.ids(), None
                        ).map_err(|e| BatcherError::Custom(e.to_string()))?;

                        Ok(account)
                    } else {
                        return Err(BatcherError::Custom(format!("token {} program account does not exist", transfer.token().to_full_string())));
                    }
                }
                _ => {
                    Err(
                        BatcherError::Custom(
                            "When sending tokens to an account that does not exist yet, fully qualified address must be used".to_string()
                        )
                    )
                }
            }
        }
    }

    async fn apply_transfer_instruction(
        batcher: &Arc<Mutex<Batcher>>,
        transaction: &Transaction,
        transfer: &TransferInstruction,
        batch_buffer: &mut HashMap<Address, Account>,
    ) -> Result<(Account, Account), BatcherError> {
        let to = transfer.to().clone();
        let from = transfer.from().clone();
        tracing::warn!(
            "tranferring {:?} in {} from {:?} to {:?}",
            &transfer.amount(),
            &transfer.token().to_full_string(),
            &from,
            &to
        );
        let from_account =
            Batcher::apply_transfer_from(transaction, transfer, batch_buffer).await?;
        let to_account = Batcher::apply_transfer_to(transaction, transfer, batch_buffer).await?;
        Ok((from_account, to_account))
    }

    async fn apply_burn_instruction(
        transaction: &Transaction,
        burn: &BurnInstruction,
        batch_buffer: &mut HashMap<Address, Account>,
    ) -> Result<Account, BatcherError> {
        let burn_address = burn.from();
        let mut account =
            Batcher::get_transfer_from_account(transaction, burn_address, batch_buffer).await?;
        account
            .apply_burn_instruction(burn.token(), burn.amount(), burn.token_ids())
            .map_err(|e| BatcherError::Custom(e.to_string()))?;
        Ok(account)
    }

    async fn apply_distribution(
        transaction: &Transaction,
        distribution: &TokenDistribution,
        batch_buffer: &mut HashMap<Address, Account>,
    ) -> Result<Account, BatcherError> {
        let program_id = match distribution.program_id() {
            AddressOrNamespace::This => transaction.to(),
            AddressOrNamespace::Address(program_address) => *program_address,
            AddressOrNamespace::Namespace(namespace) => {
                return Err(BatcherError::Custom(
                    "Namespaces are not yet supported for token distributions".to_string(),
                ))
            }
        };
        match distribution.to() {
            AddressOrNamespace::This => {
                tracing::warn!("Distribution going to {:?}", transaction.to());
                let addr = transaction.to();
                if let Some(mut acct) = Batcher::get_transfer_to_account(
                    transaction,
                    distribution.to(),
                    batch_buffer
                ).await {
                    if let AccountType::Program(program_addr) = acct.account_type() {
                        tracing::warn!("applying token distribution to {}", program_addr.to_full_string());
                    }
                    if let Some(program_account) = get_account(program_id, ActorType::Batcher).await {
                        acct.apply_token_distribution(
                            &program_id,
                            distribution.amount(),
                            distribution.token_ids(),
                            distribution.update_fields(),
                            &program_account
                        ).map_err(|e| {
                            BatcherError::Custom(
                                e.to_string()
                            )
                        })?;
                        Ok(acct)
                    } else {
                        Err(BatcherError::Custom(format!("token {} program account does not exist", program_id.to_full_string())))
                    }
                } else {
                    let mut acct = AccountBuilder::default()
                        .account_type(AccountType::Program(transaction.to()))
                        .program_namespace(None)
                        .owner_address(transaction.from())
                        .nonce(U256::from(0))
                        .programs(BTreeMap::new())
                        .program_account_linked_programs(BTreeSet::new())
                        .program_account_metadata(Metadata::new())
                        .program_account_data(ArbitraryData::new())
                        .build().map_err(|e| BatcherError::Custom(e.to_string()))?;

                    if let Some(program_account) = get_account(program_id,ActorType::Batcher).await {
                        acct.apply_token_distribution(
                            &program_id,
                            distribution.amount(),
                            distribution.token_ids(),
                            distribution.update_fields(),
                            &program_account
                        ).map_err(|e| {
                            BatcherError::Custom(
                                e.to_string()
                            )
                        })?;

                        Ok(acct)
                    } else {
                        Err(BatcherError::Custom(format!("token {} program account does not exist", program_id.to_full_string())))
                    }
                }
            }
            AddressOrNamespace::Address(to_addr) => {
                tracing::warn!("distribution going to {}", to_addr.to_full_string());
                if let Some(mut account) = Batcher::get_transfer_to_account(
                    transaction,
                    &AddressOrNamespace::Address(*to_addr),
                    batch_buffer
                ).await {
                    if let AccountType::Program(program_addr) = account.account_type() {
                        tracing::warn!("distribution going to program account: {}", program_addr.to_full_string());
                    }
                    if let Some(program_account) = get_account(program_id,ActorType::Batcher).await {
                        account.apply_token_distribution(
                            &program_id,
                            distribution.amount(),
                            distribution.token_ids(),
                            distribution.update_fields(),
                            &program_account
                        ).map_err(|e| {
                            BatcherError::Custom(
                                e.to_string()
                            )
                        })?;

                        Ok(account)
                    } else {
                        Err(BatcherError::Custom(format!("token {} program account does not exist", program_id.to_full_string())))
                    }
                } else {
                    let mut account = AccountBuilder::default()
                        .account_type(AccountType::User)
                        .program_namespace(None)
                        .owner_address(*to_addr)
                        .nonce(U256::from(0))
                        .programs(BTreeMap::new())
                        .program_account_linked_programs(BTreeSet::new())
                        .program_account_metadata(Metadata::new())
                        .program_account_data(ArbitraryData::new())
                        .build().map_err(|e| BatcherError::Custom(e.to_string()))?;

                    if let Some(program_account) = get_account(program_id,ActorType::Batcher).await {
                        account.apply_token_distribution(
                            &program_id,
                            distribution.amount(),
                            distribution.token_ids(),
                            distribution.update_fields(),
                            &program_account
                        ).map_err(|e| {
                            BatcherError::Custom(
                                e.to_string()
                            )
                        })?;

                        Ok(account)
                    } else {
                        Err(BatcherError::Custom(format!("token {} program account does not exist", program_id.to_full_string())))
                    }
                }
            }
            AddressOrNamespace::Namespace(namespace) => {
                Err(
                    BatcherError::Custom(
                        format!("Namespaced are not yet supported for Token Distrubtion applications, use address for {:?} instead", namespace)
                    )
                )
            }
        }
    }

    async fn apply_token_update(
        transaction: &Transaction,
        token_update: &TokenUpdate,
        batch_buffer: &mut HashMap<Address, Account>,
    ) -> Result<Account, BatcherError> {
        let program_id = match token_update.token() {
            AddressOrNamespace::This => transaction.to(),
            AddressOrNamespace::Address(token_address) => *token_address,
            AddressOrNamespace::Namespace(namespace) => {
                return Err(BatcherError::Custom(
                    "Namespaces are not yet supported for token updates".to_string(),
                ))
            }
        };

        match token_update.account() {
            AddressOrNamespace::This => {
                if let Some(mut account) = batch_buffer.get_mut(&transaction.to()) {
                    if let Some(program_account) = get_account(program_id, ActorType::Batcher).await
                    {
                        account
                            .apply_token_update(
                                &program_id,
                                token_update.updates(),
                                &program_account,
                            )
                            .map_err(|e| BatcherError::Custom(e.to_string()))?;
                        return Ok(account.clone());
                    } else {
                        return Err(BatcherError::Custom(format!(
                            "token {} program account does not exist",
                            program_id.to_full_string()
                        )));
                    }
                }

                tracing::warn!(
                    "attempting to get account: {} from cache in batcher",
                    &transaction.to()
                );
                if let Some(mut account) = get_account(transaction.to(), ActorType::Batcher).await {
                    if let Some(program_account) = get_account(program_id, ActorType::Batcher).await
                    {
                        account
                            .apply_token_update(
                                &program_id,
                                token_update.updates(),
                                &program_account,
                            )
                            .map_err(|e| BatcherError::Custom(e.to_string()))?;
                        Ok(account)
                    } else {
                        Err(BatcherError::Custom(format!(
                            "token {} program account does not exist",
                            program_id.to_full_string()
                        )))
                    }
                } else {
                    Err(BatcherError::Custom(
                        "Use of `This` variant impermissible on accounts that do not exist yet"
                            .to_string(),
                    ))
                }
            }
            AddressOrNamespace::Address(address) => {
                if let Some(mut account) = batch_buffer.get_mut(address) {
                    if let Some(program_account) = get_account(program_id, ActorType::Batcher).await
                    {
                        account
                            .apply_token_update(
                                &program_id,
                                token_update.updates(),
                                &program_account,
                            )
                            .map_err(|e| BatcherError::Custom(e.to_string()))?;
                        return Ok(account.clone());
                    } else {
                        return Err(BatcherError::Custom(format!(
                            "token {} program account does not exist",
                            program_id.to_full_string()
                        )));
                    }
                }

                if let Some(mut account) = get_account(*address, ActorType::Batcher).await {
                    if let Some(program_account) = get_account(program_id, ActorType::Batcher).await
                    {
                        account
                            .apply_token_update(
                                &program_id,
                                token_update.updates(),
                                &program_account,
                            )
                            .map_err(|e| BatcherError::Custom(e.to_string()))?;
                        Ok(account)
                    } else {
                        Err(BatcherError::Custom(format!(
                            "token {} program account does not exist",
                            program_id.to_full_string()
                        )))
                    }
                } else {
                    let mut account = AccountBuilder::default()
                        .account_type(AccountType::User)
                        .program_namespace(None)
                        .owner_address(*address)
                        .nonce(U256::from(0))
                        .programs(BTreeMap::new())
                        .program_account_linked_programs(BTreeSet::new())
                        .program_account_metadata(Metadata::new())
                        .program_account_data(ArbitraryData::new())
                        .build()
                        .map_err(|e| BatcherError::Custom(e.to_string()))?;

                    if let Some(program_account) = get_account(program_id, ActorType::Batcher).await
                    {
                        account
                            .apply_token_update(
                                &program_id,
                                token_update.updates(),
                                &program_account,
                            )
                            .map_err(|e| BatcherError::Custom(e.to_string()))?;
                        Ok(account)
                    } else {
                        Err(BatcherError::Custom(format!(
                            "token {} program account does not exist",
                            program_id.to_full_string()
                        )))
                    }
                }
            }
            AddressOrNamespace::Namespace(namespace) => Err(BatcherError::Custom(
                "Namespaces are not yet enabled for applying token updates".to_string(),
            )),
        }
    }

    async fn apply_program_update(
        transaction: &Transaction,
        program_update: &ProgramUpdate,
        batch_buffer: &mut HashMap<Address, Account>,
    ) -> Result<Account, BatcherError> {
        match program_update.account() {
            AddressOrNamespace::This => {
                if let Some(mut account) = batch_buffer.get_mut(&transaction.to()) {
                    account
                        .apply_program_update(program_update)
                        .map_err(|e| BatcherError::Custom(e.to_string()))?;
                    return Ok(account.clone());
                }

                tracing::warn!(
                    "attempting to get account {} from cache in batcher.rs 832",
                    transaction.to()
                );
                if let Some(mut account) = get_account(transaction.to(), ActorType::Batcher).await {
                    account
                        .apply_program_update(program_update)
                        .map_err(|e| BatcherError::Custom(e.to_string()))?;
                    Ok(account)
                } else {
                    Err(BatcherError::Custom(
                        "Use of `This` variant impermissible on accounts that do not exist yet"
                            .to_string(),
                    ))
                }
            }
            AddressOrNamespace::Address(address) => {
                if let Some(mut account) = batch_buffer.get_mut(address) {
                    account
                        .apply_program_update(program_update)
                        .map_err(|e| BatcherError::Custom(e.to_string()))?;
                    return Ok(account.clone());
                }
                tracing::warn!(
                    "attempting to get account {} from cache in batcher.rs 852",
                    &address
                );
                if let Some(mut account) = get_account(*address, ActorType::Batcher).await {
                    account
                        .apply_program_update(program_update)
                        .map_err(|e| BatcherError::Custom(e.to_string()))?;
                    Ok(account)
                } else {
                    let mut account = AccountBuilder::default()
                        .account_type(AccountType::User)
                        .program_namespace(None)
                        .owner_address(*address)
                        .nonce(U256::from(0))
                        .programs(BTreeMap::new())
                        .program_account_linked_programs(BTreeSet::new())
                        .program_account_metadata(Metadata::new())
                        .program_account_data(ArbitraryData::new())
                        .build()
                        .map_err(|e| BatcherError::Custom(e.to_string()))?;

                    account
                        .apply_program_update(program_update)
                        .map_err(|e| BatcherError::Custom(e.to_string()))?;
                    Ok(account)
                }
            }
            AddressOrNamespace::Namespace(namespace) => Err(BatcherError::Custom(
                "Namespaces are not yet enabled for applying token updates".to_string(),
            )),
        }
    }

    async fn apply_update(
        transaction: &Transaction,
        update: &TokenOrProgramUpdate,
        batch_buffer: &mut HashMap<Address, Account>,
    ) -> Result<Account, BatcherError> {
        match update {
            TokenOrProgramUpdate::TokenUpdate(token_update) => {
                tracing::warn!("received token update: {:?}", token_update);
                Batcher::apply_token_update(transaction, token_update, batch_buffer).await
            }
            TokenOrProgramUpdate::ProgramUpdate(program_update) => {
                tracing::warn!("received program update: {:?}", &program_update);
                Batcher::apply_program_update(transaction, program_update, batch_buffer).await
            }
        }
    }

    async fn apply_program_registration(
        batcher: Arc<Mutex<Batcher>>,
        transaction: Transaction,
    ) -> Result<(), BatcherError> {
        if let Some(scheduler_actor) =
            get_actor_ref::<SchedulerMessage, SchedulerError>(ActorType::Scheduler)
        {
            let mut account = match get_account(transaction.from(), ActorType::Batcher).await {
                None => {
                    let e = BatcherError::FailedTransaction {
                        msg: "deployer account doesn't exit".to_string(),
                        txn: Box::new(transaction.clone()),
                    };
                    let error_string = e.to_string();

                    let message = SchedulerMessage::CallTransactionFailure {
                        transaction_hash: transaction.hash_string(),
                        outputs: "".to_string(),
                        error: error_string,
                    };
                    scheduler_actor.cast(message);
                    return Err(e);
                }
                Some(account) => account,
            };

            let json: serde_json::Map<String, Value> = serde_json::from_str(&transaction.inputs())
                .map_err(|e| BatcherError::FailedTransaction {
                    msg: e.to_string(),
                    txn: Box::new(transaction.clone()),
                })?;

            let content_id = {
                match json
                    .get("contentId")
                    .ok_or(BatcherError::FailedTransaction {
                        msg: "content id is required".to_string(),
                        txn: Box::new(transaction.clone()),
                    })? {
                    Value::String(cid) => cid.clone(),
                    _ => {
                        return Err(BatcherError::FailedTransaction {
                            msg: "contentId is incorrect type: Must be String".to_string(),
                            txn: Box::new(transaction.clone()),
                        })
                    }
                }
            };

            let program_id = create_program_id(content_id.clone(), &transaction).map_err(|e| {
                BatcherError::FailedTransaction {
                    msg: e.to_string(),
                    txn: Box::new(transaction.clone()),
                }
            })?;

            let mut metadata = Metadata::new();
            metadata
                .inner_mut()
                .insert("content_id".to_string(), content_id);
            let mut program_account = AccountBuilder::default()
                .account_type(AccountType::Program(program_id))
                .owner_address(transaction.from())
                .nonce(U256::from(0))
                .programs(BTreeMap::new())
                .program_namespace(None)
                .program_account_linked_programs(BTreeSet::new())
                .program_account_data(ArbitraryData::new())
                .program_account_metadata(metadata)
                .build()
                .map_err(|e| BatcherError::FailedTransaction {
                    msg: e.to_string(),
                    txn: Box::new(transaction.clone()),
                })?;

            Batcher::add_account_to_batch(
                &batcher,
                program_account,
                "apply_program_registration: for program account".to_string(),
            )
            .await
            .map_err(|e| BatcherError::FailedTransaction {
                msg: e.to_string(),
                txn: Box::new(transaction.clone()),
            })?;

            account.increment_nonce();

            Batcher::add_account_to_batch(
                &batcher,
                account,
                "apply_program_registration: for user account".to_string(),
            )
            .await
            .map_err(|e| BatcherError::FailedTransaction {
                msg: e.to_string(),
                txn: Box::new(transaction.clone()),
            })?;

            let message = SchedulerMessage::RegistrationSuccess {
                program_id,
                transaction: transaction.clone(),
            };
            scheduler_actor
                .cast(message)
                .map_err(|e| BatcherError::FailedTransaction {
                    msg: e.to_string(),
                    txn: Box::new(transaction),
                })
        } else {
            Err(BatcherError::FailedTransaction {
                msg: "unable to acquire Scheduler actor".to_string(),
                txn: Box::new(transaction),
            })
        }
    }

    fn add_account_to_batch_buffer(batch_buffer: &mut HashMap<Address, Account>, account: Account) {
        match &account.account_type() {
            AccountType::User => {
                batch_buffer.insert(account.owner_address(), account);
            }
            AccountType::Program(program_address) => {
                batch_buffer.insert(*program_address, account);
            }
        }
    }

    async fn try_create_program_account(
        transaction: &Transaction,
        instruction: CreateInstruction,
        batch_buffer: &HashMap<Address, Account>,
    ) -> Result<Account, BatcherError> {
        if let Some(account) = batch_buffer.get(&transaction.to()) {
            return Ok(account.clone());
        }

        if let Some(account) = get_account(transaction.to(), ActorType::Batcher).await {
            Ok(account)
        } else {
            let mut metadata = Metadata::new();
            metadata.inner_mut().insert(
                "total_supply".to_string(),
                format!("0x{:064x}", instruction.total_supply()),
            );
            metadata.inner_mut().insert(
                "initialized_supply".to_string(),
                format!("0x{:064x}", instruction.initialized_supply()),
            );
            let mut account = AccountBuilder::default()
                .account_type(AccountType::Program(transaction.to()))
                .program_namespace(None)
                .owner_address(transaction.from())
                .program_account_data(ArbitraryData::new())
                .program_account_metadata(Metadata::new())
                .program_account_linked_programs(BTreeSet::new())
                .programs(BTreeMap::new())
                .nonce(U256::from(0))
                .build()
                .map_err(|e| BatcherError::Custom(e.to_string()))?;

            Ok(account)
        }
    }

    async fn apply_instructions_to_accounts(
        batcher: Arc<Mutex<Batcher>>,
        transaction: Transaction,
        outputs: Outputs,
    ) -> Result<(), BatcherError> {
        let mut batch_buffer = HashMap::new();
        let mut caller = get_account(transaction.to(), ActorType::Batcher)
            .await
            .ok_or(BatcherError::FailedTransaction {
                msg: "caller account does not exist".to_string(),
                txn: Box::new(transaction.clone()),
            })?;

        caller.increment_nonce();

        Batcher::add_account_to_batch(
            &batcher,
            caller,
            "apply_instructions_to_accounts: for caller account".to_string(),
        )
        .await
        .map_err(|e| BatcherError::FailedTransaction {
            msg: e.to_string(),
            txn: Box::new(transaction.clone()),
        })?;

        for instruction in outputs.instructions().iter().cloned() {
            match instruction {
                Instruction::Transfer(mut transfer) => {
                    tracing::warn!("Applying transfer instruction: {:?}", transfer);
                    let (from_account, to_account) = Batcher::apply_transfer_instruction(
                        &batcher,
                        &transaction,
                        &transfer,
                        &mut batch_buffer,
                    )
                    .await
                    .map_err(|e| BatcherError::FailedTransaction {
                        msg: e.to_string(),
                        txn: Box::new(transaction.clone()),
                    })?;
                    Batcher::add_account_to_batch_buffer(&mut batch_buffer, from_account);
                    Batcher::add_account_to_batch_buffer(&mut batch_buffer, to_account);
                }
                Instruction::Burn(burn) => {
                    tracing::info!("Applying burn instruction: {:?}", burn);
                    let account =
                        Batcher::apply_burn_instruction(&transaction, &burn, &mut batch_buffer)
                            .await
                            .map_err(|e| BatcherError::FailedTransaction {
                                msg: e.to_string(),
                                txn: Box::new(transaction.clone()),
                            })?;
                    Batcher::add_account_to_batch_buffer(&mut batch_buffer, account);
                }
                Instruction::Create(create) => {
                    tracing::info!("Applying create instruction: {:?}", create);
                    tracing::info!(
                        "Create instruction has {} distributions",
                        &create.distribution().len()
                    );
                    for dist in create.distribution() {
                        tracing::warn!("Applying distribution: {:?}", create);
                        let account =
                            Batcher::apply_distribution(&transaction, dist, &mut batch_buffer)
                                .await
                                .map_err(|e| BatcherError::FailedTransaction {
                                    msg: e.to_string(),
                                    txn: Box::new(transaction.clone()),
                                })?;
                        Batcher::add_account_to_batch_buffer(&mut batch_buffer, account);
                    }

                    let program_account =
                        Batcher::try_create_program_account(&transaction, create, &batch_buffer)
                            .await
                            .map_err(|e| BatcherError::FailedTransaction {
                                msg: e.to_string(),
                                txn: Box::new(transaction.clone()),
                            })?;
                    Batcher::add_account_to_batch_buffer(&mut batch_buffer, program_account);
                }
                Instruction::Update(update) => {
                    tracing::info!("Applying update instruction: {:?}", update);
                    tracing::info!("Update instruction has {} updates", &update.updates().len());
                    for token_or_program_update in update.updates() {
                        tracing::info!("Applying update: {:?}", &token_or_program_update);
                        let account = Batcher::apply_update(
                            &transaction,
                            token_or_program_update,
                            &mut batch_buffer,
                        )
                        .await
                        .map_err(|e| BatcherError::FailedTransaction {
                            msg: e.to_string(),
                            txn: Box::new(transaction.clone()),
                        })?;
                        Batcher::add_account_to_batch_buffer(&mut batch_buffer, account);
                    }
                }
                Instruction::Log(log) => match &log.0 {
                    ContractLogType::Info(log_str) => tracing::info!("{}", log_str),
                    ContractLogType::Warn(log_str) => tracing::warn!("{}", log_str),
                    ContractLogType::Error(log_str) => tracing::error!("{}", log_str),
                    ContractLogType::Debug(log_str) => tracing::debug!("{}", log_str),
                },
            }
        }

        for (_, account) in batch_buffer {
            Batcher::add_account_to_batch(
                &batcher,
                account,
                "apply_instructions_to_accounts: for batch buffer".to_string(),
            )
            .await
            .map_err(|e| BatcherError::FailedTransaction {
                msg: e.to_string(),
                txn: Box::new(transaction.clone()),
            })?;
        }

        tracing::warn!("Adding transaction to a batch");
        Batcher::add_transaction_to_batch(batcher, transaction.clone()).await;

        if let Some(scheduler_actor) =
            get_actor_ref::<SchedulerMessage, SchedulerError>(ActorType::Scheduler)
        {
            if let Some(pending_transactions) = get_actor_ref::<
                PendingTransactionMessage,
                PendingTransactionError,
            >(ActorType::PendingTransactions)
            {
                let message = PendingTransactionMessage::ValidCall {
                    outputs: outputs.clone(),
                    transaction: transaction.clone(),
                    cert: None,
                };

                tracing::info!(
                    "Informing pending transactions that the transaction has been applied successfully"
                );
                if let Err(err) = pending_transactions.cast(message) {
                    tracing::error!(
                        "failed to cast valid call message to pending transactions actor: {err:?}"
                    );
                }

                tracing::warn!(
                    "Batcher: attempting to get account: {:?}",
                    transaction.from()
                );
                let account = get_account(transaction.from(), ActorType::Batcher)
                    .await
                    .ok_or(BatcherError::FailedTransaction {
                        msg: "unable to acquire caller account".to_string(),
                        txn: Box::new(transaction.clone()),
                    })?;
                let owner = account.owner_address();

                let message = SchedulerMessage::CallTransactionApplied {
                    transaction_hash: transaction.hash_string(),
                    account,
                };

                tracing::warn!("Informing scheduler that the call transaction was applied");
                if let Err(err) = scheduler_actor.cast(message) {
                    tracing::error!(
                        "failed to cast call transaction applied message to scheduler actor for account address {owner}: {err:?}"
                    );
                }
                Ok(())
            } else {
                Err(BatcherError::FailedTransaction {
                    msg: "unable to acquire PendingTransactionActor".to_string(),
                    txn: Box::new(transaction.clone()),
                })
            }
        } else {
            Err(BatcherError::FailedTransaction {
                msg: "unable to acquire SchedulerActor".to_string(),
                txn: Box::new(transaction.clone()),
            })
        }
    }

    pub fn handle_transaction_error(err: String, transaction: Transaction) {
        get_actor_ref::<PendingTransactionMessage, PendingTransactionError>(
            ActorType::PendingTransactions,
        )
        .and_then(|pending_transactions| {
            let message = PendingTransactionMessage::Invalid {
                transaction,
                e: Box::new(BatcherError::Custom(err)) as Box<dyn std::error::Error + Send>,
            };
            pending_transactions.cast(message).typecast().log_err(|e| {
                PendingTransactionError::Custom(format!(
                    "failed to cast invalid transaction message to PendingTransactionActor: {e:?}"
                ))
            })
        });
    }

    async fn handle_next_batch_request(
        batcher: Arc<Mutex<Batcher>>,
        tikv_client: TikvClient,
    ) -> Result<(), BatcherError> {
        if let Some(blob_response) = {
            let mut guard = batcher.lock().await;
            if !guard.parent.empty() {
                tracing::info!("found next batch: {:?}", guard.parent);

                if let Some(batch) = guard.parent.to_owned().into() {
                    let account_map = &guard.parent.accounts;
                    tracing::info!("{account_map:?}");
                    // let transaction_map = &guard.parent.transactions;

                    for (addr, account) in account_map.iter() {
                        let data = account.clone();
                        //note: this can be serialized as well need be.
                        //TiKV will accept any key if of type String, OR Vec<u8>
                        let acc_val = AccountValue { account: data };
                        // Serialize `Account` data to be stored.
                        if let Ok(val) = bincode::serialize(&acc_val) {
                            if tikv_client.put(addr.clone(), val).await.is_ok() {
                                tracing::warn!(
                                    "Inserted Account with address of {addr:?} to persistence layer",
                                )
                            } else {
                                tracing::error!("failed to push Account data to persistence store")
                            }
                        } else {
                            tracing::error!("failed to serialize account data")
                        }
                    }

                    // while let Some(transaction) = transaction_map.iter().next() {
                    //     let data = transaction.1.clone();
                    //     if let Some(txn_sig) = data.sig().ok() {
                    //         tracing::info!("Recoverable signature obtained.");

                    //         // note: this can be serialized as well need be
                    //         let txn_key = txn_sig.to_vec();

                    //         let txn_val = TransactionValue { transaction: data };

                    //         // Serialize `Transaction` data to be stored.
                    //         if let Some(val) = bincode::serialize(&txn_val).ok() {
                    //             if let Ok(txn_key) = client.put(txn_key, val).await {
                    //                 tracing::info!("Inserted Txn with signature: {:?}", txn_key)
                    //             } else {
                    //                 tracing::error!("failed to push Txn data to persistence store.")
                    //             }
                    //         } else {
                    //             tracing::error!("failed to serialize txn data")
                    //         }
                    //     } else {
                    //         tracing::error!("failed to obtain recoverable signature")
                    //     }
                    // }
                }

                if let Some(da_client) =
                    get_actor_ref::<DaClientMessage, DaClientError>(ActorType::DaClient)
                {
                    let (tx, rx) = oneshot();
                    tracing::info!("Sending message to DA Client to store batch");
                    let message = DaClientMessage::StoreBatch {
                        batch: guard
                            .parent
                            .encode_batch()
                            .ok_or(BatcherError::Custom("failed to encode batch".to_string()))?,
                        tx,
                    };
                    da_client
                        .cast(message)
                        .typecast()
                        .log_err(|e| BatcherError::Custom(e.to_string()));
                    let handler = |resp: Result<BlobResponse, std::io::Error>| match resp {
                        Ok(r) => Ok(r),
                        Err(e) => Err(Box::new(e) as Box<dyn std::error::Error>),
                    };

                    let blob_response = handle_actor_response(rx, handler)
                        .await
                        .map_err(|e| BatcherError::Custom(e.to_string()))?;

                    tracing::info!(
                        "Batcher received blob response: RequestId: {}",
                        &blob_response.request_id()
                    );
                    let parent = guard.parent.clone();
                    guard.cache.insert(blob_response.request_id(), parent);

                    if let Some(child) = guard.children.pop_front() {
                        guard.parent = child;
                        return Ok(());
                    }

                    guard.parent = Batch::new();

                    Some(blob_response)
                } else {
                    None
                }
            } else {
                None
            }
        } {
            Batcher::request_blob_validation(batcher, blob_response.request_id()).await;
            return Ok(());
        }

        tracing::warn!("batch is currently empty, skipping");

        Ok(())
    }

    async fn request_blob_validation(batcher: Arc<Mutex<Batcher>>, request_id: String) {
        let (tx, rx) = oneshot();
        {
            let guard = batcher.lock().await;
            guard.receiver_thread_tx.send(rx).await;
        }
        if let Some(da_actor) = get_actor_ref::<DaClientMessage, DaClientError>(ActorType::DaClient)
        {
            if let Err(err) = da_actor.cast(DaClientMessage::ValidateBlob { request_id, tx }) {
                tracing::error!(
                    "failed to cast blob validation message for DaClientActor: {err:?}"
                );
            }
        }
    }

    pub(super) async fn handle_blob_verification_proof(
        batcher: Arc<Mutex<Batcher>>,
        request_id: String,
        proof: BlobVerificationProof,
    ) -> Result<(), BatcherError> {
        tracing::info!("received blob verification proof");

        if let Some(eo_client) = get_actor_ref::<EoMessage, EoClientError>(ActorType::EoClient) {
            let accounts: HashSet<String> = {
                let guard = batcher.lock().await;
                guard
                    .cache
                    .get(&request_id)
                    .ok_or(BatcherError::Custom("request id not in cache".to_string()))?
                    .accounts
                    .keys()
                    .cloned()
                    .collect()
            };

            base64::decode(proof.batch_metadata().batch_header_hash().to_string())
                .typecast()
                .log_err(|e| {
                    BatcherError::Custom("unable to decode batch_header_hash()".to_string())
                })
                .and_then(|decoded| {
                    let mut bytes = [0u8; 32];
                    bytes.copy_from_slice(&decoded);

                    let batch_header_hash = H256(bytes);

                    let blob_index = proof.blob_index();

                    let message = EoMessage::Settle {
                        accounts,
                        batch_header_hash,
                        blob_index,
                    };

                    eo_client.cast(message).typecast().log_err(|e| {
                        EoClientError::Custom(format!(
                            "failed to cast settle message to EoClientActor: {e:?}"
                        ))
                    })
                });
        }

        Ok(())
    }
}

#[async_trait]
impl Actor for BatcherActor {
    type Msg = BatcherMessage;
    type State = Arc<Mutex<Batcher>>;
    type Arguments = Arc<Mutex<Batcher>>;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(args)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let batcher_ptr = Arc::clone(state);
        match message {
            BatcherMessage::GetNextBatch { tikv_client } => {
                Batcher::handle_next_batch_request(batcher_ptr, tikv_client).await?;
                // let mut guard = self.future_pool.lock().await;
                // guard.push(fut.boxed());
            }
            BatcherMessage::AppendTransaction {
                transaction,
                outputs,
            } => {
                tracing::warn!("appending transaction to batch");
                match transaction.transaction_type() {
                    TransactionType::Send(_) | TransactionType::BridgeIn(_) => {
                        tracing::warn!("send transaction");
                        let fut =
                            Batcher::add_transaction_to_account(batcher_ptr, transaction.clone());
                        let mut guard = self.future_pool.lock().await;
                        guard.push(fut.boxed());
                    }
                    TransactionType::Call(_) => {
                        if let Some(o) = outputs {
                            let fut = Batcher::apply_instructions_to_accounts(
                                batcher_ptr,
                                transaction,
                                o,
                            );
                            let mut guard = self.future_pool.lock().await;
                            guard.push(fut.boxed());
                        } else {
                            tracing::error!("Call transaction result did not contain outputs")
                        }
                    }
                    TransactionType::RegisterProgram(_) => {
                        let fut = Batcher::apply_program_registration(batcher_ptr, transaction);
                        let mut guard = self.future_pool.lock().await;
                        guard.push(fut.boxed());
                    }
                    TransactionType::BridgeOut(_) => {}
                }
            }
            BatcherMessage::BlobVerificationProof { request_id, proof } => {
                tracing::info!("received blob verification proof");
                let fut = Batcher::handle_blob_verification_proof(batcher_ptr, request_id, proof);
                let mut guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
        }
        Ok(())
    }
}

impl ActorExt for BatcherActor {
    type Output = Result<(), BatcherError>;
    type Future<O> = StaticFuture<Self::Output>;
    type FuturePool<F> = UnorderedFuturePool<Self::Future<Self::Output>>;
    type FutureHandler = tokio_rayon::rayon::ThreadPool;
    type JoinHandle = tokio::task::JoinHandle<()>;

    fn future_pool(&self) -> Self::FuturePool<Self::Future<Self::Output>> {
        self.future_pool.clone()
    }

    fn spawn_future_handler(actor: Self, future_handler: Self::FutureHandler) -> Self::JoinHandle {
        tokio::spawn(async move {
            loop {
                let futures = actor.future_pool();
                let mut guard = futures.lock().await;
                future_handler
                    .install(|| async move {
                        if let Some(Err(err)) = guard.next().await {
                            tracing::error!("{err:?}");
                            if let BatcherError::FailedTransaction { msg, txn } = err {
                                Batcher::handle_transaction_error(msg, *txn)
                            }
                        }
                    })
                    .await;
            }
        })
    }
}

pub struct BatcherSupervisor {
    panic_tx: Sender<ActorCell>,
}
impl BatcherSupervisor {
    pub fn new(panic_tx: Sender<ActorCell>) -> Self {
        Self { panic_tx }
    }
}
impl ActorName for BatcherSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::Batcher.to_string()
    }
}
#[derive(Debug, Error, Default)]
pub enum BatcherSupervisorError {
    #[default]
    #[error("failed to acquire BatcherSupervisor from registry")]
    RactorRegistryError,
}

#[async_trait]
impl Actor for BatcherSupervisor {
    type Msg = BatcherMessage;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: SupervisionEvent,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        tracing::warn!("Received a supervision event: {:?}", message);
        match message {
            SupervisionEvent::ActorStarted(actor) => {
                tracing::info!(
                    "actor started: {:?}, status: {:?}",
                    actor.get_name(),
                    actor.get_status()
                );
            }
            SupervisionEvent::ActorPanicked(who, reason) => {
                tracing::error!("actor panicked: {:?}, err: {:?}", who.get_name(), reason);
                self.panic_tx.send(who).await.typecast().log_err(|e| e);
            }
            SupervisionEvent::ActorTerminated(who, _, reason) => {
                tracing::error!("actor terminated: {:?}, err: {:?}", who.get_name(), reason);
            }
            SupervisionEvent::PidLifecycleEvent(event) => {
                tracing::info!("pid lifecycle event: {:?}", event);
            }
            SupervisionEvent::ProcessGroupChanged(m) => {
                process_group_changed(m);
            }
        }
        Ok(())
    }
}

pub async fn batch_requestor(
    mut stopper: tokio::sync::mpsc::Receiver<u8>,
    tikv_client: TikvClient,
) {
    if let Some(batcher) = ractor::registry::where_is(ActorType::Batcher.to_string()) {
        let batcher: ActorRef<BatcherMessage> = batcher.into();
        let batch_interval_secs = std::env::var("BATCH_INTERVAL")
            .unwrap_or_else(|_| "180".to_string())
            .parse::<u64>()
            .unwrap_or(180);
        loop {
            tracing::info!("SLEEPING THEN REQUESTING NEXT BATCH");
            tokio::time::sleep(tokio::time::Duration::from_secs(batch_interval_secs)).await;
            let message = BatcherMessage::GetNextBatch {
                tikv_client: tikv_client.clone(),
            };
            tracing::warn!("requesting next batch");
            if let Err(err) = batcher.cast(message) {
                tracing::error!("Batcher Error: failed to cast GetNextBatch message to the BatcherActor during batch_requestor routine: {err:?}");
            }

            if let Ok(1) = &stopper.try_recv() {
                tracing::error!("breaking the batch requestor loop");
                break;
            }
        }
    } else {
        tracing::error!("unable to acquire BatcherActor during batch_requestor routine");
    }
}

#[cfg(test)]
mod batcher_tests {
    use crate::batcher::{ActorExt, Batcher, BatcherActor, BatcherMessage};
    use anyhow::Result;
    use eigenda_client::proof::BlobVerificationProof;
    use futures::{FutureExt, StreamExt};
    use lasr_types::TransactionType;
    use std::sync::Arc;
    use tikv_client::RawClient as TikvClient;
    use tokio::sync::Mutex;

    /// Minimal reproduction of the `ractor::Actor` trait for testing the `handle`
    /// method match arm interactions.
    trait TestHandle {
        type Msg;
        type State;
        async fn handle(&self, message: Self::Msg, state: &mut Self::State) -> Result<()>;
    }
    impl TestHandle for BatcherActor {
        type Msg = BatcherMessage;
        type State = Arc<Mutex<Batcher>>;

        async fn handle(&self, message: Self::Msg, state: &mut Self::State) -> Result<()> {
            let batcher_ptr = Arc::clone(state);
            match message {
                BatcherMessage::GetNextBatch { tikv_client } => {
                    let fut = Batcher::handle_next_batch_request(batcher_ptr, tikv_client);
                    let mut guard = self.future_pool.lock().await;
                    guard.push(fut.boxed());
                }
                BatcherMessage::AppendTransaction {
                    transaction,
                    outputs,
                } => {
                    tracing::warn!("appending transaction to batch");
                    match transaction.transaction_type() {
                        TransactionType::Send(_) | TransactionType::BridgeIn(_) => {
                            tracing::warn!("send transaction");
                            let fut = Batcher::add_transaction_to_account(
                                batcher_ptr,
                                transaction.clone(),
                            );
                            let mut guard = self.future_pool.lock().await;
                            guard.push(fut.boxed());
                        }
                        TransactionType::Call(_) => {
                            if let Some(o) = outputs {
                                let fut = Batcher::apply_instructions_to_accounts(
                                    batcher_ptr,
                                    transaction,
                                    o,
                                );
                                let mut guard = self.future_pool.lock().await;
                                guard.push(fut.boxed());
                            } else {
                                tracing::error!("Call transaction result did not contain outputs")
                            }
                        }
                        TransactionType::RegisterProgram(_) => {
                            let fut = Batcher::apply_program_registration(batcher_ptr, transaction);
                            let mut guard = self.future_pool.lock().await;
                            guard.push(fut.boxed());
                        }
                        TransactionType::BridgeOut(_) => {}
                    }
                }
                BatcherMessage::BlobVerificationProof { request_id, proof } => {
                    tracing::info!("received blob verification proof");
                    let fut =
                        Batcher::handle_blob_verification_proof(batcher_ptr, request_id, proof);
                    let mut guard = self.future_pool.lock().await;
                    guard.push(fut.boxed());
                }
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_batcher_future_handler() {
        let batcher_actor = BatcherActor::new();
        let (receivers_thread_tx, receivers_thread_rx) = tokio::sync::mpsc::channel(128);
        let mut batcher = Arc::new(Mutex::new(Batcher::new(receivers_thread_tx)));
        let bv_proof = BlobVerificationProof::default();
        let request = "test".to_string();

        batcher_actor
            .handle(
                BatcherMessage::BlobVerificationProof {
                    request_id: request,
                    proof: bv_proof,
                },
                &mut batcher,
            )
            .await
            .unwrap();
        // TODO: Add other handle methods to test interactions
        // batcher_actor.handle(BatcherMessage::AppendTransaction { transaction: , outputs:  }, &mut batcher).await.unwrap();
        // batcher_actor.handle(BatcherMessage::BlobVerificationProof { request_id: , proof:  }, &mut batcher).await.unwrap();
        {
            let guard = batcher_actor.future_pool.lock().await;
            assert!(!guard.is_empty());
        }

        let future_thread_pool = tokio_rayon::rayon::ThreadPoolBuilder::new()
            .num_threads(num_cpus::get())
            .build()
            .unwrap();

        let actor_clone = batcher_actor.clone();
        BatcherActor::spawn_future_handler(actor_clone, future_thread_pool);

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            {
                let guard = batcher_actor.future_pool.lock().await;
                if guard.is_empty() {
                    break;
                }
            }
            interval.tick().await;
        }
    }
}
