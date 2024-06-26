use async_trait::async_trait;
use std::str::FromStr;
use tokio::sync::mpsc::Sender;

use crate::{create_handler, handle_actor_response, process_group_changed, Coerce};
use jsonrpsee::types::{
    error::{INTERNAL_ERROR_CODE, INVALID_PARAMS_CODE},
    ErrorObjectOwned as RpcError,
};
use lasr_messages::{
    ActorName, ActorType, RpcMessage, RpcRequestMethod, RpcResponseError, SchedulerMessage,
    SupervisorType, TransactionResponse,
};
use lasr_rpc::LasrRpcServer;
use lasr_types::{Address, Transaction};
use ractor::{
    concurrency::oneshot, Actor, ActorCell, ActorProcessingErr, ActorRef, RpcReplyPort,
    SupervisionEvent,
};
use thiserror::Error;

#[derive(Debug)]
pub struct LasrRpcServerImpl {
    proxy: ActorRef<RpcMessage>,
}

#[derive(Debug, Clone, Default)]
pub struct LasrRpcServerActor;

impl ActorName for LasrRpcServerActor {
    fn name(&self) -> ractor::ActorName {
        ActorType::RpcServer.to_string()
    }
}

impl LasrRpcServerActor {
    pub fn new() -> Self {
        Self
    }

    fn handle_response_data(
        data: RpcMessage,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), RpcError> {
        reply
            .send(data)
            .map_err(|e| RpcError::owned(INTERNAL_ERROR_CODE, format!("{:?}", e), None::<()>))
    }

    fn handle_call_request(
        scheduler: ActorRef<SchedulerMessage>,
        transaction: Transaction,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), ActorProcessingErr> {
        tracing::info!("Forwarding call transaction to scheduler");
        Ok(scheduler
            .cast(SchedulerMessage::Call {
                transaction,
                rpc_reply: reply,
            })
            .map_err(Box::new)?)
    }

    fn handle_send_request(
        scheduler: ActorRef<SchedulerMessage>,
        transaction: Transaction,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), ActorProcessingErr> {
        Ok(scheduler
            .cast(SchedulerMessage::Send {
                transaction,
                rpc_reply: reply,
            })
            .map_err(Box::new)?)
    }

    fn handle_register_program_request(
        scheduler: ActorRef<SchedulerMessage>,
        transaction: Transaction,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), ActorProcessingErr> {
        Ok(scheduler
            .cast(SchedulerMessage::RegisterProgram {
                transaction,
                rpc_reply: reply,
            })
            .map_err(Box::new)?)
    }

    fn handle_get_account_request(
        scheduler: ActorRef<SchedulerMessage>,
        address: Address,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), ActorProcessingErr> {
        Ok(scheduler
            .cast(SchedulerMessage::GetAccount {
                address,
                rpc_reply: reply,
            })
            .map_err(Box::new)?)
    }

    fn handle_request_method(
        method: RpcRequestMethod,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), ActorProcessingErr> {
        let scheduler = LasrRpcServerActor::get_scheduler()
            .map_err(Box::new)?
            .ok_or(RpcError::owned(
                INTERNAL_ERROR_CODE,
                "Unable to acquire scheduler actor".to_string(),
                None::<()>,
            ))
            .map_err(Box::new)?;
        match method {
            RpcRequestMethod::Call { transaction } => {
                LasrRpcServerActor::handle_call_request(scheduler, transaction, reply)
            }
            RpcRequestMethod::Send { transaction } => {
                LasrRpcServerActor::handle_send_request(scheduler, transaction, reply)
            }
            RpcRequestMethod::RegisterProgram { transaction } => {
                LasrRpcServerActor::handle_register_program_request(scheduler, transaction, reply)
            }
            RpcRequestMethod::GetAccount { address } => {
                LasrRpcServerActor::handle_get_account_request(scheduler, address, reply)
            }
        }
    }

    fn get_scheduler() -> Result<Option<ActorRef<SchedulerMessage>>, RpcError> {
        if let Some(actor) = ractor::registry::where_is(ActorType::Scheduler.to_string()) {
            return Ok(Some(actor.into()));
        }

        Err(RpcError::owned(
            INTERNAL_ERROR_CODE,
            "unable to acquire scheduler".to_string(),
            None::<()>,
        ))
    }
}

#[async_trait]
impl LasrRpcServer for LasrRpcServerImpl {
    async fn call(&self, transaction: Transaction) -> Result<String, RpcError> {
        // This RPC is a program call to a program deployed to the network
        // this should lead to the scheduling of a compute and validation
        // task with the scheduler
        tracing::info!("Received RPC `call` method");
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx);
        self.send_rpc_call_method_to_self(transaction, reply)
            .await
            .map_err(|e| RpcError::owned(INTERNAL_ERROR_CODE, e.to_string(), None::<()>))?;

        let handler = create_handler!(rpc_response, call);

