#![allow(unused)]
use std::{collections::HashMap, fmt::Display};
use async_trait::async_trait;
use ethereum_types::U256;
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::oneshot, RpcReplyPort};
use thiserror::*;
use crate::{account::Address, create_handler, RecoverableSignature, AccountCacheMessage, check_account_cache, TransactionResponse, check_da_for_account, Transaction};
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

    async fn handle_get_account_request(&self, address: Address, rpc_reply: RpcReplyPort<RpcMessage>) -> Result<(), SchedulerError> {
        if let Some(account) = check_account_cache(address).await {
            rpc_reply.send(
                RpcMessage::Response { 
                    response: Ok(
                        TransactionResponse::GetAccountResponse(account)
                    ), 
                    reply: None 
                }
            ).map_err(|e| SchedulerError::Custom(e.to_string()))?;
            
            return Ok(())
        } else if let Some(account) = check_da_for_account(address).await {
            rpc_reply.send(
                RpcMessage::Response { 
                    response: Ok(
                        TransactionResponse::GetAccountResponse(account)
                    ), 
                    reply: None 
                }
            ).map_err(|e| SchedulerError::Custom(e.to_string()))?;
        } else {
            rpc_reply.send(
                RpcMessage::Response { 
                    response: Err(
                        RpcResponseError
                    ), 
                    reply: None 
                }
            ).map_err(|e| SchedulerError::Custom(e.to_string()))?;
        }

        Ok(())
    }

    fn handle_send(&self, transaction: Transaction) -> Result<(), Box<dyn std::error::Error>> {
        let engine_actor: ActorRef<EngineMessage> = ractor::registry::where_is(
            ActorType::Engine.to_string()
        ).ok_or(
            Box::new(SchedulerError::Custom("unable to acquire engine actor".to_string()))
        )?.into();

        let message = EngineMessage::Send { transaction };

        engine_actor.cast(message)?;

        Ok(())
    }
}


#[async_trait]
impl Actor for TaskScheduler {
    type Msg = SchedulerMessage;
    type State = HashMap<String, RpcReplyPort<RpcMessage>>;
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(HashMap::new())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        println!("Scheduler Received RPC Call");
        match message {
            SchedulerMessage::Call { transaction, rpc_reply } => {
                log::info!("Scheduler received RPC `call` method. Prepping to send to Validator & Engine");
            },
            SchedulerMessage::Send { transaction, rpc_reply } => {
                log::info!("Scheduler received RPC `send` method. Prepping to send to Pending Transactions");
                self.handle_send(transaction.clone());
                state.insert(transaction.hash_string(), rpc_reply);
                // Send to `Engine` where a `Transaction` will be created
                // add to RpcCollector with reply port
                // when transaction is complete and account is consolidated,
                // the collector will respond to the channel opened by the 
                // RpcServer with the necessary information
            },
            SchedulerMessage::Deploy { .. } => {
                log::info!("Scheduler received RPC `deploy` method. Prepping to send to Validator & Engine");
                // Add to pending transactions where a dependency graph will be built
                // add to RpcCollector with reply port
                // when transaction is complete and account is consolidated,
                // the collector will respond to the channel opened by the 
                // RpcServer with the necessary information
            },
            SchedulerMessage::GetAccount { address, rpc_reply } => {
                log::info!("Scheduler received RPC `getAccount` method. Prepping to check cache");
                // Check cache
                self.handle_get_account_request(address, rpc_reply).await;
                // if not in cache check DA
                // if not in DA check archives
            },
            SchedulerMessage::TransactionApplied { transaction_hash, token } => {
                log::warn!("Received TransactionApplied message, checking for RPCReplyPort");
                if let Some(reply_port) = state.remove(&transaction_hash) {
                    let response = Ok(
                        TransactionResponse::SendResponse(token)
                    );
                    let message = RpcMessage::Response { 
                        response,
                        reply: None 
                    };
                    reply_port.send(message);
                }
            }
            _ => {}
        }

        Ok(())

    }
}
