use async_trait::async_trait;
use ethereum_types::U256;
use ractor::{Actor, ActorRef, ActorProcessingErr, RpcReplyPort, concurrency::oneshot};
use crate::{
    rpc::LasrRpcServer, account::{ Address, Token}, 
    certificate::RecoverableSignature, actors::handle_actor_response, create_handler
};
use jsonrpsee::core::Error as RpcError;
use super::{messages::{RpcMessage, SchedulerMessage}, types::{RpcRequestMethod, ActorType}};

#[allow(unused)]
//TODO(asmith): Integrate timeouts
use super::types::TIMEOUT_DURATION;

#[derive(Debug)]
pub struct LasrRpcServerImpl {
    proxy: ActorRef<RpcMessage>,
}

#[derive(Debug, Clone)]
pub struct LasrRpcServerActor;

impl LasrRpcServerActor {
    pub fn new() -> Self {
        Self 
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
        op: String,
        inputs: String,
        sig: RecoverableSignature,
        nonce: U256, 
        reply: RpcReplyPort<RpcMessage>,
    ) -> Result<(), ActorProcessingErr> {
        Ok(scheduler.cast(
            SchedulerMessage::Call { program_id, from, op, inputs, sig, nonce, rpc_reply: reply }
        ).map_err(|e| Box::new(e))?)
    }

    fn handle_send_request(
        &self,
        scheduler: ActorRef<SchedulerMessage>,
        program_id: Address,
        from: Address,
        to: Address,
        amount: U256,
        sig: RecoverableSignature,
        nonce: U256,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), ActorProcessingErr> {
        Ok(scheduler.cast(
            SchedulerMessage::Send { program_id, from, to, amount, sig, nonce, rpc_reply: reply }
        ).map_err(|e| Box::new(e))?)
    }

    fn handle_deploy_request(
        &self,
        scheduler: ActorRef<SchedulerMessage>,
        program_id: Address,
        sig: RecoverableSignature,
        nonce: U256,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), ActorProcessingErr> {
        Ok(scheduler.cast(
            SchedulerMessage::Deploy { program_id, sig, nonce, rpc_reply: reply }
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
                program_id, from, op, inputs, sig, nonce
            } => {
                self.handle_call_request(scheduler, program_id, from, op, inputs, sig, nonce, reply)
            },
            RpcRequestMethod::Send { 
                program_id, from, to, amount, sig, nonce
            } => {
                self.handle_send_request(scheduler, program_id, from, to, amount, sig, nonce, reply)
            },
            RpcRequestMethod::Deploy { 
                program_id, sig, nonce
            } => {
                self.handle_deploy_request(scheduler, program_id, sig, nonce, reply)
            }
        }
    }

    async fn get_scheduler(&self) -> Result<Option<ActorRef<SchedulerMessage>>, RpcError> {
        if let Some(actor) = ractor::registry::where_is(ActorType::Scheduler.to_string()) {
            return Ok(Some(actor.into()))
        }

        Err(RpcError::Custom("unable to acquire scheduler".to_string()))
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
        op: String,
        inputs: String,
        sig: RecoverableSignature, 
        nonce: U256,
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
            op,
            inputs,
            sig,
            nonce,
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
        to: Address,
        amount: U256,
        sig: RecoverableSignature,
        nonce: U256,
    ) -> Result<Token, jsonrpsee::core::Error> {
        println!("Received RPC send method");
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx); 

        self.send_rpc_send_method_to_self(
            program_id, from, to, amount, sig, nonce, reply
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
        nonce: U256
    ) -> Result<(), jsonrpsee::core::Error> {
        println!("Received RPC deploy method"); 
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx); 

        self.send_rpc_deploy_method_to_self(
            program_id,
            sig,
            nonce,
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
        op: String,
        inputs: String,
        sig: RecoverableSignature,
        nonce: U256,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), RpcError> {
        println!("Sending RPC call method to proxy actor");
        self.proxy
            .cast(
                RpcMessage::Request {
                    method: RpcRequestMethod::Call {
                        program_id,
                        from,
                        op,
                        inputs,
                        sig,
                        nonce
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
        to: Address,
        amount: U256,
        sig: RecoverableSignature,
        nonce: U256,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), RpcError> {
        self.get_myself().cast(
                RpcMessage::Request {
                    method: RpcRequestMethod::Send { 
                        program_id, from, to, amount, sig, nonce 
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
        nonce: U256,
        reply: RpcReplyPort<RpcMessage>
    ) -> Result<(), RpcError> {
        self.get_myself().cast(
            RpcMessage::Request {
                method: RpcRequestMethod::Deploy { program_id, sig, nonce },
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
