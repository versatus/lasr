#![allow(unused)]
use std::{collections::{HashMap, VecDeque, BTreeMap, BTreeSet, HashSet}, fmt::Display};

use sha3::{Digest, Keccak256};
use async_trait::async_trait;
use eigenda_client::{batch, proof::BlobVerificationProof, response::BlobResponse};
use ethereum_types::H256;
use futures::stream::{FuturesUnordered, StreamExt};
use ractor::{Actor, ActorRef, ActorProcessingErr, factory::CustomHashFunction, concurrency::{oneshot, OneshotReceiver}, ActorCell};
use serde::{Serialize, Deserialize};
use serde_json::Value;
use thiserror::Error;
use tokio::{task::JoinHandle, sync::mpsc::{UnboundedSender, Sender, Receiver}};
use web3::types::BlockNumber;
use std::io::Write;
use flate2::{Compression, write::{ZlibEncoder, ZlibDecoder}};

use crate::{Transaction, Account, BatcherMessage, get_account, AccountBuilder, AccountCacheMessage, ActorType, SchedulerMessage, DaClientMessage, handle_actor_response, EoMessage, Address, Namespace, ProgramAccount, Metadata, ArbitraryData, program, Instruction, AddressOrNamespace, AccountType, TokenOrProgramUpdate, ContractLogType, TransferInstruction, BurnInstruction, U256, TokenDistribution, TokenUpdate, ProgramUpdate, UpdateInstruction, PendingTransactionMessage, TransactionType, Outputs, CreateInstruction, MetadataValue, create_program_id};

// const BATCH_INTERVAL: u64 = 180;
pub type PendingReceivers = FuturesUnordered<OneshotReceiver<(String, BlobVerificationProof)>>;

#[derive(Clone, Debug, Error)]
pub enum BatcherError {
    Custom(String)
}

impl Display for BatcherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Debug)]
pub struct BatcherActor;

#[derive(Builder, Clone, Debug, Serialize, Deserialize)]
pub struct Batch {
    transactions: HashMap<String, Transaction>,
    accounts: HashMap<String, Account>,
}

impl Batch {
    pub fn new() -> Self {
        Self {
            transactions: HashMap::new(),
            accounts: HashMap::new(),
        }
    }

    pub fn empty(&self) -> bool {
        self.transactions().is_empty() &&
        self.accounts().is_empty()
    }

    pub fn get_user_account(&self, address: Address) -> Option<Account> {
        if let Some(ua) = self.accounts().get(&address.to_full_string()) {
            return Some(ua.clone())
        }

        None
    }

    pub fn get_transaction(&self, id: String) -> Option<Transaction> {
        if let Some(transaction) = self.transactions().get(&id) {
            return Some(transaction.clone())
        }

        None
    }

    pub fn get_program_account(&self, account_type: AccountType) -> Option<Account> {
        if let AccountType::Program(program_address) = account_type {
            if let Some(program_account) = self.accounts().get(&program_address.to_full_string()) {
                return Some(program_account.clone())
            }
        }

        None
    }

    pub(super) fn serialize_batch(&self) -> Result<Vec<u8>, BatcherError> {
        Ok(serde_json::to_string(&self).map_err(|e| {
            BatcherError::Custom(format!("ERROR: batcher.rs in serialized_batch method: {}", e.to_string()))
        })?.as_bytes().to_vec())
    }

    pub(super) fn deserialize_batch(bytes: Vec<u8>) -> Result<Self, BatcherError> {
        let decompressed = Batch::decompress_batch(bytes)?;
        Ok(serde_json::from_str(&String::from_utf8_lossy(&decompressed).to_owned()).map_err(|e| {
                BatcherError::Custom(format!("ERROR: batcher.rs 89 {}", e.to_string()))
            }
        )?)
    }

    pub(super) fn compress_batch(&self) -> Result<Vec<u8>, BatcherError> {
        let mut compressor = ZlibEncoder::new(Vec::new(), Compression::best());
        compressor.write_all(&self.serialize_batch()?).map_err(|e| {
            BatcherError::Custom(e.to_string())
        })?;
        let compressed = compressor.finish().map_err(|e| {
            BatcherError::Custom(e.to_string())
        })?;

        Ok(compressed)
    }

    pub(super) fn decompress_batch(bytes: Vec<u8>) -> Result<Vec<u8>, BatcherError> {
        let mut decompressor = ZlibDecoder::new(Vec::new());
        decompressor.write_all(&bytes[..]).map_err(|e| {
            BatcherError::Custom(
                format!(
                    "ERROR: batcher.rs 92 {}", e.to_string()
                )
            )
        })?;
        let decompressed = decompressor.finish().map_err(|e| {
            BatcherError::Custom(
                format!(
                    "ERROR: batcher.rs 100 {}",e.to_string()
                )
            )
        })?;

        Ok(decompressed)
    }

    pub fn encode_batch(&self) -> Result<String, BatcherError> {
        let encoded = base64::encode(self.compress_batch()?);
        log::info!("encoded batch: {:?}", &encoded);
        Ok(encoded)
    }

