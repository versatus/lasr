#![allow(unused)]
use std::{fmt::Display, sync::Arc};

use crate::{create_handler, ActorExt, StaticFuture, UnorderedFuturePool};
use async_trait::async_trait;
use eo_listener::{EoServer as InnerEoServer, EventType};
use futures::{
    stream::{FuturesUnordered, StreamExt},
    FutureExt,
};
use jsonrpsee::core::Error as RpcError;
use lasr_types::{Account, Address, Token};
use ractor::{
    concurrency::{oneshot, OneshotSender},
    Actor, ActorCell, ActorProcessingErr, ActorRef, ActorStatus, Message, RpcReplyPort,
    SupervisionEvent,
};
use thiserror::Error;
use tokio::sync::{mpsc::Sender, Mutex};
use web3::ethabi::{Address as EthereumAddress, FixedBytes, Log, LogParam};

use crate::{handle_actor_response, scheduler::SchedulerError};

use lasr_messages::{
    ActorName, ActorType, BridgeEvent, BridgeEventBuilder, DaClientMessage, EngineMessage, EoEvent,
    EoMessage, SchedulerMessage, SettlementEvent, SettlementEventBuilder, SupervisorType,
    ValidatorMessage,
};

#[derive(Clone, Debug, Default)]
pub struct EoServerActor;

impl ActorName for EoServerActor {
    fn name(&self) -> ractor::ActorName {
        ActorType::EoServer.to_string()
    }
}

pub struct EoServerWrapper {
    server: InnerEoServer,
}

impl EoServerWrapper {
    pub fn new(server: InnerEoServer) -> Self {
        Self { server }
    }