        match handle_actor_response(rx, handler)
            .await
            .map_err(|e| RpcError::owned(INTERNAL_ERROR_CODE, format!("Error: {e}"), None::<()>))
        {
            Ok(resp) => match resp {
                TransactionResponse::AsyncCallResponse(transaction_hash) => Ok(transaction_hash),
                TransactionResponse::CallResponse(account) => {
                    let account_str = serde_json::to_string(&account).map_err(|e| {
                        RpcError::owned(INTERNAL_ERROR_CODE, e.to_string(), None::<()>)
                    })?;
                    return Ok(account_str);
                }
                TransactionResponse::TransactionError(rpc_response_error) => {
                    return Err(RpcError::owned(
                        INTERNAL_ERROR_CODE,
                        format!("Error: {0}", rpc_response_error.description),
                        None::<()>,
                    ))
                }
                _ => {
                    return Err(RpcError::owned(
                        INVALID_PARAMS_CODE,
                        "invalid response to `call` method".to_string(),
                        None::<()>,
                    ))
                }
            },
            Err(e) => {
                return Err(RpcError::owned(
                    INTERNAL_ERROR_CODE,
                    e.to_string(),
                    None::<()>,
                ))
            }
        }
    }

    async fn send(&self, transaction: Transaction) -> Result<String, RpcError> {
        tracing::info!("Received RPC send method");
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx);

        self.send_rpc_send_method_to_self(transaction, reply)
            .await
            .map_err(|e| RpcError::owned(INTERNAL_ERROR_CODE, e.to_string(), None::<()>))?;

        let handler = create_handler!(rpc_response, send);

        match handle_actor_response(rx, handler)
            .await
            .map_err(|e| RpcError::owned(INTERNAL_ERROR_CODE, format!("Error: {e}"), None::<()>))
        {
            Ok(resp) => match resp {
                TransactionResponse::SendResponse(token) => {
                    return serde_json::to_string(&token).map_err(|e| {
                        RpcError::owned(INTERNAL_ERROR_CODE, e.to_string(), None::<()>)
                    })
                }
                TransactionResponse::TransactionError(rpc_response_error) => {
                    tracing::error!("Returning error to client: {}", &rpc_response_error);
                    return Err(RpcError::owned(
                        INTERNAL_ERROR_CODE,
                        format!("Error: {0}", rpc_response_error.description),
                        None::<()>,
                    ));
                }
                _ => {
                    return Err(RpcError::owned(
                        INVALID_PARAMS_CODE,
                        "invalid response to `send` method".to_string(),
                        None::<()>,
                    ))
                }
            },
            Err(e) => {
                return Err(RpcError::owned(
                    INTERNAL_ERROR_CODE,
                    e.to_string(),
                    None::<()>,
                ))
            }
        }
    }

    async fn register_program(&self, transaction: Transaction) -> Result<String, RpcError> {
        tracing::info!("Received RPC registerProgram method");
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx);

        self.send_rpc_register_program_method_to_self(transaction, reply)
            .await
            .map_err(|e| RpcError::owned(INTERNAL_ERROR_CODE, e.to_string(), None::<()>))?;

        let handler = create_handler!(rpc_response, registerProgram);

        match handle_actor_response(rx, handler)
            .await
            .map_err(|e| RpcError::owned(INTERNAL_ERROR_CODE, format!("Error: {e}"), None::<()>))
        {
            Ok(resp) => match resp {
                TransactionResponse::RegisterProgramResponse(opt) => match opt {
                    Some(program_id) => return Ok(program_id),
                    None => {
                        return Err(RpcError::owned(
                            INTERNAL_ERROR_CODE,
                            "program registeration failed to return program_id".to_string(),
                            None::<()>,
                        ))
                    }
                },
                TransactionResponse::TransactionError(rpc_response_error) => {
                    tracing::error!("Returning error to client: {}", &rpc_response_error);
                    return Err(RpcError::owned(
                        INTERNAL_ERROR_CODE,
                        format!("Error: {0}", rpc_response_error.description),
                        None::<()>,
                    ));
                }
                _ => {
                    return Err(RpcError::owned(
                        INVALID_PARAMS_CODE,
                        "received invalid response for `registerProgram` method".to_string(),
                        None::<()>,
                    ));
                }
            },
            Err(e) => {
                return Err(RpcError::owned(
                    INTERNAL_ERROR_CODE,
                    e.to_string(),
                    None::<()>,
                ))
            }
        }
    }

    async fn get_account(&self, address: String) -> Result<String, RpcError> {
        tracing::debug!("Received RPC getAccount method");

        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx);

        self.send_rpc_get_account_method_to_self(address, reply)
            .await
            .map_err(|e| RpcError::owned(INTERNAL_ERROR_CODE, e.to_string(), None::<()>))?;

        let handler = create_handler!(rpc_response, getAccount);

        match handle_actor_response(rx, handler)
            .await
            .map_err(|e| RpcError::owned(INTERNAL_ERROR_CODE, format!("Error: {e}"), None::<()>))
        {
            Ok(resp) => match resp {
                TransactionResponse::GetAccountResponse(account) => {
                    tracing::debug!("received account response");
                    return serde_json::to_string(&account).map_err(|e| {
                        RpcError::owned(INTERNAL_ERROR_CODE, e.to_string(), None::<()>)
                    });
                }
                _ => {
                    return Err(RpcError::owned(
                        INVALID_PARAMS_CODE,
                        "invalid response to `getAccount` methond".to_string(),
                        None::<()>,
                    ))
                }
            },
            Err(e) => {
                return Err(RpcError::owned(
                    INTERNAL_ERROR_CODE,
                    e.to_string(),
                    None::<()>,
                ))
            }
        }
    }
}

