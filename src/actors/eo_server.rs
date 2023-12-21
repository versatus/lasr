#![allow(unused)]
use std::fmt::Display;

use async_trait::async_trait;
use eo_listener::{EoServer as InnerEoServer, EventType};
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::oneshot, RpcReplyPort, Message, ActorCell};
use thiserror::Error;
use web3::ethabi::{Log, FixedBytes, Address, LogParam};
use jsonrpsee::core::Error as RpcError;
use crate::{create_handler, TheatreMember, traits, RegistryMember};

use super::{
    handle_actor_response,
    messages::{
        RegistryMessage, RegistryActor, RegistryResponse, EoMessage, SchedulerMessage, EoEvent, SettlementEvent, BridgeEvent,
        SettlementEventBuilder, BridgeEventBuilder, EngineMessage, ValidatorMessage, DaClientMessage,
    }, types::ActorType,
    scheduler::SchedulerError
};

#[derive(Clone, Debug)]
pub struct EoServer {
    registry: ActorRef<RegistryMessage>
}

pub struct EoServerWrapper {
    actor: ActorRef<EoMessage>,
    server: InnerEoServer
}

impl EoServerWrapper {
    pub fn new(actor: ActorRef<EoMessage>, server: InnerEoServer) -> Self {
        Self {
            actor,
            server
        }
    }

    pub async fn run(mut self) -> Result<(), EoServerError> {
        loop {
            let logs = self.server.next().await;
            if let Ok(log) = &logs.log_result {
                if log.len() > 0 {
                    self.actor.cast(
                        EoMessage::Log { 
                            log_type: logs.event_type, 
                            log: log.to_vec() 
                    }).map_err(|e| {
                        EoServerError::Custom(e.to_string())
                    })?;
                } 
            } else {
                log::info!("No logs found");
            }
        }

        #[allow(unreachable_code)]
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

impl RegistryMember for EoServer {
    type Err = EoServerError;
}

impl EoServer {
    pub fn new(registry: ActorRef<RegistryMessage>) -> Self {
        Self { registry }
    }

    pub fn register_self(&self, myself: ActorRef<EoMessage>) -> Result<(), Box<dyn std::error::Error>> {
        self.registry.cast(
            RegistryMessage::Register(
                ActorType::EoServer, 
                RegistryActor::EoServer(myself)
            )
        ).map_err(|e| Box::new(e))?;
        Ok(())
    }

    async fn handle_eo_event(&self, events: EoEvent) -> Result<(), EoServerError> {
        log::info!("Handling EO event: {:?}, by sending to Engine", events);
        let message = EngineMessage::EoEvent { event: events };
        let handler = create_handler!(get_engine);
        self.send_to_actor::<EngineMessage, _, Option<ActorRef<EngineMessage>>, ActorRef<EngineMessage>>(
            handler, ActorType::Engine, message, self.registry.clone()
        ).await
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
                        settlement_event.user(
                            param.value.clone().into_address().ok_or(
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
                            param.value.clone().into_string().ok_or(
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
                        self.handle_eo_event(
                            self.parse_bridge_log(log)?.into()
                        ).await?;
                    },
                    EventType::Settlement(_) => {
                        self.handle_eo_event(
                            self.parse_settlement_log(log)?.into()
                        ).await?;
                    }
                }
            },
            EoMessage::Bridge { program_id, address, amount, content } => {
                log::info!("Eo Server ready to bridge assets to EO contract");
            },
            EoMessage::Settle { address, batch_header_hash, blob_index } => {
                log::info!("Eo server ready to settle blob index to EO contract");
                log::info!("batch_header_hash: {:?}, blob_index: {:?}", batch_header_hash, blob_index);
            },
            EoMessage::GetAccountBlobIndex { address, .. } => {
                log::info!("Eo Server Requesting Blob Index for address: {:?}", address);
            },
            EoMessage::GetContractBlobIndex { program_id, .. } => {
                log::info!("Eo Server Requesting Blob Index for program id: {:?}", program_id);
            }

            _ => { log::info!("{:?}", &message); }
        }
        return Ok(())
    }
}
