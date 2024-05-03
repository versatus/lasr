use crate::{ActorExt, Batch, StaticFuture, UnorderedFuturePool};
use async_trait::async_trait;
use eigenda_client::{
    blob::EncodedBlob,
    client::EigenDaGrpcClient,
    proof::BlobVerificationProof,
    response::BlobResponse,
    status::{BlobResult, BlobStatus},
};
use ethereum_types::H256;
use futures::{
    stream::{FuturesUnordered, StreamExt},
    FutureExt,
};
use lasr_archive::{ArchiveBackends, ArchiveRecordType, ArchiveStoreBuilder};
use lasr_messages::DaClientMessage;
use lasr_types::{
    Account, AccountType, Address, AddressOrNamespace, ArbitraryData, Metadata,
    RecoverableSignature, Token, TransactionType, U256,
};
use ractor::{concurrency::OneshotSender, Actor, ActorProcessingErr, ActorRef, SupervisionEvent};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Display,
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::{sync::Mutex, task::JoinHandle};

const MONGODB_SECRET: &str = "mongodb+srv://ohbandrew:GoofyGoober@rusty-cluster.rstuvsa.mongodb.net/?retryWrites=true&w=majority&appName=rusty-cluster";

#[derive(Clone, Debug, Default)]
pub struct DaClientActor {
    future_pool: UnorderedFuturePool<StaticFuture<Result<(), DaClientError>>>,
}

#[derive(Debug, Hash, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransactionDoc {
    // transaction: Transaction,
    transaction_type: TransactionType,
    from: [u8; 20],
    to: [u8; 20],
    program_id: [u8; 20],
    op: String,
    inputs: String,
    value: U256,
    nonce: U256,
    sig: RecoverableSignature,
}

#[derive(Debug, Hash, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountDoc {
    // account: Account,
    account_type: AccountType,
    program_namespace: Option<AddressOrNamespace>,
    owner_address: Address,
    programs: BTreeMap<Address, Token>,
    nonce: U256,
    program_account_data: ArbitraryData,
    program_account_metadata: Metadata,
    program_account_linked_programs: BTreeSet<AddressOrNamespace>,
}