impl LasrRpcServerImpl {
    pub fn new(proxy: ActorRef<RpcMessage>) -> Self {
        Self { proxy }
    }

    async fn send_rpc_call_method_to_self(
        &self,
        transaction: Transaction,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), RpcResponseError> {
        tracing::info!("Sending RPC call method to proxy actor");
        self.proxy
            .cast(RpcMessage::Request {
                method: Box::new(RpcRequestMethod::Call { transaction }),
                reply,
            })
            .map_err(|e| RpcResponseError {
                description: e.to_string(),
            })
    }

    async fn send_rpc_send_method_to_self(
        &self,
        transaction: Transaction,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), RpcResponseError> {
        self.get_myself()
            .cast(RpcMessage::Request {
                method: Box::new(RpcRequestMethod::Send { transaction }),
                reply,
            })
            .map_err(|e| RpcResponseError {
                description: e.to_string(),
            })
    }

    async fn send_rpc_register_program_method_to_self(
        &self,
        transaction: Transaction,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), RpcResponseError> {
        self.get_myself()
            .cast(RpcMessage::Request {
                method: Box::new(RpcRequestMethod::RegisterProgram { transaction }),
                reply,
            })
            .map_err(|e| RpcResponseError {
                description: e.to_string(),
            })
    }

    async fn send_rpc_get_account_method_to_self(
        &self,
        address: String,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), RpcResponseError> {
        let address: Address = Address::from_str(&address).map_err(|e| RpcResponseError {
            description: (e.to_string()),
        })?;
        self.get_myself()
            .cast(RpcMessage::Request {
                method: Box::new(RpcRequestMethod::GetAccount { address }),
                reply,
            })
            .map_err(|e| RpcResponseError {
                description: e.to_string(),
            })
    }

    fn get_myself(&self) -> ActorRef<RpcMessage> {
        self.proxy.clone()
    }
}

#[async_trait]
impl Actor for LasrRpcServerActor {
    type Msg = RpcMessage;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        message: Self::Msg,
        _: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        tracing::debug!("RPC Actor Received RPC Message");
        match message {
            RpcMessage::Request { method, reply } => {
                LasrRpcServerActor::handle_request_method(*method, reply)?
            }
            RpcMessage::Response { response, reply } => {
                let reply = reply.ok_or(Box::new(RpcError::owned(
                    INTERNAL_ERROR_CODE,
                    "Unable to acquire rpc reply sender in RpcMessage::Response".to_string(),
                    None::<()>,
                )))?;
                let message = RpcMessage::Response {
                    response,
                    reply: None,
                };
                LasrRpcServerActor::handle_response_data(message, reply)?
            }
        }

        Ok(())
    }
}

pub struct LasrRpcServerSupervisor {
    panic_tx: Sender<ActorCell>,
}
impl LasrRpcServerSupervisor {
    pub fn new(panic_tx: Sender<ActorCell>) -> Self {
        Self { panic_tx }
    }
}
impl ActorName for LasrRpcServerSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::LasrRpcServer.to_string()
    }
}
#[derive(Debug, Error, Default)]
pub enum LasrRpcServerSupervisorError {
    #[default]
    #[error("failed to acquire LasrRpcServerSupervisor from registry")]
    RactorRegistryError,
}

#[async_trait]
impl Actor for LasrRpcServerSupervisor {
    type Msg = RpcMessage;
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
        tracing::warn!("Received a supervision event: {:?}", message);
        match message {
            SupervisionEvent::ActorStarted(actor) => {
                tracing::info!(
                    "actor started: {:?}, status: {:?}",
                    actor.get_name(),
                    actor.get_status()
                );
            }
            SupervisionEvent::ActorPanicked(who, reason) => {
                tracing::error!("actor panicked: {:?}, err: {:?}", who.get_name(), reason);
                self.panic_tx.send(who).await.typecast().log_err(|e| e);
            }
            SupervisionEvent::ActorTerminated(who, _, reason) => {
                tracing::error!("actor terminated: {:?}, err: {:?}", who.get_name(), reason);
            }
            SupervisionEvent::PidLifecycleEvent(event) => {
                tracing::info!("pid lifecycle event: {:?}", event);
            }
            SupervisionEvent::ProcessGroupChanged(m) => {
                process_group_changed(m);
            }
        }
        Ok(())
    }
}
