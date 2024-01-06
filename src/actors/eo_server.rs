#![allow(unused)]
use std::fmt::Display;

use async_trait::async_trait;
use eo_listener::{EoServer as InnerEoServer, EventType};
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::{oneshot, OneshotSender}, RpcReplyPort, Message, ActorCell, ActorStatus};
use thiserror::Error;
use web3::ethabi::{Log, FixedBytes, Address as EthereumAddress, LogParam};
use jsonrpsee::core::Error as RpcError;
use crate::{create_handler, Account, Address, Token};
use tokio::sync::mpsc::Sender;

use super::{
    handle_actor_response,
    messages::{
        EoMessage, SchedulerMessage, EoEvent, SettlementEvent, BridgeEvent,
        SettlementEventBuilder, BridgeEventBuilder, EngineMessage, ValidatorMessage, DaClientMessage,
    }, types::ActorType,
    scheduler::SchedulerError
};

#[derive(Clone, Debug)]
pub struct EoServer;

pub struct EoServerWrapper {
    server: InnerEoServer
}

impl EoServerWrapper {
    pub fn new(server: InnerEoServer) -> Self {
        Self {
            server
        }
    }

    pub async fn run(mut self) -> Result<(), EoServerError> {
        let eo_actor: ActorCell = ractor::registry::where_is(ActorType::EoServer.to_string()).ok_or(
            EoServerError::Custom(
                "unable to acquire eo_actor".to_string()
            )
        )?;

        loop {
            let logs = self.server.next().await;
            if let Ok(log) = &logs.log_result {
                if log.len() > 0 {
                    let actor: ActorRef<EoMessage> = ractor::registry::where_is(
                        ActorType::EoServer.to_string()
                    ).ok_or(
                        EoServerError::Custom("unable to acquire EO Server Actor".to_string())
                    )?.into();
                    actor.cast(
                        EoMessage::Log { 
                            log_type: logs.event_type, 
                            log: log.to_vec() 
                    }).map_err(|e| {
                        EoServerError::Custom(e.to_string())
                    })?;
                } 
            } 

            if let ActorStatus::Stopped = eo_actor.get_status() {
                break
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Error)]
pub enum EoServerError {
    Custom(String)
}

impl Default for EoServerError {
    fn default() -> Self {
        EoServerError::Custom(
            "unable to acquire actor".to_string()
        )
    }
}

impl Display for EoServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl EoServer {
    pub fn new() -> Self {
        Self 
    }

    async fn handle_eo_event(&self, events: EoEvent) -> Result<(), EoServerError> {
        let message = EngineMessage::EoEvent { event: events };
        let engine: ActorRef<EngineMessage> = ractor::registry::where_is(
            ActorType::Engine.to_string()
        ).ok_or(
            EoServerError::Custom("unable to acquire engine".to_string())
        )?.into();
        engine.cast(message).map_err(|e| EoServerError::Custom(e.to_string()))?;
        Ok(())
    }
    
    fn parse_bridge_log(&self, logs: Vec<Log>) -> Result<Vec<BridgeEvent>, Box<dyn std::error::Error + Send + Sync>> {
        log::info!("Parsing bridge event");
        let mut events = Vec::new();
        let mut bridge_event = BridgeEventBuilder::default();
        for log in logs {
            for param in log.params {
                match &param.name[..] {
                    "user" => {
                        bridge_event.user(
                            param.value.clone().into_address().ok_or(
                                self.boxed_custom_eo_error(&param)
                            )?
                        );
                    },
                    "tokenAddress" => {
                        bridge_event.program_id(
                            param.value.clone().into_address().ok_or(
                                self.boxed_custom_eo_error(&param)
                            )?
                        );
                    },
                    "amount" => {
                        bridge_event.amount(
                            param.value.clone().into_uint().ok_or(
                                self.boxed_custom_eo_error(&param)
                            )?
                        );
                    
                    },
                    "tokenId" => {
                        bridge_event.token_id(
                            param.value.clone().into_uint().ok_or(
                                self.boxed_custom_eo_error(&param)
                            )?
                        );
                    },
                    "tokenType" => {
                        bridge_event.token_type(
                            param.value.clone().into_string().ok_or(
                                self.boxed_custom_eo_error(&param)
                            )?
                        );
                    },
                    "bridgeEventId" => {
                        bridge_event.bridge_event_id(
                            param.value.clone().into_uint().ok_or(
                                self.boxed_custom_eo_error(&param)
                            )?
                        );
                    }
                    _ => {/* return error */}
                }
            }
            let be = bridge_event.build()?;
            events.push(be.clone());
        }
        Ok(events)
    }

    fn parse_settlement_log(&self, logs: Vec<Log>) -> Result<Vec<SettlementEvent>, Box<dyn std::error::Error + Send + Sync>> {
        let mut events = Vec::new();
        let mut settlement_event = SettlementEventBuilder::default();
        for log in logs {
            for param in log.params {
                match &param.name[..] {
                    "user" => {
                        settlement_event.accounts(
                            param.value.clone().into_array().ok_or(
                                self.boxed_custom_eo_error(&param)
                            )?
                        );
                    },
                    "batchHeaderHash" => {
                        settlement_event.batch_header_hash(
                            param.value.clone().into_fixed_bytes().ok_or(
                                self.boxed_custom_eo_error(&param)
                            )?
                        );
                    },
                    "blobIndex" => {
                        settlement_event.blob_index(
                            param.value.clone().into_uint().ok_or(
                                self.boxed_custom_eo_error(&param)
                            )?
                        );
                    },
                    "blobEventId" => {
                        settlement_event.settlement_event_id(
                            param.value.clone().into_uint().ok_or(
                                self.boxed_custom_eo_error(&param)
                            )?
                        );
                    }
                    _ => { /* return error */}
                }
            }
            let se = settlement_event.build()?;
            events.push(se.clone());
        }
        Ok(events)
    }

    fn boxed_custom_eo_error(&self, param: &LogParam) -> Box<dyn std::error::Error + Send + Sync> {
        Box::new(
            EoServerError::Custom(
                format!("Unable to parse log param value into type: {:?}", param)
            )
        )
    }
}


#[async_trait]
impl Actor for EoServer {
    type Msg = EoMessage;
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
        match message {
            EoMessage::Log { log, log_type } => {
                match log_type {
                    EventType::Bridge(_) => {
                        let res = self.handle_eo_event(
                            self.parse_bridge_log(log)?.into()
                        ).await;

                        if let Err(e) = &res {
                            log::error!("eo_server encountered an error: {}", e);
                        } else {
                            log::info!("{:?}", res);
                        }
                    },
                    EventType::Settlement(_) => {
                        log::warn!("eo_server discovered Settlement event");
                        let res = self.handle_eo_event(
                            self.parse_settlement_log(log)?.into()
                        ).await;

                        if let Err(e) = &res {
                            log::error!("eo_server encountered an error: {}", e);
                        } else {
                            log::info!("{:?}", res);
                        }
                    }
                }
            },
            EoMessage::Bridge { program_id, address, amount, content } => {
                log::info!("Eo Server ready to bridge assets to EO contract");
            },
            _ => { log::info!("Eo Server received unhandled message"); }
        }
        return Ok(())
    }
}