impl DaClientActor {
    pub fn new() -> Self {
        Self {
            future_pool: Arc::new(Mutex::new(FuturesUnordered::new())),
        }
    }
    async fn store_batch(
        da_client: Arc<Mutex<DaClient>>,
        batch: String,
        tx: OneshotSender<Result<BlobResponse, std::io::Error>>,
    ) -> Result<(), DaClientError> {
        log::info!("DA Client asked to store blob");
        let blob_response = {
            let state = da_client.lock().await;
            state.disperse_blobs(batch.clone()).await
        };

        // Get a handle on the persistence store
        if let Ok(mut store) = ArchiveStoreBuilder::default()
            .uri(MONGODB_SECRET.to_string())
            .backend(ArchiveBackends::MongoDB)
            .datastore("lasr_archive".to_string())
            .build()
        {
            log::info!("archive store: {}", store);
            if let Ok(decoded_batch) = Batch::decode_batch(&batch) {
                let account_map = decoded_batch.accounts;
                let transaction_map = decoded_batch.transactions;

                while let Some(account) = account_map.clone().into_iter().next() {
                    let data = account.1;
                    let doc = AccountDoc {
                        account_type: data.account_type(),
                        program_namespace: data.program_namespace(),
                        owner_address: data.owner_address(),
                        programs: data.programs().clone(),
                        nonce: data.nonce(),
                        program_account_data: data.program_account_data().clone(),
                        program_account_metadata: data.program_account_metadata().clone(),
                        program_account_linked_programs: data
                            .program_account_linked_programs()
                            .clone(),
                    };

                    // Write Account data to persistance store
                    if let Ok(id) = store.create(ArchiveRecordType::Account, &doc).await {
                        log::info!("Inserted Account ID is: {}", id)
                    } else {
                        log::error!("ArchiverError: Failed to insert Account document.")
                    };
                }
                while let Some(transaction) = transaction_map.clone().into_iter().next() {
                    let data = transaction.1;
                    let doc = TransactionDoc {
                        transaction_type: data.transaction_type(),
                        from: data.from().into(),
                        to: data.to().into(),
                        program_id: data.program_id().into(),
                        op: data.op(),
                        inputs: data.inputs(),
                        value: data.value(),
                        nonce: data.nonce(),
                        sig: data
                            .sig()
                            .expect("Failed to retrieve Recoverable Signature."),
                    };

                    // Write Transaction data to persistance store
                    if let Ok(id) = store
                        .create(ArchiveRecordType::TransactionBatch, &doc)
                        .await
                    {
                        log::info!("Inserted Transaction ID is: {}", id)
                    } else {
                        log::error!("ArchiverError: Failed to insert Transaction document.")
                    }
                }
            } else {
                log::error!("BatcherError: Failed to decode batch.")
            }
        } else {
            log::error!("ArchiveError: Failed to get handle on persistence store.")
        };
        let _ = tx.send(blob_response);
        Ok(())
    }
    async fn validate_blob(
        da_client: Arc<Mutex<DaClient>>,
        request_id: String,
        tx: OneshotSender<(String, BlobVerificationProof)>,
    ) -> Result<(), DaClientError> {
        log::info!("DA Client asked to validate blob");
        let state = da_client.lock().await;
        validate_blob(state.client.clone(), request_id, tx).await;
        // Spawn a tokio task to poll EigenDa for the validated blob
        Ok(())
    }
    async fn retrieve_account(
        da_client: Arc<Mutex<DaClient>>,
        address: Address,
        batch_header_hash: H256,
        blob_index: u128,
        tx: OneshotSender<Option<Account>>,
    ) -> Result<(), DaClientError> {
        log::warn!("Received a RetrieveAccount message");
        let batch_header_hash = base64::encode(batch_header_hash.0);
        let res = {
            let state = da_client.lock().await;
            state
                .client
                .retrieve_blob(&batch_header_hash.clone().into(), blob_index)
        };
        if let Ok(blob) = res {
            let encoded_blob = EncodedBlob::from_str(&blob);
            if let Ok(blob) = encoded_blob {
                let res = Batch::decode_batch(&blob.data());
                if let Ok(batch) = &res {
                    let account = batch.get_user_account(address);
                    if let Err(Some(account)) = tx.send(account.clone()) {
                        log::error!(
                            "DaClient Error: failed to send account data: {}",
                            account.owner_address()
                        );
                    }
                    log::warn!("successfully decoded account blob");
                    if let Some(acct) = account {
                        if let AccountType::Program(addr) = acct.account_type() {
                            log::warn!("found account: {}", addr.to_full_string());
                        } else {
                            log::warn!("found account: {}", acct.owner_address().to_full_string());
                        }
                    }
                } else {
                    log::error!("{:?}", res);
                }
            }
        } else {
            log::error!("Error attempting to retreive account for batcher_header_hash {batch_header_hash} and blob_index {blob_index}: {:?}", res);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct DaClient {
    client: EigenDaGrpcClient,
}

#[derive(Clone, Debug, Error)]
pub enum DaClientError {
    Custom(String),
}

impl Display for DaClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Default for DaClientError {
    fn default() -> Self {
        DaClientError::Custom("DA Client unable to acquire actor".to_string())
    }
}

impl DaClient {
    pub fn new(client: EigenDaGrpcClient) -> Self {
        Self { client }
    }

    async fn disperse_blobs(&self, batch: String) -> Result<BlobResponse, std::io::Error> {
        let response = self.client.disperse_blob(batch)?;
        Ok(response)
    }
}

#[async_trait]
impl Actor for DaClientActor {
    type Msg = DaClientMessage;
    type State = Arc<Mutex<DaClient>>;
    type Arguments = Self::State;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::State,
    ) -> Result<Self::State, ActorProcessingErr> {
        log::info!("Da Client running prestart routine");
        Ok(args)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let da_client_ptr = Arc::clone(state);
        match message {
            // Optimistically and naively store account blobs
            DaClientMessage::StoreBatch { batch, tx } => {
                let fut = DaClientActor::store_batch(da_client_ptr, batch, tx);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            DaClientMessage::ValidateBlob { request_id, tx } => {
                let fut = DaClientActor::validate_blob(da_client_ptr, request_id, tx);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            // Optimistically and naively retreive account blobs
            DaClientMessage::RetrieveAccount {
                address,
                batch_header_hash,
                blob_index,
                tx,
            } => {
                let fut = DaClientActor::retrieve_account(
                    da_client_ptr,
                    address,
                    batch_header_hash,
                    blob_index,
                    tx,
                );
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            DaClientMessage::RetrieveTransaction { .. } => {}
            DaClientMessage::RetrieveContract { .. } => {}
            _ => {}
        }
        Ok(())
    }
}

impl ActorExt for DaClientActor {
    type Output = Result<(), DaClientError>;
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
                            log::error!("{err:?}");
                        }
                    })
                    .await;
            }
        })
    }
}

