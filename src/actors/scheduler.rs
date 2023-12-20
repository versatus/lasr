#![allow(unused)]
use std::{collections::HashMap, fmt::Display};
use async_trait::async_trait;
use ethereum_types::U256;
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::oneshot, RpcReplyPort};
use thiserror::*;
use crate::{account::Address, create_handler, RecoverableSignature, Token, RegistryMember};
use super::{messages::{RegistryResponse, RpcMessage, ValidatorMessage, EngineMessage, SchedulerMessage, RegistryMessage, RegistryActor, RpcResponseError, EoMessage, DaClientMessage}, types::ActorType, handle_actor_response, eo_server, da_client};
use jsonrpsee::core::Error as RpcError;

/// A generic error type to propagate errors from this actor 
/// and other actors that interact with it
#[derive(Debug, Clone, Error)]
pub enum SchedulerError {
    Custom(String)
}

/// Display implementation for Scheduler error
impl Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Default for SchedulerError {
    fn default() -> Self {
        SchedulerError::Custom(
            "Scheduler unable to acquire actor".to_string()
        )
    }
}

/// The actor struct for the scheduler actor
#[derive(Debug, Clone)]
pub struct TaskScheduler {
    registry: ActorRef<RegistryMessage>,
}

impl RegistryMember for TaskScheduler {
    type Err = SchedulerError;
}

impl TaskScheduler {
    /// Creates a new TaskScheduler with a reference to the Registry actor
    pub fn new(registry: ActorRef<RegistryMessage>) -> Self {
        Self { 
            registry,
        }
    }

    /// Registers the actor with the Registry
    pub fn register_self(&self, myself: ActorRef<SchedulerMessage>) -> Result<(), Box<dyn std::error::Error>> {
        self.registry.cast(
            RegistryMessage::Register(ActorType::Scheduler, RegistryActor::Scheduler(myself))
        ).map_err(|e| Box::new(e))?;

        Ok(())
    }

    /// Requests a blob index from the EO server 
    async fn get_blob_index(&self, address: Address) -> Result<(), SchedulerError> {
        let (tx, rx) = oneshot();
        let handler = create_handler!(get_eo);
        self.send_to_actor::<EoMessage, _, Option<ActorRef<EoMessage>>, ActorRef<EoMessage>>(
            handler, 
            ActorType::EoServer,
            EoMessage::GetAccountBlobIndex { address, sender: tx }, 
            self.registry.clone()
        ).await
    }

    /// Requests a blob from DA
    async fn get_blob(&self, batch_header_hash: String, blob_index: u128) -> Result<(), SchedulerError> {
        let handler = create_handler!(get_da);
        let message = DaClientMessage::RetrieveBlob { batch_header_hash, blob_index };
        self.send_to_actor::<DaClientMessage, _, Option<ActorRef<DaClientMessage>>, ActorRef<DaClientMessage>>(
            handler,
            ActorType::DaClient,
            message,
            self.registry.clone()
        ).await
    }
}


#[async_trait]
impl Actor for TaskScheduler {
    type Msg = SchedulerMessage;
    type State = ();
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        println!("Scheduler Received RPC Call");
        match message {
            SchedulerMessage::Call { 
                program_id, from, to, op, inputs, sig, tx_hash, rpc_reply 
            } => {
                log::info!("Scheduler received RPC `call` method. Prepping to send to Validator & Engine");
                // Check cache for account (and dependencies) and Contract Blob for ABI
                // If one or both not in cache, get blob index from eo
                // If one or both not in cache or EO, return error to user via RPC server
                // If both found in cache or EO schedule for execution & validation 
                // If valid & executed without error, return updated account to 
                // user via RPC, and prepare as part of batch to store in DA and 
                // settle blob index to EO
            },
            SchedulerMessage::Send { .. } => {
                log::info!("Scheduler received RPC `send` method. Prepping to send to Validator & Engine");
                // Check cache for account (and dependencies)
                // If not in cache, get blob index from eo
                // If not in cache or EO, return error to user via RPC server
                // If found in cache or EO schedule for execution & validation 
                // If valid & executed without error, return updated account to 
                // user via RPC, and prepare as part of batch to store in DA and 
                // settle blob index to EO
            },
            SchedulerMessage::Deploy { .. } => {
                log::info!("Scheduler received RPC `deploy` method. Prepping to send to Validator & Engine");
            },
            SchedulerMessage::ValidatorComplete { .. } => {
                log::info!("Scheduler received a return message from the Validator");
            },
            SchedulerMessage::EngineComplete { .. } => {
                log::info!("Scheduler received a return message from the Engine");
            }
            SchedulerMessage::BlobRetrieved { .. } => {
                log::info!("Scheduler received a return message from the DaClient");
            },
            SchedulerMessage::BlobIndexAcquired { 
                address, blob_index, batch_header_hash 
            } => {
                log::info!("Scheduler received a return message from the EoServer. Prepping to retrieve blob from DA");
                self.get_blob(batch_header_hash, blob_index).await?;
            }
            SchedulerMessage::EoEvent { event } => {
                log::info!("Scheduler received an EO Event: {:?}", &event);
            }
        }

        Ok(())

    }
}
