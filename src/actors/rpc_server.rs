use async_trait::async_trait;
use ethereum_types::U256;
use ractor::{Actor, ActorRef, ActorProcessingErr, RpcReplyPort, concurrency::oneshot, Message};
use crate::{
    rpc::LasrRpcServer, account::{ Address, Token }, 
    certificate::RecoverableSignature, actors::{messages::RegistryResponse, handle_actor_response}, create_handler
};
use jsonrpsee::core::Error as RpcError;
use crate::SchedulerError;

use super::{messages::{RpcMessage, SchedulerMessage, RegistryMessage, RegistryActor}, types::{RpcRequestMethod, ActorType}};
#[allow(unused)]
//TODO(asmith): Integrate timeouts
use super::types::TIMEOUT_DURATION;

#[derive(Debug)]
pub struct LasrRpcServerImpl {
    proxy: ActorRef<RpcMessage>,
}

#[derive(Debug, Clone)]
pub struct LasrRpcServerActor { 
    registry: ActorRef<RegistryMessage>,
}

impl LasrRpcServerActor {
    pub fn new(registry: ActorRef<RegistryMessage>) -> Self {
        Self { registry }
    }

    pub fn register_self(&self, actor_ref: ActorRef<RpcMessage>) -> Result<(), RpcError> {
        self.registry.cast(
            RegistryMessage::Register(ActorType::RpcServer, RegistryActor::RpcServer(actor_ref))
        ).map_err(|e| RpcError::Custom(e.to_string()))?;

        Ok(())
    }

    async fn handle_response_data(
        &self,
        data: RpcMessage,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), RpcError> {
        reply.send(data).map_err(|e| RpcError::Custom(format!("{:?}", e)))?;
        Ok(())
    }

    fn handle_call_request(
        &self,
        scheduler: ActorRef<SchedulerMessage>,
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        op: String,
        inputs: String,
        sig: RecoverableSignature,
        tx_hash: String,
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), ActorProcessingErr> {
        Ok(scheduler.cast(
            SchedulerMessage::Call { program_id, from, to, op, inputs, sig, tx_hash, rpc_reply: reply }
        ).map_err(|e| Box::new(e))?)
    }

    fn handle_send_request(
        &self,
        scheduler: ActorRef<SchedulerMessage>,
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        amount: U256,
        sig: RecoverableSignature,
        tx_hash: String,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), ActorProcessingErr> {
        Ok(scheduler.cast(
            SchedulerMessage::Send { program_id, from, to, amount, content: None, sig, tx_hash, rpc_reply: reply }
        ).map_err(|e| Box::new(e))?)
    }

    fn handle_deploy_request(
        &self,
        scheduler: ActorRef<SchedulerMessage>,
        program_id: Address,
        sig: RecoverableSignature,
        tx_hash: String,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), ActorProcessingErr> {
        Ok(scheduler.cast(
            SchedulerMessage::Deploy { program_id, sig, tx_hash, rpc_reply: reply }
        ).map_err(|e| Box::new(e))?)
    }

    async fn handle_request_method(
        &self,
        method: RpcRequestMethod,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), ActorProcessingErr> {
        let scheduler = self.get_scheduler().await
            .map_err(|e| Box::new(e))?.ok_or(
                RpcError::Custom(
                    "Unable to acquire scheduler actor".to_string()
                )
            ).map_err(|e| Box::new(e))?;
        match method {
            RpcRequestMethod::Call { 
                program_id, from, to, op, inputs, sig, tx_hash 
            } => {
                self.handle_call_request(scheduler, program_id, from, to, op, inputs, sig, tx_hash, reply)
            },
            RpcRequestMethod::Send { 
                program_id, from, to, amount, sig, tx_hash 
            } => {
                self.handle_send_request(scheduler, program_id, from, to, amount, sig, tx_hash, reply)
            },
            RpcRequestMethod::Deploy { 
                program_id, sig, tx_hash 
            } => {
                self.handle_deploy_request(scheduler, program_id, sig, tx_hash, reply)
            }
        }
    }

    async fn get_scheduler(&self) -> Result<Option<ActorRef<SchedulerMessage>>, RpcError> {
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx);
        let msg = RegistryMessage::GetActor(ActorType::Scheduler, reply);
        self.registry.cast(msg).map_err(|e| {
            RpcError::Custom(
                format!("{:?}", e)
            )
        })?;

        let handler = create_handler!(get_scheduler); 

        handle_actor_response(rx, handler).await.map_err(|_| {
            RpcError::Custom("invalid_registry_response_received".to_string())
        })
    }

    async fn handle_deploy_response_data(
        &self,
        msg: RpcMessage, 
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), RpcError> {
        match msg {
            RpcMessage::DeploySuccess { .. }=> { 
                reply.send(msg).map_err(|e| RpcError::Custom(format!("{:?}", e)))?;
                Ok(())
            }
            _ => {
                Err(RpcError::Custom("Received an Invalid Response Message to the RPC Actor".to_string()))
            }
        }
    }
}

