use async_trait::async_trait;
use eigenda_client::proof::BlobVerificationProof;
use eigenda_client::response::BlobResponse;
use futures::stream::FuturesUnordered;
use lasr_messages::{ActorName, SupervisorType};
use lasr_messages::{ActorType, BlobCacheMessage, DaClientMessage};
use lasr_types::{Address, Transaction};
use ractor::ActorRef;
use ractor::SupervisionEvent;
use ractor::{
    concurrency::{oneshot, OneshotReceiver},
    ActorProcessingErr,
};
use ractor::{Actor, ActorCell};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use thiserror::Error;
use tokio::sync::mpsc::Sender;

use crate::Coerce;

#[derive(Debug, Default)]
pub struct PendingBlobCache {
    //TODO(asmith) create an ergonimical RequestId struct for EigenDa
    //Blob responses
    queue: HashMap<String /*request_id*/, (HashSet<Address>, HashSet<Transaction>)>,
    receivers: FuturesUnordered<OneshotReceiver<(String /*request_id*/, BlobVerificationProof)>>,
}

#[derive(Debug, Clone, Error)]
pub struct PendingBlobError;

impl Display for PendingBlobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl PendingBlobCache {
    pub fn new() -> Self {
        let queue = HashMap::new();
        let receivers = FuturesUnordered::new();
        Self { queue, receivers }
    }

    #[allow(unused)]
    fn handle_queue_removal(
        &mut self,
        response: BlobResponse,
        proof: BlobVerificationProof,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.queue.remove(&response.request_id());
        Ok(())
    }

    #[allow(unused)]
    fn handle_queue_write(
        &mut self,
        response: BlobResponse,
        accounts: HashSet<Address>,
        transactions: HashSet<Transaction>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(entry) = self.queue.get_mut(&response.request_id()) {
            *entry = (accounts, transactions);
        } else {
            self.queue
                .insert(response.request_id(), (accounts, transactions));
        }
        let (tx, rx) = oneshot();
        self.receivers.push(rx);
        let da_actor: ActorRef<DaClientMessage> =
            ractor::registry::where_is(ActorType::DaClient.to_string())
                .ok_or(Box::new(PendingBlobError) as Box<dyn std::error::Error>)?
                .into();
        da_actor.cast(DaClientMessage::ValidateBlob {
            request_id: response.request_id(),
            tx,
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct BlobCacheActor;

impl BlobCacheActor {
    pub fn new() -> Self {
        Self
    }
}
impl ActorName for BlobCacheActor {
    fn name(&self) -> ractor::ActorName {
        ActorType::BlobCache.to_string()
    }
}

#[async_trait]
impl Actor for BlobCacheActor {
    type Msg = BlobCacheMessage;
    type State = PendingBlobCache;
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(PendingBlobCache::new())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        _message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        Ok(())
    }
}

pub struct BlobCacheSupervisor {
    panic_tx: Sender<ActorCell>,
}
impl BlobCacheSupervisor {
    pub fn new(panic_tx: Sender<ActorCell>) -> Self {
        Self { panic_tx }
    }
}
impl ActorName for BlobCacheSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::BlobCache.to_string()
    }
}
#[derive(Debug, Error, Default)]
pub enum BlobCacheSupervisorError {
    #[default]
    #[error("failed to acquire BlobCacheSupervisor from registry")]
    RactorRegistryError,
}

#[async_trait]
impl Actor for BlobCacheSupervisor {
    type Msg = BlobCacheMessage;
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
