use std::collections::HashMap;
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use ractor::{concurrency::{OneshotReceiver, oneshot}, ActorProcessingErr};
use crate::{Address, EoMessage, DaClientMessage, BlobCacheMessage, ActorType};
use eigenda_client::response::BlobResponse;
use eigenda_client::proof::BlobVerificationProof;
use thiserror::Error;
use std::fmt::Display;
use ractor::ActorRef;
use ractor::Actor;

#[derive(Debug)]
pub struct PendingBlobCache {
    //TODO(asmith) create an ergonimical RequestId struct for EigenDa 
    //Blob responses
    queue: HashMap<Address, BlobResponse>,
    receivers: FuturesUnordered<OneshotReceiver<(Address, BlobVerificationProof)>>,
}

#[derive(Debug, Clone, Error)]
pub struct PendingBlobError;

impl Display for PendingBlobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl PendingBlobCache {
    pub fn new(
    ) -> Self {
        let queue = HashMap::new();
        let receivers = FuturesUnordered::new();
        Self { queue, receivers }
    }

    #[allow(unused)]
    fn handle_queue_removal(
        &mut self, 
        address: Address, 
        proof: BlobVerificationProof
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.queue.remove(&address);
        Ok(())
    }

    #[allow(unused)]
    fn handle_queue_write(
        &mut self,
        address: Address, 
        response: BlobResponse
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(entry) = self.queue.get_mut(&address) {
            *entry = response.clone();
        } else {
            self.queue.insert(address.clone(), response.clone());
        }
        let (tx, rx) = oneshot();
        self.receivers.push(rx);
        let da_actor: ActorRef<DaClientMessage> = ractor::registry::where_is(ActorType::DaClient.to_string()).ok_or(
            Box::new(PendingBlobError) as Box<dyn std::error::Error>
        )?.into();
        let _ = da_actor.cast(
            DaClientMessage::ValidateBlob { 
                request_id: response.request_id(),
                address,
                tx
            }
        )?;

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
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            BlobCacheMessage::Cache => {},
            BlobCacheMessage::Get => {},
            BlobCacheMessage::Remove => {},
        }
        Ok(())
    }
}