async fn get_blob_status(
    client: &EigenDaGrpcClient,
    request_id: &str,
) -> Result<BlobStatus, std::io::Error> {
    log::info!("acquired blob status");
    client.clone().get_blob_status(&request_id.to_owned()[..])
}

#[async_recursion::async_recursion]
async fn poll_blob_status(
    client: EigenDaGrpcClient,
    request_id: String,
    tx: OneshotSender<(String, BlobVerificationProof)>,
) -> Result<(), Box<dyn std::error::Error + Send>> {
    let res = get_blob_status(&client, &request_id).await;
    if let Ok(status) = res {
        if status.status().clone() != BlobResult::Confirmed {
            tokio::time::sleep(Duration::from_secs(30)).await;
            return poll_blob_status(client.clone(), request_id.clone(), tx).await;
        } else if let Some(proof) = status.blob_verification_proof() {
            log::info!("acquired verification proof, sending back to batcher");
            let _ = tx.send((request_id, proof.clone()));
        }
    } else {
        log::error!("{:?}", res);
        return Ok(());
    }

    Ok(())
}

async fn validate_blob(
    client: EigenDaGrpcClient,
    request_id: String,
    tx: OneshotSender<(String, BlobVerificationProof)>,
) -> JoinHandle<Result<(), Box<dyn std::error::Error + Send>>> {
    log::info!("spawning blob validation task");
    tokio::task::spawn(async move { poll_blob_status(client.clone(), request_id, tx).await })
}

pub struct DaSupervisor;

#[async_trait]
impl Actor for DaSupervisor {
    type Msg = DaClientMessage;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        log::info!("Da Client running prestart routine");
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: SupervisionEvent,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        log::warn!("Received a supervision event: {:?}", message);
        match message {
            SupervisionEvent::ActorStarted(actor) => {
                log::info!(
                    "actor started: {:?}, status: {:?}",
                    actor.get_name(),
                    actor.get_status()
                );
            }
            SupervisionEvent::ActorPanicked(who, reason) => {
                log::error!("actor panicked: {:?}, err: {:?}", who.get_name(), reason);
            }
            SupervisionEvent::ActorTerminated(who, _, reason) => {
                log::error!("actor terminated: {:?}, err: {:?}", who.get_name(), reason);
            }
            SupervisionEvent::PidLifecycleEvent(event) => {
                log::info!("pid lifecycle event: {:?}", event);
            }
            SupervisionEvent::ProcessGroupChanged(m) => {
                log::warn!("process group changed: {:?}", m.get_group());
            }
        }
        Ok(())
    }
}