    pub fn decode_batch(batch: &str) -> Result<Self, BatcherError> {
        Self::deserialize_batch(
            base64::decode(batch)
                .map_err(|e| {
                    BatcherError::Custom(
                        format!("ERROR: batcher.rs 118 {}", e.to_string())
                    )
                })?
        )
    }

    pub(super) fn check_size(&self) -> Result<usize, BatcherError> {
        let encoded = self.encode_batch()?;

        Ok(encoded.as_bytes().len())
    }

    pub(super) fn transaction_would_exceed_capacity(
        &self,
        transaction: Transaction
    ) -> Result<bool, BatcherError> {
        let mut test_batch = self.clone();
        test_batch.transactions.insert(transaction.hash_string(), transaction.clone());
        test_batch.at_capacity()
    }

    pub(super) fn account_would_exceed_capacity(
        &self,
        account: Account
    ) -> Result<bool, BatcherError> {
        let mut test_batch = self.clone();
        test_batch.accounts.insert(account.owner_address().to_full_string(), account);
        test_batch.at_capacity()
    }

    pub(super) fn at_capacity(&self) -> Result<bool, BatcherError> {
        Ok(self.check_size()? >= 512 * 1024)
    }

    pub fn insert_transaction(&mut self, transaction: Transaction) -> Result<(), BatcherError> {
        if !self.transaction_would_exceed_capacity(transaction.clone())? {
            self.transactions.insert(transaction.hash_string(), transaction.clone());
            return Ok(())
        }

        Err(
            BatcherError::Custom(
                "transactions at capacity".to_string()
            )
        )
    }

    pub fn insert_account(&mut self, account: Account) -> Result<(), BatcherError> {
        if !self.clone().account_would_exceed_capacity(account.clone())? {
            log::info!("inserting account into batch");
            match account.account_type() {
                AccountType::Program(address) => {
                    self.accounts.insert(address.clone().to_full_string(), account.clone());
                }
                AccountType::User => {
                    self.accounts.insert(account.owner_address().to_full_string(), account.clone());
                }
            }
            log::info!("{:?}", &self);
            return Ok(())
        }

        Err(
            BatcherError::Custom(
                "accounts at capacity".to_string()
            )
        )
    }

    pub fn transactions(&self) -> HashMap<String, Transaction> {
        self.transactions.clone()
    }

    pub fn accounts(&self) -> HashMap<String, Account> {
        self.accounts.clone()
    }
}

#[derive(Debug)]
pub struct Batcher {
    parent: Batch,
    children: VecDeque<Batch>,
    cache: HashMap<String /* request_id*/, Batch>,
    receiver_thread_tx: Sender<OneshotReceiver<(String, BlobVerificationProof)>>
}

impl Batcher {
    pub async fn run_receivers(
        mut receiver: Receiver<OneshotReceiver<(String, BlobVerificationProof)>>,
    ) -> Result<(), BatcherError> {
        let mut pending_receivers: PendingReceivers = FuturesUnordered::new(); 
        log::info!("starting batch receivers");
        loop {
            tokio::select! {
                new_pending = receiver.recv() => {
                    match new_pending {
                        Some(pending_rx) => {
                            log::info!("batcher received a new receiver for a pending blob");
                            pending_receivers.push(pending_rx);
                        },
                        _ => {}
                    }
                },
                next_proof = pending_receivers.next() => {
                    match next_proof {
                        Some(Ok((request_id, proof))) => {
                            log::info!("batcher received blob verification proof");
                            let batcher: ActorRef<BatcherMessage> = {
                                ractor::registry::where_is(
                                    ActorType::Batcher.to_string()
                                ).ok_or(
                                    BatcherError::Custom(
                                        "unable to acquire batcher".to_string()
                                    )
                                )?.into()
                            };

                            let message = BatcherMessage::BlobVerificationProof {
                                request_id,
                                proof
                            };

                            batcher.cast(message).map_err(|e| {
                                BatcherError::Custom(e.to_string())
                            })?;
                        }
                        _ => {}
                    }
                },
            }
        }
    }

    pub fn new(
        receiver_thread_tx: Sender<OneshotReceiver<(String, BlobVerificationProof)>>
    ) -> Self {
        Self {
            parent: Batch::new(),
            children: VecDeque::new(),
            cache: HashMap::new(),
            receiver_thread_tx
       }
    } 

    pub(super) async fn cache_account(
        &self,
        account: &Account
    ) -> Result<(), Box<dyn std::error::Error>> {
        let account_cache: ActorRef<AccountCacheMessage> = ractor::registry::where_is(
            ActorType::AccountCache.to_string()
        ).ok_or(
            Box::new(BatcherError::Custom("unable to acquire account cache actor".to_string()))
        )?.into();

        let message = AccountCacheMessage::Write { account: account.clone() };
        account_cache.cast(message)?;

        Ok(())
    }

    pub(super) async fn add_transaction_to_batch(
        &mut self,
        transaction: Transaction
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut new_batch = false; 
        let mut res = self.parent.insert_transaction(transaction.clone());
        let mut iter = self.children.iter_mut();
        while let Err(ref mut e) = res {
            log::error!("{e}");
            if let Some(mut child) = iter.next() {
                res = child.insert_transaction(transaction.clone());
            } else {
                new_batch = true;
            }
        }
        
        if new_batch {
            let mut batch = Batch::new();
            batch.insert_transaction(transaction.clone());
            self.children.push_back(batch);
        }

        Ok(())
    }

