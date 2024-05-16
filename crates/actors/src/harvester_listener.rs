#![allow(unused)]
use crate::{
    check_account_cache, check_da_for_account, create_handler, da_client, eo_server, get_actor_ref,
    handle_actor_response, Coerce, SchedulerError,
};
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use jsonrpsee::core::Error as RpcError;
use lasr_messages::{
    AccountCacheMessage, ActorName, ActorType, DaClientMessage, EngineMessage, EoMessage,
    HarvesterListenerMessage, RpcMessage, RpcResponseError, SchedulerMessage, SupervisorType,
    TransactionResponse, ValidatorMessage,
};
use lasr_types::{Account, Address, RecoverableSignature, Transaction};
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
pub enum HarvesterListenerError {
    #[error("failed to acquire HarvesterListenerActor from registry")]
    RactorRegistryError,

    #[error("{0}")]
    Custom(String),
}

impl Default for HarvesterListenerError {
    fn default() -> Self {
        HarvesterListenerError::RactorRegistryError
    }
}

pub type MethodResults = Arc<Mutex<FuturesUnordered<Result<(), Box<dyn std::error::Error>>>>>;

/// The actor struct for the harvester listener actor
#[derive(Debug, Clone, Default)]
pub struct HarvesterListenerActor;
impl ActorName for HarvesterListenerActor {
    fn name(&self) -> ractor::ActorName {
        ActorType::HarvesterListener.to_string()
    }
}

impl HarvesterListenerActor {
    /// Creates a new HarvesterListenerActor with a reference to the Registry actor
    pub fn new() -> Self {
        Self
    }

    async fn send_message_to_scheduler(
        &self,
        message: SchedulerMessage,
    ) -> Result<(), HarvesterListenerError> {
        if let Some(scheduler_actor) = Self::get_scheduler().await {
            scheduler_actor
                .cast(message)
                .map_err(|e| HarvesterListenerError::Custom(e.to_string()))?;
        } else {
            return Err(HarvesterListenerError::RactorRegistryError);
        }

        Ok(())
    }

    async fn get_scheduler() -> Option<ActorRef<SchedulerMessage>> {
        get_actor_ref::<SchedulerMessage, SchedulerError>(ActorType::Scheduler)
    }
}

#[async_trait]
impl Actor for HarvesterListenerActor {
    type Msg = HarvesterListenerMessage;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            HarvesterListenerMessage::TransactionApplied(transaction_hash, token) => {
                self.send_message_to_scheduler(SchedulerMessage::TransactionApplied {
                    transaction_hash,
                    token,
                })
                .await?;
            }
            HarvesterListenerMessage::RegistrationSuccess(transaction, program_id) => {
                self.send_message_to_scheduler(SchedulerMessage::RegistrationSuccess {
                    transaction,
                    program_id,
                })
                .await?;
            }
            HarvesterListenerMessage::CallTransactionApplied(transaction_hash, account) => {
                self.send_message_to_scheduler(SchedulerMessage::CallTransactionApplied {
                    transaction_hash,
                    account,
                })
                .await?;
            }
            HarvesterListenerMessage::CallTransactionFailure(transaction_hash, outputs, error) => {
                self.send_message_to_scheduler(SchedulerMessage::CallTransactionFailure {
                    transaction_hash,
                    outputs,
                    error,
                })
                .await?;
            }
            _ => {}
        }

        Ok(())
    }
}

pub struct HarvesterListenerSupervisor {
    panic_tx: Sender<ActorCell>,
}
impl HarvesterListenerSupervisor {
    pub fn new(panic_tx: Sender<ActorCell>) -> Self {
        Self { panic_tx }
    }
}
impl ActorName for HarvesterListenerSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::HarvesterListener.to_string()
    }
}
#[derive(Debug, Error, Default)]
pub enum HarvesterListenerSupervisorError {
    #[default]
    #[error("failed to acquire HarvesterListenerSupervisor from registry")]
    RactorRegistryError,
}

#[async_trait]
impl Actor for HarvesterListenerSupervisor {
    type Msg = HarvesterListenerMessage;
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
                log::warn!("process group changed: {:?}", m.get_group());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PendingTransactionActor, TaskScheduler};
    use anyhow::anyhow;
    use bincode::serialize;
    use eo_listener::EoServerError;
    use lasr_messages::AccountCacheMessage;
    use lasr_messages::ActorType::{HarvesterListener, PendingTransactions};
    use lasr_types::{Address, AddressOrNamespace, Token, Transaction, TransactionType, U256};
    use log::info;
    use ractor::concurrency::Duration;
    use ractor::{ActorRef, Message};
    use ractor_cluster::NodeServer;
    use std::sync::Arc;

    #[tokio::test]
    async fn handle_transaction_applied() {
        pub struct FakeSchedulerActor;

        #[async_trait]
        impl Actor for FakeSchedulerActor {
            type Msg = SchedulerMessage;
            type State = ();
            type Arguments = ();

            async fn pre_start(
                &self,
                myself: ActorRef<Self::Msg>,
                _: (),
            ) -> Result<Self::State, ActorProcessingErr> {
                Ok(())
            }

            async fn handle(
                &self,
                _myself: ActorRef<Self::Msg>,
                message: Self::Msg,
                _state: &mut Self::State,
            ) -> Result<(), ActorProcessingErr> {
                return match message {
                    SchedulerMessage::TransactionApplied {
                        transaction_hash,
                        token,
                    } => Ok(()),
                    (_) => {
                        panic!("unexpected message: {:?}", message);
                    }
                };
            }
        }

        simple_logger::init_with_level(log::Level::Info)
            .map_err(|e| EoServerError::Other(e.to_string()))
            .unwrap();
        let fake_scheduler = FakeSchedulerActor;

        let fake_scheduler_ref =
            Actor::spawn(Some(ActorType::Scheduler.to_string()), fake_scheduler, ())
                .await
                .expect("unable to spawn fake scheduler actor");

        let harvester_listener_actor = HarvesterListenerActor::new();
        let (harvester_listener_actor_ref, _) = Actor::spawn(
            Some(HarvesterListener.to_string()),
            harvester_listener_actor,
            (),
        )
        .await
        .expect("unable to spawn validator actor");

        let string = "".to_string();

        let transaction = Transaction::default();

        let token = transaction.into();

        let message = HarvesterListenerMessage::TransactionApplied(string, token);

        let result = harvester_listener_actor_ref.cast(message);

        assert!(result.is_ok());

        ractor::concurrency::sleep(Duration::from_millis(1000)).await;

        assert!(ractor::registry::where_is(ActorType::Scheduler.to_string()).is_some());
    }

}
