#![allow(unused)]
use crate::{
    check_account_cache, create_handler, da_client, eo_server, handle_actor_response,
    process_group_changed, Coerce,
};
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use jsonrpsee::core::Error as RpcError;
use lasr_messages::{
    AccountCacheMessage, ActorName, ActorType, DaClientMessage, EngineMessage, EoMessage,
    RpcMessage, RpcResponseError, SchedulerMessage, SupervisorType, TransactionResponse,
    ValidatorMessage,
};
use lasr_types::{Address, RecoverableSignature, Transaction};
use ractor::{concurrency::oneshot, Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use ractor::{ActorCell, SupervisionEvent};
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, fmt::Display};
use thiserror::*;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

/// A generic error type to propagate errors from this actor
/// and other actors that interact with it
#[derive(Debug, Clone, Error)]
pub enum SchedulerError {
    #[error("failed to acquire SchedulerActor from registry")]
    RactorRegistryError,

    #[error("{0}")]
    Custom(String),
}

impl Default for SchedulerError {
    fn default() -> Self {
        SchedulerError::RactorRegistryError
    }
}

pub type MethodResults = Arc<Mutex<FuturesUnordered<Result<(), Box<dyn std::error::Error>>>>>;

pub struct SchedulerState {
    pub reply_map: HashMap<String, RpcReplyPort<RpcMessage>>,
    pub handle_method_results: MethodResults,
    pub scheduler_results_handler: JoinHandle<()>,
}

/// The actor struct for the scheduler actor
#[derive(Debug, Clone, Default)]
pub struct TaskScheduler;
impl ActorName for TaskScheduler {
    fn name(&self) -> ractor::ActorName {
        ActorType::Scheduler.to_string()
    }
}

impl TaskScheduler {
    /// Creates a new TaskScheduler with a reference to the Registry actor
    pub fn new() -> Self {
        Self
    }

    async fn handle_get_account_request(
        &self,
        address: Address,
        rpc_reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), SchedulerError> {
        log::info!(
            "Checking for account cache for account: {} from scheduler",
            address.to_full_string()
        );
        if let Some(account) = check_account_cache(address).await {
            rpc_reply
                .send(RpcMessage::Response {
                    response: Ok(TransactionResponse::GetAccountResponse(account)),
                    reply: None,
                })
                .map_err(|e| SchedulerError::Custom(e.to_string()))?;

            return Ok(());
        }
        if let None = check_account_cache(address).await {
            log::info!("unable to find account in cache or persistence store");
            rpc_reply
                .send(RpcMessage::Response {
                    response: Err(RpcResponseError {
                        description:
                            "unable to find account in Persistence Store or Protocol Cache"
                                .to_string(),
                    }),
                    reply: None,
                })
                .map_err(|e| SchedulerError::Custom(e.to_string()))?;
        }

        Ok(())
    }

    fn handle_send(&self, transaction: Transaction) -> Result<(), Box<dyn std::error::Error>> {
        log::info!("scheduler handling send: {}", transaction.hash_string());
        let engine_actor: ActorRef<EngineMessage> =
            ractor::registry::where_is(ActorType::Engine.to_string())
                .ok_or(Box::new(SchedulerError::Custom(
                    "unable to acquire engine actor".to_string(),
                )))?
                .into();

        let message = EngineMessage::Send { transaction };

        engine_actor.cast(message)?;

        Ok(())
    }

    fn handle_call(&self, transaction: Transaction) -> Result<(), Box<dyn std::error::Error>> {
        log::info!("scheduler handling call: {}", transaction.hash_string());
        let engine_actor: ActorRef<EngineMessage> =
            ractor::registry::where_is(ActorType::Engine.to_string())
                .ok_or(Box::new(SchedulerError::Custom(
                    "unable to acquire engine actor".to_string(),
                )))?
                .into();

        let message = EngineMessage::Call { transaction };
        engine_actor.cast(message)?;

        Ok(())
    }