    pub(super) async fn add_account_to_batch(
        &mut self,
        account: Account
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.cache_account(&account).await?;
        let mut new_batch = false; 
        let mut res = self.parent.insert_account(account.clone());
        let mut iter = self.children.iter_mut();
        while let Err(ref mut e) = res {
            log::error!("{e}");
            if let Some(mut child) = iter.next() {
                res = child.insert_account(account.clone());
            } else {
                new_batch = true;
            }
        }
        
        if new_batch {
            let mut batch = Batch::new();
            batch.insert_account(account.clone());
            self.children.push_back(batch);
        }

        Ok(())
    }

    pub(super) async fn add_transaction_to_account(
        &mut self,
        transaction: Transaction
    ) -> Result<(), Box<dyn std::error::Error>> {
        log::warn!("checking account cache for account: {:?}", transaction.from());
        let mut from_account = get_account(transaction.from()).await;
        let (from_account, token) = if let Some(mut account) = from_account {
            let token = account.apply_send_transaction(transaction.clone()).map_err(|e| e as Box<dyn std::error::Error>)?;
            (account, token)
        } else {
            if !transaction.transaction_type().is_bridge_in() {
                return Err(Box::new(BatcherError::Custom("sender account does not exist".to_string())))
            }

            log::info!("transaction is first for account {:x} bridge_in, building account", transaction.from());
            let mut account = AccountBuilder::default()
                .account_type(crate::AccountType::User)
                .program_namespace(None)
                .owner_address(transaction.from())
                .programs(BTreeMap::new())
                .nonce(crate::U256::from(0))
                .program_account_data(ArbitraryData::new())
                .program_account_metadata(Metadata::new())
                .program_account_linked_programs(BTreeSet::new())
                .build()?;
            let token = account.apply_send_transaction(
                transaction.clone()
            ).map_err(|e| e as Box<dyn std::error::Error>)?;

            (account, token)
        };
        
        log::info!(
            "applied transaction {} to account {:x}, informing scheduler",
            transaction.clone().hash_string(),
            from_account.owner_address()
        );

        log::info!("adding account to batch");
        self.add_account_to_batch(from_account).await?;

        if transaction.to() != transaction.from() {
            log::warn!("checking account cache for account: {:?}", transaction.to());
            let mut to_account = get_account(transaction.to()).await;
            let to_account = if let Some(mut account) = to_account {
                let _ = account.apply_send_transaction(transaction.clone());
                account
            } else {
                log::info!("first transaction send to account {:x} building account", transaction.to());
                let mut account = AccountBuilder::default()
                    .account_type(crate::AccountType::User)
                    .program_namespace(None)
                    .owner_address(transaction.to())
                    .programs(BTreeMap::new())
                    .nonce(crate::U256::from(0))
                    .program_account_data(ArbitraryData::new())
                    .program_account_metadata(Metadata::new())
                    .program_account_linked_programs(BTreeSet::new())
                    .build()?;

                log::info!("applying transaction to `to` account");
                let _ = account.apply_send_transaction(transaction.clone());
                account
            };
            log::info!("adding account to batch");
            self.add_account_to_batch(to_account).await?;
        }

        log::info!("adding transaction to batch");
        self.add_transaction_to_batch(transaction.clone()).await?;

        let scheduler: ActorRef<SchedulerMessage> = ractor::registry::where_is(
            ActorType::Scheduler.to_string()
        ).ok_or(
            Box::new(BatcherError::Custom("unable to acquire scheduler".to_string()))
        )?.into();
            
        let message = SchedulerMessage::TransactionApplied { 
            transaction_hash: transaction.clone().hash_string(),
            token: token.clone()
        };

        scheduler.cast(message)?;

        let pending_tx: ActorRef<PendingTransactionMessage> = ractor::registry::where_is(
            ActorType::PendingTransactions.to_string()
        ).ok_or(
            Box::new(BatcherError::Custom("unable to acquire scheduler".to_string()))
        )?.into();
            
        let message = PendingTransactionMessage::Valid { transaction, cert: None };

        pending_tx.cast(message)?;

        Ok(())
    }

    async fn get_transfer_from_account(
        &mut self,
        transaction: &Transaction,
        from: &AddressOrNamespace,
        batch_buffer: &mut HashMap<Address, Account>
    ) -> Result<Account, BatcherError> {
        match from {
            AddressOrNamespace::This => {
                let account_address = transaction.clone().to();
                if let Some(account) = batch_buffer.get(&account_address) {
                    return Ok(account.clone())
                } else {
                    log::info!("requesting account: {:?}", &account_address.to_full_string());
                    return get_account(account_address).await.ok_or(
                        BatcherError::Custom(
                            "the `from` account in a transfer must exist".to_string()
                        )
                    )
                }
            }
            AddressOrNamespace::Address(address) => {
                if let Some(account) = batch_buffer.get(&address) {
                let account_address = transaction.clone().to();
                    return Ok(account.clone())
                } else {
                    log::info!("requesting account: {:?}", &address.to_full_string());
                    return get_account(address.clone()).await.ok_or(
                        BatcherError::Custom(
                            "the `from` account in a transfer must exist".to_string()
                        )
                    )
                }
            }
            AddressOrNamespace::Namespace(namespace) => {
                return Err(
                    BatcherError::Custom(
                        "Transfers from namespaces are not yet supported, use address for {:?} instead".to_string()
                    )
                )
            }
        }
    }

