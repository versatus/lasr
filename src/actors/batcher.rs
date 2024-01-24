#![allow(unused)]
use std::{collections::{HashMap, VecDeque, BTreeMap, BTreeSet}, fmt::Display};

use async_trait::async_trait;
use eigenda_client::{response::BlobResponse, proof::BlobVerificationProof};
use ethereum_types::H256;
use futures::stream::{FuturesUnordered, StreamExt};
use ractor::{Actor, ActorRef, ActorProcessingErr, factory::CustomHashFunction, concurrency::{oneshot, OneshotReceiver}, ActorCell};
use serde::{Serialize, Deserialize};
use thiserror::Error;
use tokio::{task::JoinHandle, sync::mpsc::{UnboundedSender, Sender, Receiver}};
use web3::types::BlockNumber;
use std::io::Write;
use flate2::{Compression, write::{ZlibEncoder, ZlibDecoder}};

use crate::{Transaction, Account, BatcherMessage, get_account, AccountBuilder, AccountCacheMessage, ActorType, SchedulerMessage, DaClientMessage, handle_actor_response, EoMessage, Address, Namespace, ProgramAccount, Metadata, ArbitraryData, program, Instruction, AddressOrNamespace, AccountType, TokenOrProgramUpdate, ContractLogType};

const BATCH_INTERVAL: u64 = 180;
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
    transactions: HashMap<[u8; 32], Transaction>,
    accounts: HashMap<[u8; 20], Account>,
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

    pub fn get_user_account(&self, address: impl Into<[u8; 20]>) -> Option<Account> {
        if let Some(ua) = self.accounts().get(&address.into()) {
            return Some(ua.clone())
        }

        None
    }

    pub fn get_transaction(&self, id: impl Into<[u8; 32]>) -> Option<Transaction> {
        if let Some(transaction) = self.transactions().get(&id.into()) {
            return Some(transaction.clone())
        }

        None
    }

    pub fn get_program_account(&self, account_type: AccountType) -> Option<Account> {
        if let AccountType::Program(program_address) = account_type {
            if let Some(program_account) = self.accounts().get(&program_address.inner()) {
                return Some(program_account.clone())
            }
        }

        None
    }

    pub(super) fn serialize_batch(&self) -> Result<Vec<u8>, BatcherError> {
        bincode::serialize(&self).map_err(|e| {
            BatcherError::Custom(e.to_string())
        })
    }

    pub(super) fn deserialize_batch(bytes: Vec<u8>) -> Result<Self, BatcherError> {
        Ok(bincode::deserialize(
            &Self::decompress_batch(
                bytes
            )?
        ).map_err(|e| {
            BatcherError::Custom(format!("ERROR: batcher.rs 71 {}", e.to_string()))
        })?)
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
        let mut id: [u8; 32] = [0; 32];
        id.copy_from_slice(&transaction.hash());
        test_batch.transactions.insert(id, transaction.clone());
        test_batch.at_capacity()
    }

    pub(super) fn account_would_exceed_capacity(
        &self,
        account: Account
    ) -> Result<bool, BatcherError> {
        let mut test_batch = self.clone();
        test_batch.accounts.insert(account.owner_address().into(), account);
        test_batch.at_capacity()
    }

    pub(super) fn at_capacity(&self) -> Result<bool, BatcherError> {
        Ok(self.check_size()? >= 512 * 1024)
    }

    pub fn insert_transaction(&mut self, transaction: Transaction) -> Result<(), BatcherError> {
        if !self.transaction_would_exceed_capacity(transaction.clone())? {
            let mut id: [u8; 32] = [0; 32];
            id.copy_from_slice(&transaction.hash());
            self.transactions.insert(id, transaction.clone());
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
            let mut id: [u8; 20] = account.owner_address().into();
            self.accounts.insert(id, account.clone());
            log::info!("{:?}", &self);
            return Ok(())
        }

        Err(
            BatcherError::Custom(
                "accounts at capacity".to_string()
            )
        )
    }

    pub fn transactions(&self) -> HashMap<[u8; 32], Transaction> {
        self.transactions.clone()
    }

    pub fn accounts(&self) -> HashMap<[u8; 20], Account> {
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
        account: Account
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
        let mut res = self.parent.insert_transaction(transaction.clone());
        if let Err(e) = res {
            log::info!("{e}");
        }

        Ok(())
    }

    pub(super) async fn add_account_to_batch(
        &mut self,
        account: Account
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.cache_account(account.clone()).await?;
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
        let mut from_account = get_account(transaction.from()).await;
        let (from_account, token) = if let Some(mut account) = from_account {
            let token = account.apply_transaction(transaction.clone()).map_err(|e| e as Box<dyn std::error::Error>)?;
            (account, token)
        } else {
            if !transaction.transaction_type().is_bridge_in() {
                return Err(Box::new(BatcherError::Custom("sender account does not exist".to_string())))
            }

            log::info!("transaction is first for account {:x} bridge_in, building account", transaction.from());
            let mut account = AccountBuilder::default()
                .program_namespace(None)
                .owner_address(transaction.from())
                .programs(BTreeMap::new())
                .nonce(ethereum_types::U256::from(0).into())
                .program_account_data(ArbitraryData::new())
                .program_account_metadata(Metadata::new())
                .program_account_linked_programs(BTreeSet::new())
                .build()?;
            let token = account.apply_transaction(
                transaction.clone()
            ).map_err(|e| e as Box<dyn std::error::Error>)?;

            (account, token)
        };
        
        log::info!(
            "applied transaction {} to account {:x}, informing scheduler",
            transaction.clone().hash_string(),
            from_account.owner_address()
        );

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

        log::info!("adding account to batch");
        self.add_account_to_batch(from_account).await?;

        if transaction.to() != transaction.from() {
            let mut to_account = get_account(transaction.to()).await;
            let to_account = if let Some(mut account) = to_account {
                let _ = account.apply_transaction(transaction.clone());
                account
            } else {
                log::info!("first transaction send to account {:x} building account", transaction.to());
                let mut account = AccountBuilder::default()
                    .program_namespace(None)
                    .owner_address(transaction.to())
                    .programs(BTreeMap::new())
                    .nonce(ethereum_types::U256::from(0).into())
                    .program_account_data(ArbitraryData::new())
                    .program_account_metadata(Metadata::new())
                    .program_account_linked_programs(BTreeSet::new())
                    .build()?;

                log::info!("applying transaction to `to` account");
                let _ = account.apply_transaction(transaction.clone());
                account
            };
            log::info!("adding account to batch");
            self.add_account_to_batch(to_account).await?;
        }

        log::info!("adding transaction to batch");
        self.add_transaction_to_batch(transaction).await?;

        Ok(())
    }

    async fn apply_instructions_to_accounts(
        &mut self,
        transaction: Transaction, 
        instructions: Vec<Instruction>
    ) -> Result<(), BatcherError> {
        let mut accounts_to_batch = Vec::new();
        for instruction in instructions {
            match instruction {
                Instruction::Transfer(transfer) => {
                    let from = transfer.from();
                    match from {
                        AddressOrNamespace::Address(from_address) => {
                            let mut account = get_account(from_address.clone()).await;
                            match account {
                                Some(mut acct) => {
                                    acct.apply_transfer_instruction(
                                        transfer.clone()
                                    ).map_err(|e| {
                                        BatcherError::Custom(
                                            e.to_string()
                                        )
                                    })?;
                                    accounts_to_batch.push(acct);
                                }
                                None => {
                                    return Err(
                                        BatcherError::Custom(
                                            format!("from account must exist for Transfer Instruction to be valid")
                                        )
                                    )
                                }
                            }
                        },
                        AddressOrNamespace::Namespace(from_namespace) => {
                            return Err(
                                BatcherError::Custom(
                                    format!("Namespaces not yet supported for Transfer instruction. Use account address for {:?}", from_namespace)
                                )
                            )
                        }
                    }
                    match transfer.to() {
                        AddressOrNamespace::Address(to_address) => {
                            let mut account = get_account(to_address.clone()).await;
                            match account {
                                Some(mut acct) => {
                                    acct.apply_transfer_instruction(
                                        transfer.clone()
                                    ).map_err(|e| {
                                        BatcherError::Custom(
                                            e.to_string()
                                        )
                                    })?;
                                    accounts_to_batch.push(acct);
                                }
                                None => {
                                    let mut acct = AccountBuilder::default()
                                        .account_type(crate::AccountType::User)
                                        .program_namespace(None)
                                        .owner_address(to_address.clone())
                                        .nonce(crate::U256::from(ethereum_types::U256::from(0)))
                                        .programs(BTreeMap::new())
                                        .program_account_linked_programs(BTreeSet::new())
                                        .program_account_metadata(Metadata::new())
                                        .program_account_data(ArbitraryData::new())
                                        .build().map_err(|e| {
                                            BatcherError::Custom(e.to_string())
                                        })?;
                                    acct.apply_transfer_instruction(
                                        transfer.clone()
                                    ).map_err(|e| {
                                        BatcherError::Custom(
                                            e.to_string()
                                        )
                                    })?;
                                    self.cache_account(acct.clone())
                                        .await
                                        .map_err(|e| {
                                            BatcherError::Custom(
                                                e.to_string()
                                            )
                                    })?;
                                    accounts_to_batch.push(acct);
                                }
                            }
                        }
                        AddressOrNamespace::Namespace(to_namespace) => {
                            return Err(
                                BatcherError::Custom(
                                    format!("Namespaces not yet supported for Transfer Instruction. Use account address for {:?}", to_namespace)
                                )
                            )
                        }
                    }
                }
                Instruction::Burn(burn) => {
                    let burn_address = burn.from();
                    match burn_address {
                        AddressOrNamespace::Address(address) => {
                            let mut account = get_account(address.clone()).await;
                            match account {
                                Some(mut acct) => {
                                    acct.apply_burn_instruction(
                                        burn.clone()
                                    ).map_err(|e| {
                                        BatcherError::Custom(
                                            e.to_string()
                                        )
                                    })?;
                                    accounts_to_batch.push(acct);
                                }
                                None => {
                                    return Err(
                                        BatcherError::Custom(
                                            "account must exist in order to have a burn instruction applied to it".to_string()
                                        )
                                    )
                                }
                            }
                        }
                        AddressOrNamespace::Namespace(namespace) => {
                            return Err(
                                BatcherError::Custom(
                                    format!("Namespaces not yet supported for Burn Instruction. Use account address for {:?}", namespace)
                                )
                            )
                        }
                    }
                }
                Instruction::Create(create) => {
                    let program_namespace = create.program_namespace();
                    let program_id = create.program_id();
                    let program_owner = create.program_owner();
                    //TODO: Add TokenStatics struct that can used to serialize metadata
                    let token_metadata = bincode::serialize(&[create.total_supply(), create.initialized_supply()])
                        .map_err(|e| {
                            BatcherError::Custom(e.to_string())
                        })?;
                    let distribution = create.distribution();
                    match program_id {
                        AddressOrNamespace::Address(program_address) => {
                            let account_type = AccountType::Program(program_address.clone());
                            if let Some(acct) = get_account(program_address.clone()).await {
                                return Err(
                                    BatcherError::Custom(
                                        "unable to create a new program account at a program address that already exists".to_string()
                                    )
                                )
                            }
                            let program_account = AccountBuilder::default()
                                .account_type(account_type)
                                .program_namespace(Some(program_namespace.clone()))
                                .owner_address(program_owner.clone())
                                .nonce(crate::U256::from(ethereum_types::U256::from(0)))
                                .programs(BTreeMap::new())
                                .program_account_linked_programs(BTreeSet::new())
                                .program_account_metadata(Metadata::from(token_metadata))
                                .program_account_data(ArbitraryData::new())
                                .build().map_err(|e| BatcherError::Custom(e.to_string()))?;
                                
                            self.cache_account(program_account.clone())
                                .await
                                .map_err(|e| {
                                    BatcherError::Custom(
                                        e.to_string()
                                    )
                            })?;
                            accounts_to_batch.push(program_account);
                        }
                        AddressOrNamespace::Namespace(namespace) => {
                            return Err(
                                BatcherError::Custom(
                                    format!("Namespaces not yet supported for Create Instruction, use address for {:?} instead", namespace)
                                )
                            )
                        }
                    }
                    for dist in distribution {
                        let to = dist.to();
                        match to {
                            AddressOrNamespace::Address(to_addr) => {
                                if let Some(mut acct) = get_account(to_addr.clone()).await {
                                    acct.apply_token_distribution(
                                        dist.clone()
                                    ).map_err(|e| {
                                        BatcherError::Custom(
                                            e.to_string()
                                        )
                                    })?;

                                    accounts_to_batch.push(acct);
                                } else {
                                    let mut acct = AccountBuilder::default()
                                        .account_type(AccountType::User)
                                        .program_namespace(None)
                                        .owner_address(to_addr.clone())
                                        .nonce(crate::U256::from(ethereum_types::U256::from(0)))
                                        .programs(BTreeMap::new())
                                        .program_account_linked_programs(BTreeSet::new())
                                        .program_account_metadata(Metadata::new())
                                        .program_account_data(ArbitraryData::new())
                                        .build().map_err(|e| BatcherError::Custom(e.to_string()))?;

                                    acct.apply_token_distribution(
                                        dist.clone()
                                    ).map_err(|e| {
                                        BatcherError::Custom(
                                            e.to_string()
                                        )
                                    })?;

                                    accounts_to_batch.push(acct);
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
                }
                Instruction::Update(update) => {
                    for token_or_program_update in update.updates() {
                        match token_or_program_update {
                            TokenOrProgramUpdate::TokenUpdate(token_update) => {
                                match token_update.account() {
                                    AddressOrNamespace::Address(addr) => {
                                        if let Some(mut acct) = get_account(addr.clone()).await {
                                            acct.apply_token_update(
                                                token_update.clone()
                                            ).map_err(|e| {
                                                BatcherError::Custom(
                                                    e.to_string()
                                                )
                                            })?;
                                            accounts_to_batch.push(acct);
                                        } else {
                                            let mut acct = AccountBuilder::default()
                                                .account_type(AccountType::User)
                                                .program_namespace(None)
                                                .owner_address(addr.clone())
                                                .nonce(crate::U256::from(ethereum_types::U256::from(0)))
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

                                            acct.apply_token_update(
                                                token_update.clone()
                                            ).map_err(|e| {
                                                BatcherError::Custom(
                                                    e.to_string()
                                                )
                                            })?;
                                            accounts_to_batch.push(acct);
                                        }
                                    }
                                    AddressOrNamespace::Namespace(namespace) => {
                                        return Err(
                                            BatcherError::Custom(
                                                format!("Update Instruction does not yet support namespaces for accounts, use address for {:?} instead", namespace)
                                            )
                                        )
                                    }
                                }
                            }
                            TokenOrProgramUpdate::ProgramUpdate(program_update) => {
                                match program_update.account() {
                                    AddressOrNamespace::Address(addr) => {
                                        if let Some(mut acct) = get_account(addr.clone()).await {
                                            acct.apply_program_update(
                                                program_update.clone()
                                            ).map_err(|e| {
                                                BatcherError::Custom(
                                                    e.to_string()
                                                )
                                            })?;
                                            accounts_to_batch.push(acct);
                                        } else {
                                            return Err(
                                                BatcherError::Custom(
                                                    "Program updates can only be applied to programs that already exist".to_string()
                                                )
                                            )
                                        }
                                    }
                                    AddressOrNamespace::Namespace(namespace) => {
                                        return Err(
                                            BatcherError::Custom(
                                                format!("Update Instruction does not yet support namespaces for accounts, use address for {:?}, instead", namespace)
                                            )
                                        )
                                    }
                                }
                            }
                        }
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

        for account in accounts_to_batch {
            self.cache_account(
                account.clone()
            ).await.map_err(|e| {
                BatcherError::Custom(
                    e.to_string()
                )
            })?;
        }

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

        let accounts = self.cache.get(&request_id).ok_or(
            BatcherError::Custom("request id not in cache".to_string())
        )?.accounts.iter()
            .map(|(addr, _)| addr.into())
            .collect();
        
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
            BatcherMessage::AppendTransaction(transaction) => {
                log::info!("appending transaction to batch");
                let res = state.add_transaction_to_account(transaction).await;
                if let Err(e) = res {
                    log::error!("{e}");
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

    loop {
        log::info!("SLEEPING THEN REQUESTING NEXT BATCH");
        tokio::time::sleep(tokio::time::Duration::from_secs(BATCH_INTERVAL)).await;
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
