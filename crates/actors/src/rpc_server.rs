use async_trait::async_trait;
use std::str::FromStr;
use tokio::sync::mpsc::Sender;

use crate::{create_handler, handle_actor_response, process_group_changed, Coerce};
use ractor::{
    concurrency::oneshot, Actor, ActorCell, ActorProcessingErr, ActorRef, MessagingErr,
    RpcReplyPort, SupervisionEvent,
};

use jsonrpsee::types::ErrorObjectOwned as RpcError;
use lasr_messages::{
    ActorName, ActorType, RpcMessage, RpcRequestMethod, RpcResponseError, SchedulerMessage,
    SupervisorType, TransactionResponse,
};
use lasr_rpc::LasrRpcServer;
use lasr_types::{Address, Transaction};
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
            .map_err(|e| RpcError::Custom(format!("{:?}", e)))
    }

    fn handle_call_request(
        scheduler: ActorRef<SchedulerMessage>,
        transaction: Transaction,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), ActorProcessingErr> {
        log::info!("Forwarding call transaction to scheduler");
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
            .ok_or(RpcError::Custom(
                "Unable to acquire scheduler actor".to_string(),
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

        Err(RpcError::Custom("unable to acquire scheduler".to_string()))
    }
}

#[async_trait]
impl LasrRpcServer for LasrRpcServerImpl {
    async fn call(&self, transaction: Transaction) -> Result<String, RpcError> {
        // This RPC is a program call to a program deployed to the network
        // this should lead to the scheduling of a compute and validation
        // task with the scheduler
        log::info!("Received RPC `call` method");
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx);
        self.send_rpc_call_method_to_self(transaction, reply)
            .await?;

        let handler = create_handler!(rpc_response, call);

        match handle_actor_response(rx, handler)
            .await
            .map_err(|e| RpcError::Custom(format!("Error: {}", e)))
        {
            Ok(resp) => match resp {
                TransactionResponse::AsyncCallResponse(transaction_hash) => Ok(transaction_hash),
                TransactionResponse::CallResponse(account) => {
                    let account_str = serde_json::to_string(&account)
                        .map_err(|e| RpcError::Custom(e.to_string()))?;
                    return Ok(account_str);
                }
                TransactionResponse::TransactionError(rpc_response_error) => {
                    return Err(RpcError::Custom(rpc_response_error.description))
                }
                _ => {
                    return Err(RpcError::Custom(
                        "invalid response to `call` method".to_string(),
                    ))
                }
            },
            Err(e) => return Err(RpcError::Custom(e.to_string())),
        }
    }

    async fn send(&self, transaction: Transaction) -> Result<String, RpcError> {
        log::info!("Received RPC send method");
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx);

        self.send_rpc_send_method_to_self(transaction, reply)
            .await?;

        let handler = create_handler!(rpc_response, send);

        match handle_actor_response(rx, handler)
            .await
            .map_err(|e| RpcError::Custom(format!("Error: {}", e)))
        {
            Ok(resp) => match resp {
                TransactionResponse::SendResponse(token) => {
                    return serde_json::to_string(&token)
                        .map_err(|e| RpcError::Custom(e.to_string()))
                }
                TransactionResponse::TransactionError(rpc_response_error) => {
                    log::error!("Returning error to client: {}", &rpc_response_error);
                    return Err(RpcError::Custom(rpc_response_error.description));
                }
                _ => {
                    return Err(RpcError::Custom(
                        "invalid response to `send` method".to_string(),
                    ))
                }
            },
            Err(e) => return Err(RpcError::Custom(e.to_string())),
        }
    }

    async fn register_program(&self, transaction: Transaction) -> Result<String, RpcError> {
        log::info!("Received RPC registerProgram method");
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx);

        self.send_rpc_register_program_method_to_self(transaction, reply)
            .await?;

        let handler = create_handler!(rpc_response, registerProgram);

        match handle_actor_response(rx, handler)
            .await
            .map_err(|e| RpcError::Custom(format!("Error: {}", e)))
        {
            Ok(resp) => match resp {
                TransactionResponse::RegisterProgramResponse(opt) => match opt {
                    Some(program_id) => return Ok(program_id),
                    None => {
                        return Err(RpcError::Custom(
                            "program registeration failed to return program_id".to_string(),
                        ))
                    }
                },
                TransactionResponse::TransactionError(rpc_response_error) => {
                    log::error!("Returning error to client: {}", &rpc_response_error);
                    return Err(RpcError::Custom(rpc_response_error.description));
                }
                _ => {
                    return Err(RpcError::Custom(
                        "received invalid response for `registerProgram` method".to_string(),
                    ));
                }
            },
            Err(e) => return Err(RpcError::Custom(e.to_string())),
        }
    }

    async fn get_account(&self, address: String) -> Result<String, RpcError> {
        log::info!("Received RPC getAccount method");

        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx);

        self.send_rpc_get_account_method_to_self(address, reply)
            .await?;

        let handler = create_handler!(rpc_response, getAccount);

        match handle_actor_response(rx, handler)
            .await
            .map_err(|e| RpcError::Custom(format!("Error: {}", e)))
        {
            Ok(resp) => match resp {
                TransactionResponse::GetAccountResponse(account) => {
                    log::info!("received account response");
                    return serde_json::to_string(&account)
                        .map_err(|e| RpcError::Custom(e.to_string()));
                }
                _ => {
                    return Err(RpcError::Custom(
                        "invalid response to `getAccount` methond".to_string(),
                    ))
                }
            },
            Err(e) => return Err(RpcError::Custom(e.to_string())),
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
        log::info!("Sending RPC call method to proxy actor");
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
        log::info!("RPC Actor Received RPC Message");
        match message {
            RpcMessage::Request { method, reply } => {
                LasrRpcServerActor::handle_request_method(*method, reply)?
            }
            RpcMessage::Response { response, reply } => {
                let reply = reply.ok_or(Box::new(RpcError::Custom(
                    "Unable to acquire rpc reply sender in RpcMessage::Response".to_string(),
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