    pub async fn run(mut self) -> Result<(), EoServerError> {
        let eo_actor: ActorRef<EoMessage> =
            ractor::registry::where_is(ActorType::EoServer.to_string())
                .ok_or(EoServerError::Custom(
                    "unable to acquire eo_actor".to_string(),
                ))?
                .into();

        if let Err(e) = self.server.load_processed_blocks().await {
            log::error!("unable to load processed blocks from file: {}", e);
        }

        loop {
            let logs = self.server.next().await;
            if let Ok(log) = &logs.log_result {
                if !log.is_empty() {
                    log::info!("non-empty log found: {:?}", log);
                    eo_actor
                        .cast(EoMessage::Log {
                            log_type: logs.event_type,
                            log: log.to_vec(),
                        })
                        .map_err(|e| EoServerError::Custom(e.to_string()))?;

                    self.server.save_blocks_processed();
                }
            }

            if let ActorStatus::Stopped = eo_actor.get_status() {
                log::error!("EO Actor stopped");
                break;
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Error)]
pub enum EoServerError {
    Custom(String),
}

impl Default for EoServerError {
    fn default() -> Self {
        EoServerError::Custom("unable to acquire actor".to_string())
    }
}

impl Display for EoServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl EoServerActor {
    pub fn new() -> Self {
        Self
    }

    fn handle_eo_event(events: EoEvent) -> Result<(), EoServerError> {
        log::warn!("discovered EO event: {:?}", events);
        let message = EngineMessage::EoEvent { event: events };
        let engine: ActorRef<EngineMessage> =
            ractor::registry::where_is(ActorType::Engine.to_string())
                .ok_or(EoServerError::Custom(
                    "unable to acquire engine".to_string(),
                ))?
                .into();
        let _ = engine
            .cast(message)
            .map_err(|e| EoServerError::Custom(e.to_string()));
        Ok(())
    }

    fn parse_bridge_log(
        logs: Vec<Log>,
    ) -> Result<Vec<BridgeEvent>, Box<dyn std::error::Error + Send + Sync>> {
        log::warn!("Parsing bridge event: {:?}", logs);
        let mut events = Vec::new();
        let mut bridge_event = BridgeEventBuilder::default();
        for log in logs {
            for param in log.params {
                match &param.name[..] {
                    "user" => {
                        bridge_event.user(
                            param
                                .value
                                .clone()
                                .into_address()
                                .ok_or(EoServerActor::boxed_custom_eo_error(&param))?,
                        );
                    }
                    "tokenAddress" => {
                        bridge_event.program_id(
                            param
                                .value
                                .clone()
                                .into_address()
                                .ok_or(EoServerActor::boxed_custom_eo_error(&param))?,
                        );
                    }
                    "amount" => {
                        bridge_event.amount(
                            param
                                .value
                                .clone()
                                .into_uint()
                                .ok_or(EoServerActor::boxed_custom_eo_error(&param))?
                                .into(),
                        );
                    }
                    "tokenId" => {
                        bridge_event.token_id(
                            param
                                .value
                                .clone()
                                .into_uint()
                                .ok_or(EoServerActor::boxed_custom_eo_error(&param))?
                                .into(),
                        );
                    }
                    "tokenType" => {
                        bridge_event.token_type(
                            param
                                .value
                                .clone()
                                .into_string()
                                .ok_or(EoServerActor::boxed_custom_eo_error(&param))?,
                        );
                    }
                    "bridgeEventId" => {
                        bridge_event.bridge_event_id(
                            param
                                .value
                                .clone()
                                .into_uint()
                                .ok_or(EoServerActor::boxed_custom_eo_error(&param))?
                                .into(),
                        );
                    }
                    _ => { /* return error */ }
                }
            }
            let be = bridge_event.build()?;
            events.push(be.clone());
        }
        Ok(events)
    }

    fn parse_settlement_log(
        logs: Vec<Log>,
    ) -> Result<Vec<SettlementEvent>, Box<dyn std::error::Error + Send + Sync>> {
        let mut events = Vec::new();
        let mut settlement_event = SettlementEventBuilder::default();
        for log in logs {
            for param in log.params {
                match &param.name[..] {
                    "user" => {
                        settlement_event.accounts(
                            param
                                .value
                                .clone()
                                .into_array()
                                .ok_or(EoServerActor::boxed_custom_eo_error(&param))?,
                        );
                    }
                    "batchHeaderHash" => {
                        settlement_event.batch_header_hash(
                            param
                                .value
                                .clone()
                                .into_fixed_bytes()
                                .ok_or(EoServerActor::boxed_custom_eo_error(&param))?,
                        );
                    }
                    "blobIndex" => {
                        settlement_event.blob_index(
                            param
                                .value
                                .clone()
                                .into_uint()
                                .ok_or(EoServerActor::boxed_custom_eo_error(&param))?
                                .into(),
                        );
                    }
                    "blobEventId" => {
                        settlement_event.settlement_event_id(
                            param
                                .value
                                .clone()
                                .into_uint()
                                .ok_or(EoServerActor::boxed_custom_eo_error(&param))?
                                .into(),
                        );
                    }
                    _ => { /* return error */ }
                }
            }
            let se = settlement_event.build()?;
            events.push(se.clone());
        }
        Ok(events)
    }

    fn boxed_custom_eo_error(param: &LogParam) -> Box<dyn std::error::Error + Send + Sync> {
        Box::new(EoServerError::Custom(format!(
            "Unable to parse log param value into type: {:?}",
            param
        )))
    }

    fn handle_log(log: Vec<web3::ethabi::Log>, log_type: EventType) {
        match log_type {
            EventType::Bridge(_) => {
                log::warn!("received bridge event");
                let parsed_bridge_log_res = EoServerActor::parse_bridge_log(log);
                match parsed_bridge_log_res {
                    Ok(parsed_bridge_log) => {
                        let res = EoServerActor::handle_eo_event(parsed_bridge_log.into());

                        if let Err(e) = &res {
                            log::error!("eo_server encountered an error: {e:?}");
                        } else {
                            log::info!("{:?}", res);
                        }
                    }
                    Err(e) => {
                        log::error!("Error parsing bridge log: {e:?}");
                    }
                }
            }
            EventType::Settlement(_) => {
                log::info!("eo_server discovered Settlement event");
                let parsed_settlement_log_res = EoServerActor::parse_settlement_log(log);
                match parsed_settlement_log_res {
                    Ok(parsed_settlement_log) => {
                        let res = EoServerActor::handle_eo_event(parsed_settlement_log.into());

                        if let Err(e) = &res {
                            log::error!("eo_server encountered an error: {e:?}");
                        } else {
                            log::info!("{:?}", res);
                        }
                    }
                    Err(e) => {
                        log::error!("Error parsing settlement log: {e:?}");
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Actor for EoServerActor {
    type Msg = EoMessage;
    type State = ();
    type Arguments = Self::State;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::Arguments, ActorProcessingErr> {
        Ok(args)
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        message: Self::Msg,
        _: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            EoMessage::Log { log, log_type } => {
                EoServerActor::handle_log(log, log_type);
            }
            EoMessage::Bridge {
                program_id,
                address,
                amount,
                content,
            } => {
                log::info!("Eo Server ready to bridge assets to EO contract");
            }
            _ => {
                log::info!("Eo Server received unhandled message");
            }
        }
        Ok(())
    }
}

pub struct EoServerSupervisor;
impl EoServerSupervisor {
    pub fn new() -> Self {
        Self
    }
}
impl ActorName for EoServerSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::EoServer.to_string()
    }
}

#[async_trait]
impl Actor for EoServerSupervisor {
    type Msg = EoMessage;
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
            }
            SupervisionEvent::ActorTerminated(who, _, reason) => {
                log::error!("actor terminated: {:?}, err: {:?}", who.get_name(), reason);
            }
            SupervisionEvent::PidLifecycleEvent(event) => {
                log::info!("pid lifecycle event: {:?}", event);
            }
            SupervisionEvent::ProcessGroupChanged(m) => {
                log::warn!("process group changed: {:?}", m.get_group());
            }
        }
        Ok(())
    }
}
