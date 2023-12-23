#![allow(unused)]
use std::{collections::HashMap, fmt::Display};
use async_trait::async_trait;
use ethereum_types::U256;
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::oneshot, RpcReplyPort};
use thiserror::*;
use crate::{account::Address, create_handler, RecoverableSignature};
use super::{messages::{RpcMessage, ValidatorMessage, EngineMessage, SchedulerMessage, RpcResponseError, EoMessage, DaClientMessage}, types::ActorType, handle_actor_response, eo_server, da_client};
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
pub struct TaskScheduler;

impl TaskScheduler {
    /// Creates a new TaskScheduler with a reference to the Registry actor
    pub fn new() -> Self {
        Self 
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
            _ => {}
        }

        Ok(())

    }
}
