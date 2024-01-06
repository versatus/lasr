#![allow(unused)]
use std::{collections::{HashMap, VecDeque, BTreeMap}, fmt::Display};

use async_trait::async_trait;
use eigenda_client::{response::BlobResponse, proof::BlobVerificationProof};
use ethereum_types::H256;
use futures::stream::{FuturesUnordered, StreamExt};
use ractor::{Actor, ActorRef, ActorProcessingErr, factory::CustomHashFunction, concurrency::{oneshot, OneshotReceiver}, ActorCell};
use serde::{Serialize, Deserialize};
use thiserror::Error;
use tokio::{task::JoinHandle, sync::mpsc::{UnboundedSender, Sender, Receiver}};
use std::io::Write;
use flate2::{Compression, write::{ZlibEncoder, ZlibDecoder}};

use crate::{Transaction, Account, BatcherMessage, get_account, AccountBuilder, AccountCacheMessage, ActorType, SchedulerMessage, DaClientMessage, handle_actor_response, EoMessage, Address};

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
    accounts: HashMap<[u8; 20], Account>
}

impl Batch {
    pub fn new() -> Self {
        Self {
            transactions: HashMap::new(),
            accounts: HashMap::new(),
        }
    }

    pub fn empty(&self) -> bool {
        let mut empty = false;
        empty = self.transactions().is_empty();
        empty = self.accounts().is_empty();
        return empty
    }

    pub fn get_account(&self, address: impl Into<[u8; 20]>) -> Option<Account> {
        if let Some(account) = self.accounts().get(&address.into()) {
            return Some(account.clone())
        }

        None
    }

    pub fn get_transaction(&self, id: impl Into<[u8; 32]>) -> Option<Transaction> {
        if let Some(transaction) = self.transactions().get(&id.into()) {
            return Some(transaction.clone())
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
        test_batch.accounts.insert(account.address().into(), account);
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
            let mut id: [u8; 20] = account.address().into();
            self.accounts.insert(id, account.clone());
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
                .address(transaction.from())
                .programs(BTreeMap::new())
                .nonce(0.into())
                .build()?;
            let token = account.apply_transaction(
                transaction.clone()
            ).map_err(|e| e as Box<dyn std::error::Error>)?;

            (account, token)
        };
        
        log::info!(
            "applied transaction {} to account {:x}, informing scheduler",
            transaction.clone().hash_string(),
            from_account.address()
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
                    .address(transaction.to())
                    .programs(BTreeMap::new())
                    .nonce(0.into())
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

    async fn handle_next_batch_request(&mut self) -> Result<(), BatcherError> {
        if !self.parent.empty() {
            let da_client: ActorRef<DaClientMessage> = ractor::registry::where_is(
                ActorType::DaClient.to_string()
            ).ok_or(
                BatcherError::Custom("unable to acquire DA Actor".to_string())
            )?.into();

            let (tx, rx) = oneshot();
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
                log::info!("sending batch to DA Client");
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