    async fn get_transfer_to_account(
        &mut self,
        transaction: &Transaction,
        to: &AddressOrNamespace,
        batch_buffer: &mut HashMap<Address, Account>
    ) -> Option<Account> {
        match to {
            AddressOrNamespace::This => {
                let account_address = transaction.clone().to();
                if let Some(account) = batch_buffer.get(&account_address) {
                    return Some(account.clone())
                } else {
                    log::info!("requesting account: {:?}", &account_address.to_full_string());
                    return get_account(account_address).await
                }
            }
            AddressOrNamespace::Address(address) => {
                if let Some(account) = batch_buffer.get(&address) {
                    return Some(account.clone())
                } else {
                    log::info!("requesting account: {:?}", &address.to_full_string());
                    return get_account(address.clone()).await
                }
            }
            AddressOrNamespace::Namespace(namespace) => {
                return None
            }
        }
    }

    async fn apply_transfer_from(
        &mut self,
        transaction: &Transaction,
        transfer: &TransferInstruction,
        batch_buffer: &mut HashMap<Address, Account>
    ) -> Result<Account, BatcherError> {
        let from = transfer.from().clone();
        let mut account = self.get_transfer_from_account(transaction, &from, batch_buffer).await?;
        account.apply_transfer_from_instruction(
            transfer.token(), transfer.amount(), transfer.ids()
        ).map_err(|e| BatcherError::Custom(e.to_string()))?;
        Ok(account)
    }