    fn handle_register_program(
        &self,
        transaction: Transaction,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let engine_actor: ActorRef<EngineMessage> =
            ractor::registry::where_is(ActorType::Engine.to_string())
                .ok_or(Box::new(SchedulerError::Custom(
                    "unable to acquire engine actor".to_string(),
                )))?
                .into();

        let message = EngineMessage::RegisterProgram { transaction };

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
        match message {
            SchedulerMessage::Call {
                transaction,
                rpc_reply,
            } => {
                log::info!("Scheduler received RPC `call` method. Prepping to send to Engine");
                // Convert handle_call to async, store future in Arc<Mutex<FuturesUnordered>> in `Self::State`
                // handle futures in separate thread.
                self.handle_call(transaction.clone());
                state.insert(transaction.hash_string(), rpc_reply);
            }
            SchedulerMessage::Send {
                transaction,
                rpc_reply,
            } => {
                log::info!("Scheduler received RPC `send` method. Prepping to send to Pending Transactions");
                self.handle_send(transaction.clone());
                state.insert(transaction.hash_string(), rpc_reply);
            }
            SchedulerMessage::RegisterProgram {
                transaction,
                rpc_reply,
            } => {
                log::info!("Scheduler received RPC `registerProgram` method. Prepping to send to Validator & Engine");
                self.handle_register_program(transaction.clone());
                state.insert(transaction.hash_string(), rpc_reply);
            }
            SchedulerMessage::GetAccount { address, rpc_reply } => {
                log::info!("Scheduler received RPC `getAccount` method for account: {:?}. Prepping to check cache", address);
                // Check cache
                self.handle_get_account_request(address, rpc_reply).await;
                // if not in cache check DA
                // if not in DA check archives
            }
            SchedulerMessage::TransactionApplied {
                transaction_hash,
                token,
            } => {
                log::warn!("Received TransactionApplied message, checking for RPCReplyPort");
                if let Some(reply_port) = state.remove(&transaction_hash) {
                    let response = Ok(TransactionResponse::SendResponse(token));
                    let message = RpcMessage::Response {
                        response,
                        reply: None,
                    };
                    reply_port.send(message);
                }
            }
            SchedulerMessage::SendTransactionFailure {
                transaction_hash,
                error,
            } => {
                if let Some(reply_port) = state.remove(&transaction_hash) {
                    let response = Ok(TransactionResponse::TransactionError(RpcResponseError {
                        description: error.to_string(),
                    }));

                    let message = RpcMessage::Response {
                        response,
                        reply: None,
                    };
                    reply_port.send(message);
                }
            }
            SchedulerMessage::RegistrationSuccess {
                transaction,
                program_id,
            } => {
                if let Some(reply_port) = state.remove(&transaction.hash_string()) {
                    let response = Ok(TransactionResponse::RegisterProgramResponse(Some(
                        program_id.to_full_string(),
                    )));

                    let message = RpcMessage::Response {
                        response,
                        reply: None,
                    };
                    reply_port.send(message);
                }
            }
            SchedulerMessage::CallTransactionAsyncPending { transaction_hash } => {
                if let Some(reply_port) = state.remove(&transaction_hash) {
                    let response = Ok(TransactionResponse::AsyncCallResponse(transaction_hash));

                    let message = RpcMessage::Response {
                        response,
                        reply: None,
                    };
                    reply_port.send(message);
                };
            }
            SchedulerMessage::CallTransactionApplied {
                transaction_hash,
                account,
            } => {
                if let Some(reply_port) = state.remove(&transaction_hash) {
                    let response = Ok(TransactionResponse::CallResponse(account));
                    let message = RpcMessage::Response {
                        response,
                        reply: None,
                    };
                    reply_port.send(message);
                }
            }
            SchedulerMessage::CallTransactionFailure {
                transaction_hash,
                outputs,
                error,
            } => {
                if let Some(reply_port) = state.remove(&transaction_hash) {
                    let response = Ok(TransactionResponse::TransactionError(RpcResponseError {
                        description: format!(
                            "Transaction {} failed due to {}: {}",
                            transaction_hash, error, outputs
                        ),
                    }));

                    let message = RpcMessage::Response {
                        response,
                        reply: None,
                    };
                    reply_port.send(message);
                }
            }
            _ => {}
        }

        Ok(())
    }
}

pub struct TaskSchedulerSupervisor {
    panic_tx: Sender<ActorCell>,
}
impl TaskSchedulerSupervisor {
    pub fn new(panic_tx: Sender<ActorCell>) -> Self {
        Self { panic_tx }
    }
}
impl ActorName for TaskSchedulerSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::Scheduler.to_string()
    }
}
#[derive(Debug, Error, Default)]
pub enum TaskSchedulerSupervisorError {
    #[default]
    #[error("failed to acquire TaskSchedulerSupervisor from registry")]
    RactorRegistryError,
}

#[async_trait]
impl Actor for TaskSchedulerSupervisor {
    type Msg = SchedulerMessage;
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
                process_group_changed(m);
            }
        }
        Ok(())
    }
}
