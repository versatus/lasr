use std::{sync::Arc, time::Duration};

use crate::{ActorExt, Batch, Coerce, StaticFuture, UnorderedFuturePool};
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
use lasr_messages::{ActorName, ActorType, DaClientMessage, SupervisorType};
use lasr_types::{Account, AccountType, Address};
use ractor::{
    concurrency::OneshotSender, Actor, ActorCell, ActorProcessingErr, ActorRef, SupervisionEvent,
};
use thiserror::Error;
use tokio::{
    sync::{mpsc::Sender, Mutex},
    task::JoinHandle,
};

#[derive(Clone, Debug, Default)]
pub struct DaClientActor {
    future_pool: UnorderedFuturePool<StaticFuture<()>>,
}

impl ActorName for DaClientActor {
    fn name(&self) -> ractor::ActorName {
        ActorType::DaClient.to_string()
    }
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
    ) {
        log::info!("DA Client asked to store blob");
        let blob_response = {
            let state = da_client.lock().await;
            state.disperse_blobs(batch).await
        };
        if let Err(err) = tx.send(blob_response) {
            log::error!("DaClient Error: failed to send blob_response: {err:?}");
        }
    }
    async fn validate_blob(
        da_client: Arc<Mutex<DaClient>>,
        request_id: String,
        tx: OneshotSender<(String, BlobVerificationProof)>,
    ) {
        log::info!("DA Client asked to validate blob");
        let client = {
            let state = da_client.lock().await;
            state.client.clone()
        };
        validate_blob(client, request_id, tx).await;
    }
    async fn retrieve_account(
        da_client: Arc<Mutex<DaClient>>,
        address: Address,
        batch_header_hash: H256,
        blob_index: u128,
        tx: OneshotSender<Option<Account>>,
    ) {
        log::warn!("Received a RetrieveAccount message");
        let batch_header_hash = base64::encode(batch_header_hash.0);
        let res = {
            let state = da_client.lock().await;
            state
                .client
                .retrieve_blob(&batch_header_hash.clone().into(), blob_index)
        };
        match res {
            Ok(blob) => {
                if let Ok(blob) = EncodedBlob::from_str(&blob) {
                    Batch::decode_batch(&blob.data()).map(|batch| {
                        let account = batch.get_user_account(address);
                        if let Err(Some(account)) = tx.send(account.clone()) {
                            log::error!(
                                "DaClient Error: failed to send account data for address: {}",
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
                    });
                }
            }
            Err(err) => log::error!("Error attempting to retreive account for batcher_header_hash {batch_header_hash} and blob_index {blob_index}: {err:?}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DaClient {
    client: EigenDaGrpcClient,
}

#[derive(Clone, Debug, Error)]
pub enum DaClientError {
    #[error("failed to acquire DaClientActor from registry")]
    RactorRegistryError,

    #[error("{0}")]
    Custom(String),
}

impl Default for DaClientError {
    fn default() -> Self {
        DaClientError::RactorRegistryError
    }
}

impl DaClient {
    pub fn new(client: EigenDaGrpcClient) -> Self {
        Self { client }
    }

    async fn disperse_blobs(&self, batch: String) -> Result<BlobResponse, std::io::Error> {
        self.client.disperse_blob(batch)
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
    type Output = ();
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
                    .install(|| async move { guard.next().await })
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
    tokio::task::spawn(async move { poll_blob_status(client, request_id, tx).await })
}

pub struct DaClientSupervisor {
    panic_tx: Sender<ActorCell>,
}
impl DaClientSupervisor {
    pub fn new(panic_tx: Sender<ActorCell>) -> Self {
        Self { panic_tx }
    }
}
impl ActorName for DaClientSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::DaClient.to_string()
    }
}

#[async_trait]
impl Actor for DaClientSupervisor {
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
                self.panic_tx.send(who).await.typecast().log_err(|e| e);
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