    async fn apply_transfer_to(
        &mut self,
        transaction: &Transaction,
        transfer: &TransferInstruction,
        batch_buffer: &mut HashMap<Address, Account>
    ) -> Result<Account, BatcherError> {
        let to = transfer.to().clone();
        if let Some(mut account) = self.get_transfer_to_account(transaction, &to, batch_buffer).await {
            account.apply_transfer_to_instruction(
                transfer.token(), transfer.amount(), transfer.ids()
            ).map_err(|e| BatcherError::Custom(e.to_string()))?;
            Ok(account)
        } else {
            match to {
                AddressOrNamespace::Address(address) => {
                    let mut account = AccountBuilder::default()
                        .account_type(crate::AccountType::User)
                        .program_namespace(None)
                        .owner_address(address.clone())
                        .nonce(crate::U256::from(0))
                        .programs(BTreeMap::new())
                        .program_account_linked_programs(BTreeSet::new())
                        .program_account_metadata(Metadata::new())
                        .program_account_data(ArbitraryData::new())
                        .build().map_err(|e| {
                            BatcherError::Custom(e.to_string())
                        })?;
                    account.apply_transfer_to_instruction(
                        transfer.token(), transfer.amount(), transfer.ids() 
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;

                    Ok(account)
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
        &mut self,
        transaction: &Transaction,
        transfer: &TransferInstruction,
        batch_buffer: &mut HashMap<Address, Account>
    ) -> Result<(Account, Account), BatcherError> {
        let to = transfer.to().clone();
        let from_account = self.apply_transfer_from(transaction, transfer, batch_buffer).await?;
        let to_account = self.apply_transfer_to(transaction, transfer, batch_buffer).await?;
        Ok((from_account, to_account))
    }

    async fn apply_burn_instruction(
        &mut self,
        transaction: &Transaction,
        burn: &BurnInstruction,
        batch_buffer: &mut HashMap<Address, Account>
    ) -> Result<Account, BatcherError> {
        let burn_address = burn.from();
        let mut account = self.get_transfer_from_account(transaction, burn_address, batch_buffer).await?;
        account.apply_burn_instruction(burn.token(), burn.amount(), burn.token_ids())
            .map_err(|e| BatcherError::Custom(e.to_string()))?;
        Ok(account)
    }

    async fn apply_distribution(
        &mut self,
        transaction: &Transaction,
        distribution: &TokenDistribution,
        batch_buffer: &mut HashMap<Address, Account>
    ) -> Result<Account, BatcherError> {
        let program_id = match distribution.program_id() {
            AddressOrNamespace::This => transaction.to(),
            AddressOrNamespace::Address(program_address) => program_address.clone(),
            AddressOrNamespace::Namespace(namespace) => {
                return Err(
                    BatcherError::Custom(
                        "Namespaces are not yet supported for token distributions".to_string()
                    )
                )
            }
        };
        match distribution.to() {
            AddressOrNamespace::This => {
                let addr = transaction.to();
                if let Some(mut acct) = self.get_transfer_to_account(
                    &transaction, 
                    distribution.to(), 
                    batch_buffer
                ).await {
                    acct.apply_token_distribution(
                        &program_id, 
                        distribution.amount(), 
                        distribution.token_ids(), 
                        distribution.update_fields()
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    return Ok(acct)
                } else {
                    let mut acct = AccountBuilder::default()
                        .account_type(AccountType::Program(transaction.to()))
                        .program_namespace(None)
                        .owner_address(transaction.from())
                        .nonce(crate::U256::from(0))
                        .programs(BTreeMap::new())
                        .program_account_linked_programs(BTreeSet::new())
                        .program_account_metadata(Metadata::new())
                        .program_account_data(ArbitraryData::new())
                        .build().map_err(|e| BatcherError::Custom(e.to_string()))?;

                    acct.apply_token_distribution(
                        &program_id, 
                        distribution.amount(), 
                        distribution.token_ids(), 
                        distribution.update_fields()
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;

                    return Ok(acct)
                }
            }
            AddressOrNamespace::Address(to_addr) => {
                if let Some(mut account) = self.get_transfer_to_account(
                    transaction, 
                    distribution.to(), 
                    batch_buffer
                ).await {
                    account.apply_token_distribution(
                        &program_id, 
                        distribution.amount(),
                        distribution.token_ids(), 
                        distribution.update_fields()
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    return Ok(account)
                } else {
                    let mut account = AccountBuilder::default()
                        .account_type(AccountType::User)
                        .program_namespace(None)
                        .owner_address(to_addr.clone())
                        .nonce(crate::U256::from(0))
                        .programs(BTreeMap::new())
                        .program_account_linked_programs(BTreeSet::new())
                        .program_account_metadata(Metadata::new())
                        .program_account_data(ArbitraryData::new())
                        .build().map_err(|e| BatcherError::Custom(e.to_string()))?;

                    account.apply_token_distribution(
                        &program_id,
                        distribution.amount(),
                        distribution.token_ids(),
                        distribution.update_fields()
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;

                    Ok(account)
                }
            }
            AddressOrNamespace::Namespace(namespace) => {
                return Err(
                    BatcherError::Custom(
                        format!("Namespaced are not yet supported for Token Distrubtion applications, use address for {:?} instead", namespace)
                    )
                )
            }
        }
    }
    
    async fn apply_token_update(
        &mut self,
        transaction: &Transaction,
        token_update: &TokenUpdate,
        batch_buffer: &mut HashMap<Address, Account>
    ) -> Result<Account, BatcherError> {
        let program_id = match token_update.token() {
            AddressOrNamespace::This => transaction.to(),
            AddressOrNamespace::Address(token_address) => token_address.clone(),
            AddressOrNamespace::Namespace(namespace) => {
                return Err(
                    BatcherError::Custom(
                        "Namespaces are not yet supported for token updates".to_string()
                    )
                )
            }
        };
        match token_update.account() {
            AddressOrNamespace::This => {
                if let Some(mut account) = batch_buffer.get_mut(&transaction.to()) {
                    account.apply_token_update(
                        &program_id, token_update.updates()
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    return Ok(account.clone())
                }
                
                log::warn!("attempting to get account: {} from cache in batcher", &transaction.to());
                if let Some(mut account) = get_account(transaction.to()).await {
                    account.apply_token_update(
                        &program_id, token_update.updates()
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    return Ok(account)
                } else {
                    return Err(
                        BatcherError::Custom(
                            "Use of `This` variant impermissible on accounts that do not exist yet".to_string()
                        )
                    )
                }
            }
            AddressOrNamespace::Address(address) => {
                if let Some(mut account) = batch_buffer.get_mut(&transaction.to()) {
                    account.apply_token_update(
                        &program_id, token_update.updates()
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    return Ok(account.clone())
                } 

                log::warn!("attempting to get account: {} from cache in batcher", &transaction.to());
                if let Some(mut account) = get_account(address.clone()).await {
                    account.apply_token_update(
                        &program_id, token_update.updates()
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    return Ok(account)
                } else {
                    let mut account = AccountBuilder::default()
                        .account_type(AccountType::User)
                        .program_namespace(None)
                        .owner_address(address.clone())
                        .nonce(crate::U256::from(0))
                        .programs(BTreeMap::new())
                        .program_account_linked_programs(BTreeSet::new())
                        .program_account_metadata(Metadata::new())
                        .program_account_data(ArbitraryData::new())
                        .build()
                        .map_err(|e| {
                            BatcherError::Custom(
                                e.to_string()
                            )
                        })?;

                    account.apply_token_update(
                        &program_id, token_update.updates()
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    Ok(account)
                }
            }
            AddressOrNamespace::Namespace(namespace) => {
                return Err(
                    BatcherError::Custom(
                        "Namespaces are not yet enabled for applying token updates".to_string()
                    )
                )
            }
        }
    }

    async fn apply_program_update(
        &mut self,
        transaction: &Transaction,
        program_update: &ProgramUpdate,
        batch_buffer: &mut HashMap<Address, Account>
    ) -> Result<Account, BatcherError> {
        match program_update.account() {
            AddressOrNamespace::This => {
                if let Some(mut account) = batch_buffer.get_mut(&transaction.to()) {
                    account.apply_program_update(
                        program_update
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    return Ok(account.clone())
                }
                    
                log::warn!("attempting to get account {} from cache in batcher.rs 832", transaction.to());
                if let Some(mut account) = get_account(transaction.to()).await {
                    account.apply_program_update(
                        program_update
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    return Ok(account)
                } else {
                    return Err(
                        BatcherError::Custom(
                            "Use of `This` variant impermissible on accounts that do not exist yet".to_string()
                        )
                    )
                }
            }
            AddressOrNamespace::Address(address) => {
                log::warn!("attempting to get account {} from cache in batcher.rs 852", &address);
                if let Some(mut account) = get_account(address.clone()).await {
                    account.apply_program_update(
                        program_update
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    Ok(account)
                } else {
                    let mut account = AccountBuilder::default()
                        .account_type(AccountType::User)
                        .program_namespace(None)
                        .owner_address(address.clone())
                        .nonce(crate::U256::from(0))
                        .programs(BTreeMap::new())
                        .program_account_linked_programs(BTreeSet::new())
                        .program_account_metadata(Metadata::new())
                        .program_account_data(ArbitraryData::new())
                        .build()
                        .map_err(|e| {
                            BatcherError::Custom(
                                e.to_string()
                            )
                        })?;

                    account.apply_program_update(
                        program_update
                    ).map_err(|e| {
                        BatcherError::Custom(
                            e.to_string()
                        )
                    })?;
                    Ok(account)
                }
            }
            AddressOrNamespace::Namespace(namespace) => {
                return Err(
                    BatcherError::Custom(
                        "Namespaces are not yet enabled for applying token updates".to_string()
                    )
                )
            }
        }
    }

    async fn apply_update(
        &mut self,
        transaction: &Transaction,
        update: &TokenOrProgramUpdate,
        batch_buffer: &mut HashMap<Address, Account>
    ) -> Result<Account, BatcherError> {
        match update {
            TokenOrProgramUpdate::TokenUpdate(token_update) => {
                self.apply_token_update(transaction, &token_update, batch_buffer).await
            }
            TokenOrProgramUpdate::ProgramUpdate(program_update) => {
                self.apply_program_update(transaction, &program_update, batch_buffer).await
            }
        }
    }

    async fn apply_program_registration(
        &mut self,
        transaction: &Transaction
    ) -> Result<(), BatcherError> {
        let json: serde_json::Map<String, Value> = serde_json::from_str(&transaction.inputs()).map_err(|e| {
            BatcherError::Custom(e.to_string())
        })?;

        let content_id = {
            match json.get("contentId").ok_or(BatcherError::Custom("content id is required".to_string()))? { 
                Value::String(cid) => cid.clone(),
                _ => {
                    return Err(BatcherError::Custom("contentId is incorrect type: Must be String".to_string()))
                }
            }
        };

        let program_id = create_program_id(content_id.clone(), transaction).map_err(|e| {
            BatcherError::Custom(e.to_string())
        })?;

        let mut metadata = Metadata::new();
        metadata.inner_mut().insert("content_id".to_string(), content_id);
        let mut program_account = AccountBuilder::default()
            .account_type(AccountType::Program(program_id.clone()))
            .owner_address(transaction.from())
            .nonce(crate::U256::from(0))
            .programs(BTreeMap::new())
            .program_namespace(None)
            .program_account_linked_programs(BTreeSet::new())
            .program_account_data(ArbitraryData::new())
            .program_account_metadata(metadata)
            .build().map_err(|e| BatcherError::Custom(e.to_string()))?;

        self.add_account_to_batch(program_account).await.map_err(|e| {
            BatcherError::Custom(e.to_string())
        })?;

        let actor: ActorRef<SchedulerMessage> = ractor::registry::where_is(ActorType::Scheduler.to_string()).ok_or(
            BatcherError::Custom("unable to acquire Scheduler".to_string())
        )?.into();

        let message = SchedulerMessage::RegistrationSuccess { program_id, transaction: transaction.clone() };
        actor.cast(message).map_err(|e| {
            BatcherError::Custom(e.to_string())
        })?;

        Ok(())
    }

    fn add_account_to_batch_buffer(
        &mut self, 
        batch_buffer: &mut HashMap<Address, Account>, 
        account: Account
    ) {
        match &account.account_type() {
            AccountType::User => {
                batch_buffer.insert(account.owner_address(), account);
            }
            AccountType::Program(program_address) => {
                batch_buffer.insert(program_address.clone(), account);
            }
        }
    }

    async fn try_create_program_account(
        &mut self,
        transaction: &Transaction,
        instruction: CreateInstruction
    ) -> Result<Account, BatcherError> {
        if let Some(account) = get_account(transaction.to()).await {
            return Ok(account)
        } else {
            let mut metadata = Metadata::new();
            metadata.inner_mut().insert(
                "total_supply".to_string(),
                format!("0x{:064x}", instruction.total_supply())
            );
            metadata.inner_mut().insert(
                "initialized_supply".to_string(),
                format!("0x{:064x}", instruction.initialized_supply())
            );
            let mut account = AccountBuilder::default()
                .account_type(AccountType::Program(transaction.to()))
                .program_namespace(None)
                .owner_address(transaction.from())
                .program_account_data(ArbitraryData::new())
                .program_account_metadata(Metadata::new())
                .program_account_linked_programs(BTreeSet::new())
                .programs(BTreeMap::new())
                .nonce(crate::U256::from(0))
                .build()
                .map_err(|e| BatcherError::Custom(e.to_string()))?;

            return Ok(account)
        }
    }

    async fn apply_instructions_to_accounts(
        &mut self,
        transaction: &Transaction, 
        outputs: &Outputs,
    ) -> Result<(), BatcherError> {
        let mut batch_buffer = HashMap::new();
        for instruction in outputs.instructions().into_iter().cloned() {
            match instruction {
                Instruction::Transfer(mut transfer) => {
                    let (from_account, to_account) = self.apply_transfer_instruction(&transaction, &transfer, &mut batch_buffer).await?;
                    self.add_account_to_batch_buffer(&mut batch_buffer, from_account);
                    self.add_account_to_batch_buffer(&mut batch_buffer, to_account);
                }
                Instruction::Burn(burn) => {
                    let account = self.apply_burn_instruction(&transaction, &burn, &mut batch_buffer).await?;
                    self.add_account_to_batch_buffer(&mut batch_buffer, account);
                }
                Instruction::Create(create) => {
                    for dist in create.distribution() {
                        let account = self.apply_distribution(&transaction, dist, &mut batch_buffer).await?;
                        self.add_account_to_batch_buffer(&mut batch_buffer, account);
                    }

                    let program_account = self.try_create_program_account(&transaction, create).await?;
                    self.add_account_to_batch_buffer(&mut batch_buffer, program_account);
                }
                Instruction::Update(update) => {
                    for token_or_program_update in update.updates() {
                        let account = self.apply_update(&transaction, token_or_program_update, &mut batch_buffer).await?;
                        self.add_account_to_batch_buffer(&mut batch_buffer, account);
                    }
                }
                Instruction::Log(log) => {
                    match &log.0 {
                        ContractLogType::Info(log_str) => log::info!("{}", log_str),
                        ContractLogType::Warn(log_str) => log::warn!("{}", log_str),
                        ContractLogType::Error(log_str) => log::error!("{}", log_str),
                        ContractLogType::Debug(log_str) => log::debug!("{}", log_str),
                    }
                }
            }
        }

        for (_, account) in batch_buffer {
            self.cache_account(
                &account
            ).await.map_err(|e| {
                BatcherError::Custom(
                    e.to_string()
                )
            })?;

            self.add_account_to_batch(account).await.map_err(|e| {
                BatcherError::Custom(e.to_string())
            })?;
        }

        self.add_transaction_to_batch(transaction.clone()).await.map_err(|e| {
            BatcherError::Custom(e.to_string())
        });

        let scheduler: ActorRef<SchedulerMessage> = ractor::registry::where_is(
            ActorType::Scheduler.to_string()
        ).ok_or(
            BatcherError::Custom(
                "Error: batcher.rs: 816: unable to acquire scheduler actor".to_string()
            )
        )?.into();

        let pending_transactions: ActorRef<PendingTransactionMessage> = ractor::registry::where_is(
            ActorType::PendingTransactions.to_string()
        ).ok_or(
            BatcherError::Custom(
                "Error: batcher.rs: 966: unable to acquire pending transactions actor".to_string()
            )
        )?.into();

        let message = PendingTransactionMessage::ValidCall { 
            outputs: outputs.clone(), 
            transaction: transaction.clone(), 
            cert: None 
        };

        pending_transactions.cast(message);

        log::warn!("attempting to get account: {:?} in batcher.rs 1003", transaction.from());
        let account = get_account(transaction.from()).await.ok_or(
            BatcherError::Custom("Error: batcher.rs: 821: unable to acquire caller account".to_string())
        )?;
        
        let message = SchedulerMessage::CallTransactionApplied { 
            transaction_hash: transaction.hash_string(), 
            account 
        };

        scheduler.cast(message);

        Ok(())

    }

    async fn handle_next_batch_request(&mut self) -> Result<(), BatcherError> {
        if !self.parent.empty() {
            let da_client: ActorRef<DaClientMessage> = ractor::registry::where_is(
                ActorType::DaClient.to_string()
            ).ok_or(
                BatcherError::Custom("unable to acquire DA Actor".to_string())
            )?.into();

            let (tx, rx) = oneshot();
            log::info!("Sending message to DA Client to store batch");
            let message = DaClientMessage::StoreBatch { batch: self.parent.encode_batch()?, tx };
            da_client.cast(message).map_err(|e| BatcherError::Custom(e.to_string()))?;
            let handler = |resp: Result<BlobResponse, std::io::Error> | {
                match resp {
                    Ok(r) => Ok(r),
                    Err(e) => Err(Box::new(e) as Box<dyn std::error::Error>)
                }
            };

            let blob_response = handle_actor_response(rx, handler).await
                .map_err(|e| BatcherError::Custom(e.to_string()))?;

            log::info!("Batcher received blob response: RequestId: {}", &blob_response.request_id());
            let parent = self.parent.clone();
            self.cache.insert(blob_response.request_id(), parent);

            if let Some(child) = self.children.pop_front() {
                self.parent = child;
                return Ok(())
            } 

            self.parent = Batch::new();

            self.request_blob_validation(blob_response.request_id()).await?;

            return Ok(())
        }

        log::warn!("batch is currently empty, skipping");
        dbg!(&self.parent);

        return Ok(())
    }

    async fn request_blob_validation(&mut self, request_id: String) -> Result<(), BatcherError> {
        let (tx, rx) = oneshot();
        self.receiver_thread_tx.send(rx).await;
        let da_actor: ActorRef<DaClientMessage> = ractor::registry::where_is(ActorType::DaClient.to_string()).ok_or(
            BatcherError::Custom("unable to acquire da client actor ref".to_string())
        )?.into();
        let _ = da_actor.cast(
            DaClientMessage::ValidateBlob { 
                request_id,
                tx
            }
        ).map_err(|e| BatcherError::Custom(e.to_string()))?;

        Ok(())
    }

    pub(super) async fn handle_blob_verification_proof(
        &mut self,
        request_id: String,
        proof: BlobVerificationProof
    ) -> Result<(), BatcherError> {
        log::info!("received blob verification proof");
        
        let eo_client: ActorRef<EoMessage> = ractor::registry::where_is(
            ActorType::EoClient.to_string()
        ).ok_or(
            BatcherError::Custom("unable to acquire eo client actor ref".to_string())
        )?.into();

        let accounts: HashSet<String> = self.cache.get(&request_id).ok_or(
            BatcherError::Custom("request id not in cache".to_string())
        )?.accounts.iter().map(|(k, _)| k.clone()).collect();

        let decoded = base64::decode(&proof.batch_metadata().batch_header_hash().to_string()).map_err(|e| {
            BatcherError::Custom("unable to decode batch_header_hash()".to_string())
        })?;

        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&decoded);

        let batch_header_hash = H256(bytes);

        let blob_index = proof.blob_index();

        let message = EoMessage::Settle { accounts, batch_header_hash, blob_index };

        let res = eo_client.cast(message);
        if let Err(e) = res {
            log::error!("{}", e);
        }

        Ok(())
    }
}

impl BatcherActor {
    pub fn new() -> Self {
        BatcherActor
    }
}


#[async_trait]
impl Actor for BatcherActor {
    type Msg = BatcherMessage;
    type State = Batcher; 
    type Arguments = Batcher;
    
    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Batcher,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(args) 
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            BatcherMessage::GetNextBatch => {
                let res = state.handle_next_batch_request().await;
                if let Err(e) = res {
                    log::error!("{e}");
                }
            }
            BatcherMessage::AppendTransaction { transaction, outputs } => {
                log::info!("appending transaction to batch");
                match transaction.transaction_type() {
                    TransactionType::Send(_) | TransactionType::BridgeIn(_) => {
                        let res = state.add_transaction_to_account(transaction).await;
                        if let Err(e) = res {
                            log::error!("{e}");
                        }
                    }
                    TransactionType::Call(_) => {
                        if let Some(o) = outputs {
                            let res = state.apply_instructions_to_accounts(&transaction, &o).await;
                            if let Err(e) = res {
                                log::error!("{e}");
                            }
                        } else {
                            log::error!("Call transaction result did not contain outputs")
                        }
                    }
                    TransactionType::RegisterProgram(_) => {
                        let res = state.apply_program_registration(&transaction).await;
                        if let Err(e) = res {
                            log::error!("{e}");
                        }
                    },
                    TransactionType::BridgeOut(_) => {}
                }
            }
            BatcherMessage::BlobVerificationProof { request_id, proof } => {
                log::info!("received blob verification proof");
                let res = state.handle_blob_verification_proof(request_id, proof).await;
            }
        }
        Ok(())
    }
}

pub async fn batch_requestor(mut stopper: tokio::sync::mpsc::Receiver<u8>) -> Result<(), Box<dyn std::error::Error + Send>> {
    let batcher: ActorRef<BatcherMessage> = ractor::registry::where_is(
        ActorType::Batcher.to_string()
    ).unwrap().into(); 

    let batch_interval_secs = std::env::var("BATCH_INTERVAL")
            .unwrap_or_else(|_| "180".to_string())
            .parse::<u64>()
            .unwrap_or(180);
    loop {
        log::info!("SLEEPING THEN REQUESTING NEXT BATCH");
        tokio::time::sleep(tokio::time::Duration::from_secs(batch_interval_secs)).await;
        let message = BatcherMessage::GetNextBatch;
        log::warn!("requesting next batch");
        batcher.cast(message).map_err(|e| {
            Box::new(
                BatcherError::Custom(
                    e.to_string()
                )
            ) as Box<dyn std::error::Error + Send>
        });

        if let Ok(1) = &stopper.try_recv() {
            log::error!("breaking the batch requestor loop");
            break
        }
    }

    Ok(())
}
