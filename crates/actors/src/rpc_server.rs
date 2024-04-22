use async_trait::async_trait;
use std::str::FromStr;

use crate::{create_handler, handle_actor_response};
use ractor::{concurrency::oneshot, Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use jsonrpsee::core::Error as RpcError;
use lasr_messages::{
    ActorType, RpcMessage, RpcRequestMethod, SchedulerMessage, TransactionResponse,
};
use lasr_rpc::LasrRpcServer;
use lasr_types::{Address, Transaction};

#[derive(Debug)]
pub struct LasrRpcServerImpl {
    proxy: ActorRef<RpcMessage>,
}

#[derive(Debug, Clone, Default)]
pub struct LasrRpcServerActor;

impl LasrRpcServerActor {
    pub fn new() -> Self {
        Self
    }

    async fn handle_response_data(
        &self,
        data: RpcMessage,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), RpcError> {
        reply
            .send(data)
            .map_err(|e| RpcError::Custom(format!("{:?}", e)))?;
        Ok(())
    }

    fn handle_call_request(
        &self,
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
        &self,
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
        &self,
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
        &self,
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

    async fn handle_request_method(
        &self,
        method: RpcRequestMethod,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), ActorProcessingErr> {
        let scheduler = self
            .get_scheduler()
            .await
            .map_err(Box::new)?
            .ok_or(RpcError::Custom(
                "Unable to acquire scheduler actor".to_string(),
            ))
            .map_err(Box::new)?;
        match method {
            RpcRequestMethod::Call { transaction } => {
                self.handle_call_request(scheduler, transaction, reply)
            }
            RpcRequestMethod::Send { transaction } => {
                self.handle_send_request(scheduler, transaction, reply)
            }
            RpcRequestMethod::RegisterProgram { transaction } => {
                self.handle_register_program_request(scheduler, transaction, reply)
            }
            RpcRequestMethod::GetAccount { address } => {
                self.handle_get_account_request(scheduler, address, reply)
            }
        }
    }

    async fn get_scheduler(&self) -> Result<Option<ActorRef<SchedulerMessage>>, RpcError> {
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
                _ => return Err(jsonrpsee::core::Error::Custom(
                    "invalid response to `call` method".to_string(),
                )),
            },
            Err(e) => return Err(jsonrpsee::core::Error::Custom(e.to_string())),
        }
    }

    async fn send(&self, transaction: Transaction) -> Result<String, jsonrpsee::core::Error> {
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
                        .map_err(|e| jsonrpsee::core::Error::Custom(e.to_string()))
                }
                TransactionResponse::TransactionError(rpc_response_error) => {
                    log::error!("Returning error to client: {}", &rpc_response_error);
                    return Err(jsonrpsee::core::Error::Custom(
                        rpc_response_error.description,
                    ));
                }
                _ => {
                    return Err(jsonrpsee::core::Error::Custom(
                        "invalid response to `send` method".to_string(),
                    ))
                }
            },
            Err(e) => return Err(jsonrpsee::core::Error::Custom(e.to_string())),
        }
    }

    async fn register_program(
        &self,
        transaction: Transaction,
    ) -> Result<String, jsonrpsee::core::Error> {
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
                    return Err(jsonrpsee::core::Error::Custom(
                        rpc_response_error.description,
                    ));
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

    async fn get_account(&self, address: String) -> Result<String, jsonrpsee::core::Error> {
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
                        .map_err(|e| jsonrpsee::core::Error::Custom(e.to_string()));
                }
                _ => {
                    return Err(jsonrpsee::core::Error::Custom(
                        "invalid response to `getAccount` methond".to_string(),
                    ))
                }
            },
            Err(e) => return Err(jsonrpsee::core::Error::Custom(e.to_string())),
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
    ) -> Result<(), RpcError> {
        log::info!("Sending RPC call method to proxy actor");
        self.proxy
            .cast(RpcMessage::Request {
                method: Box::new(RpcRequestMethod::Call { transaction }),
                reply,
            })
            .map_err(|e| RpcError::Custom(e.to_string()))
    }

    async fn send_rpc_send_method_to_self(
        &self,
        transaction: Transaction,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), RpcError> {
        self.get_myself()
            .cast(RpcMessage::Request {
                method: Box::new(RpcRequestMethod::Send { transaction }),
                reply,
            })
            .map_err(|e| RpcError::Custom(e.to_string()))
    }

    async fn send_rpc_register_program_method_to_self(
        &self,
        transaction: Transaction,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), RpcError> {
        self.get_myself()
            .cast(RpcMessage::Request {
                method: Box::new(RpcRequestMethod::RegisterProgram { transaction }),
                reply,
            })
            .map_err(|e| RpcError::Custom(e.to_string()))
    }

    async fn send_rpc_get_account_method_to_self(
        &self,
        address: String,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), RpcError> {
        let address: Address =
            Address::from_str(&address).map_err(|e| RpcError::Custom(e.to_string()))?;
        self.get_myself()
            .cast(RpcMessage::Request {
                method: Box::new(RpcRequestMethod::GetAccount { address }),
                reply,
            })
            .map_err(|e| RpcError::Custom(e.to_string()))
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
                self.handle_request_method(*method, reply).await?;
            }
            RpcMessage::Response { response, reply } => {
                let reply = reply.ok_or(Box::new(RpcError::Custom(
                    "Unable to acquire rpc reply sender in RpcMessage::Response".to_string(),
                )))?;
                let message = RpcMessage::Response {
                    response,
                    reply: None,
                };
                self.handle_response_data(message, reply).await?;
            }
        }

        Ok(())
    }
}
