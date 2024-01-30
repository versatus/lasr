use crate::{ActorType, Address, BlobCacheMessage, DaClientMessage, Transaction};
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use lasr_da::proof::BlobVerificationProof;
use lasr_da::response::BlobResponse;
use ractor::Actor;
use ractor::ActorRef;
use ractor::{
    concurrency::{oneshot, OneshotReceiver},
    ActorProcessingErr,
};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use thiserror::Error;

#[derive(Debug)]
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
        let _ = da_actor.cast(DaClientMessage::ValidateBlob {
            request_id: response.request_id(),
            tx,
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct BlobCacheActor;

impl BlobCacheActor {
    pub fn new() -> Self {
        Self
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
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            _ => {}
        }
        Ok(())
    }
}