#[async_trait]
impl LasrRpcServer for LasrRpcServerImpl {
    async fn call(
        &self,
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        op: String,
        inputs: String,
        tx_hash: String,
        sig: RecoverableSignature 
    ) -> Result<Token, RpcError> {
        // This RPC is a program call to a program deployed to the network
        // this should lead to the scheduling of a compute and validation
        // task with the scheduler
        println!("Received RPC `call` method");
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx); 
        self.send_rpc_call_method_to_self(
            program_id,
            from,
            to,
            op,
            inputs,
            sig,
            tx_hash,
            reply
        ).await?;
        
        let handler = create_handler!(rpc_response, call); 

        handle_actor_response(rx, handler).await.map_err(|e| {
            RpcError::Custom(
                format!("Error: {}", e)
            )
        })
    }
    
    async fn send(
        &self,
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        amount: U256,
        tx_hash: String,
        sig: RecoverableSignature
    ) -> Result<Token, jsonrpsee::core::Error> {
        println!("Received RPC send method");
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx); 

        self.send_rpc_send_method_to_self(
            program_id, from, to, amount, sig, tx_hash,reply
        ).await?;

        let handler = create_handler!(rpc_response, send); 

        handle_actor_response(rx, handler).await.map_err(|e| {
            RpcError::Custom(
                format!("Error: {}", e)
            )
        })
    }

    async fn deploy(
        &self,
        program_id: Address,
        sig: RecoverableSignature,
        tx_hash: String,
    ) -> Result<(), jsonrpsee::core::Error> {
        println!("Received RPC deploy method"); 
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx); 

        self.send_rpc_deploy_method_to_self(
            program_id,
            sig,
            tx_hash,
            reply 
        ).await?;
        
        let handler = create_handler!(rpc_deploy_success);

        handle_actor_response(rx, handler).await.map_err(|e| {
            RpcError::Custom(
                format!("Error: {}", e)
            )
        })
    }
}

impl LasrRpcServerImpl {

    pub fn new(proxy: ActorRef<RpcMessage>) -> Self {
        Self { proxy }
    }

    async fn send_rpc_call_method_to_self(
        &self, 
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        op: String,
        inputs: String,
        sig: RecoverableSignature,
        tx_hash: String,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), RpcError> {
        println!("Sending RPC call method to proxy actor");
        self.proxy
            .cast(
                RpcMessage::Request {
                    method: RpcRequestMethod::Call {
                        program_id,
                        from,
                        to,
                        op,
                        inputs,
                        sig,
                        tx_hash,
                    },
                    reply
                }
            ).map_err(|e| {
                RpcError::Custom(e.to_string())
            }
        )
    }

    async fn send_rpc_send_method_to_self(
        &self,
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        amount: U256,
        sig: RecoverableSignature,
        tx_hash: String,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), RpcError> {
        self.get_myself().cast(
                RpcMessage::Request {
                    method: RpcRequestMethod::Send { 
                        program_id, from, to, amount, sig, tx_hash 
                    }, 
                    reply 
                }
            ).map_err(|e| {
                RpcError::Custom(e.to_string())
            }
        )
    }

    async fn send_rpc_deploy_method_to_self(
        &self,
        program_id: Address,
        sig: RecoverableSignature,
        tx_hash: String,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), RpcError> {
        self.get_myself().cast(
            RpcMessage::Request {
                method: RpcRequestMethod::Deploy { program_id, sig, tx_hash },
                reply
            }
        ).map_err(|e| {
            RpcError::Custom(e.to_string())
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
        println!("RPC Actor Received RPC Message");
        match message {
            RpcMessage::Request {
                method, reply 
            } => {
                self.handle_request_method(method, reply).await?;
            },
            RpcMessage::Response { response, reply } => {
                let reply = reply.ok_or(
                    Box::new(
                        RpcError::Custom("Unable to acquire rpc reply sender in RpcMessage::Response".to_string())
                    )
                )?;
                let message = RpcMessage::Response { response, reply: None };
                self.handle_response_data(message, reply).await?;
            },
            RpcMessage::DeploySuccess { reply } => {
                let reply = reply.ok_or(
                    Box::new(
                        RpcError::Custom("Unable to acquire rpc reply sender in RpcMessage::Response".to_string())
                    )
                )?;
                let msg = RpcMessage::DeploySuccess { reply: None  };
                self.handle_deploy_response_data(msg, reply).await?;
            }
        }

        Ok(())
    }
}
