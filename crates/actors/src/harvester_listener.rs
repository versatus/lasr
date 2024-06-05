#![allow(unused)]
use crate::{
    check_account_cache, create_handler, da_client, eo_server, get_actor_ref,
    handle_actor_response, AccountValue, Batch, Batcher, BatcherError, Coerce,
    PendingTransactionActor, PendingTransactionError, SchedulerError,
};
use async_trait::async_trait;
use eigenda_client::proof::BlobVerificationProof;
use futures::stream::FuturesUnordered;
use lasr_messages::{
    AccountCacheMessage, ActorName, ActorType, DaClientMessage, EngineMessage, EoMessage,
    HarvesterListenerMessage, PendingTransactionMessage, PgGroupType, RpcMessage, RpcResponseError,
    SchedulerMessage, SupervisorType, TransactionResponse, ValidatorMessage,
};
use lasr_types::{Account, Address, NodeType, RecoverableSignature, Transaction};
use ractor::concurrency::OneshotReceiver;
use ractor::{concurrency::oneshot, Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use ractor::{ActorCell, SupervisionEvent};
use std::collections::VecDeque;
use std::sync::Arc;
use std::{collections::HashMap, fmt::Display};
use thiserror::*;
use tikv_client::RawClient as TikvClient;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
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

pub struct HarvesterListener {
    tikv_client: Option<TikvClient>,
}

impl HarvesterListener {
    pub fn new(tikv_client: Option<TikvClient>) -> Self {
        Self { tikv_client }
    }
}

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

    async fn send_message_to_pending_transaction_actor(
        &self,
        message: PendingTransactionMessage,
    ) -> Result<(), HarvesterListenerError> {
        if let Some(pending_transaction_actor) = Self::get_pending_transactions_actor().await {
            pending_transaction_actor
                .cast(message)
                .map_err(|e| HarvesterListenerError::Custom(e.to_string()))?;
        } else {
            return Err(HarvesterListenerError::RactorRegistryError);
        }

        Ok(())
    }

    async fn get_pending_transactions_actor() -> Option<ActorRef<PendingTransactionMessage>> {
        get_actor_ref::<PendingTransactionMessage, PendingTransactionError>(
            ActorType::PendingTransactions,
        )
    }
}

#[async_trait]
impl Actor for HarvesterListenerActor {
    type Msg = HarvesterListenerMessage;
    type State = Arc<Mutex<HarvesterListener>>;
    type Arguments = Arc<Mutex<HarvesterListener>>;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        ractor::pg::join(
            PgGroupType::HarvesterListener.to_string(),
            vec![myself.get_cell()],
        );

        Ok(args)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            HarvesterListenerMessage::TransactionApplied(transaction) => {
                log::info!("Informing Scheduler that the transaction was applied");
                if let Err(err) = self
                    .send_message_to_scheduler(SchedulerMessage::TransactionApplied {
                        transaction_hash: transaction.hash_string(),
                        token: transaction.clone().into(),
                    })
                    .await
                {
                    log::error!(
                        "failed to cast transaction applied message to Scheduler actor: {err:?}"
                    );
                };

                log::info!(
                    "Informing pending transactions that the transaction has been applied successfully"
                );
                if let Err(err) = self
                    .send_message_to_pending_transaction_actor(PendingTransactionMessage::Valid {
                        transaction,
                        cert: None,
                    })
                    .await
                {
                    log::error!(
                        "failed to cast valid call message to pending transactions actor: {err:?}"
                    );
                }
            }
            HarvesterListenerMessage::RegistrationSuccess(transaction, program_id) => {
                self.send_message_to_scheduler(SchedulerMessage::RegistrationSuccess {
                    transaction,
                    program_id,
                })
                .await?;
            }
            HarvesterListenerMessage::CallTransactionApplied(transaction, account, outputs) => {
                log::info!("Informing Scheduler that the call transaction was applied");
                if let Err(err) = self
                    .send_message_to_scheduler(SchedulerMessage::CallTransactionApplied {
                        transaction_hash: transaction.hash_string(),
                        account,
                    })
                    .await
                {
                    log::error!(
                        "failed to cast call transaction applied message to Scheduler actor: {err:?}"
                    );
                };

                log::info!(
                    "Informing pending transactions that the transaction has been applied successfully"
                );

                if let Err(err) = self
                    .send_message_to_pending_transaction_actor(
                        PendingTransactionMessage::ValidCall {
                            outputs,
                            transaction,
                            cert: None,
                        },
                    )
                    .await
                {
                    log::error!(
                        "failed to cast valid call message to pending transactions actor: {err:?}"
                    );
                }
            }
            HarvesterListenerMessage::CallTransactionFailure(transaction_hash, outputs, error) => {
                self.send_message_to_scheduler(SchedulerMessage::CallTransactionFailure {
                    transaction_hash,
                    outputs,
                    error,
                })
                .await?;
            }
            HarvesterListenerMessage::InvalidTransactionNotification(transaction, error) => {
                log::info!("Informing pending transactions that the transaction is invalid");
                if let Err(err) = self
                    .send_message_to_pending_transaction_actor(PendingTransactionMessage::Invalid {
                        transaction,
                        e: Box::new(BatcherError::Custom(error))
                            as Box<dyn std::error::Error + Send>,
                    })
                    .await
                {
                    log::error!(
                        "failed to cast invalid message to pending transactions actor: {err:?}"
                    );
                }
            }
            HarvesterListenerMessage::ForwardAccountWrite(addr, account) => {
                match state.lock().await.tikv_client {
                    Some(ref tikv_client) => {
                        let acc_val = AccountValue { account };
                        // Serialize `Account` data to be stored.
                        if let Some(val) = bincode::serialize(&acc_val).ok() {
                            if tikv_client.put(addr.clone(), val).await.is_ok() {
                                log::warn!(
                                "Inserted Account with address of {addr:?} to persistence layer",
                            )
                            } else {
                                log::error!("failed to push Account data to persistence store")
                            }
                        }
                    }
                    None => {
                        log::warn!("Tikv client not available, ForwardAccountWrite received from Harvester");
                    }
                }
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
    use lasr_messages::{AccountCacheMessage, messages};
    use lasr_messages::ActorType::HarvesterListener as HarvesterListenerType;
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
        pub struct FakePendingTransactionActor;

        #[async_trait]
        impl Actor for FakePendingTransactionActor {
            type Msg = PendingTransactionMessage;
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
                    messages::PendingTransactionMessage::Valid {
                        transaction, cert
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

        let fake_pending_transaction = FakePendingTransactionActor;

        let fake_pending_transaction_ref =
            Actor::spawn(Some(ActorType::PendingTransactions.to_string()), fake_pending_transaction, ())
                .await
                .expect("unable to spawn fake scheduler actor");

        let harvester_listener_actor = HarvesterListenerActor::new();
        let harvester_listener = Arc::new(Mutex::new(HarvesterListener::new(None)));
        let (harvester_listener_actor_ref, _) = Actor::spawn(
            Some(HarvesterListenerType.to_string()),
            harvester_listener_actor,
            harvester_listener,
        )
        .await
        .expect("unable to spawn validator actor");

        ractor::concurrency::sleep(Duration::from_millis(100)).await;

        let transaction = Transaction::default();

        let message = HarvesterListenerMessage::TransactionApplied(transaction);

        let result = harvester_listener_actor_ref.cast(message);

        assert!(result.is_ok());

        ractor::concurrency::sleep(Duration::from_millis(100)).await;

        assert!(ractor::registry::where_is(ActorType::Scheduler.to_string()).is_some());
    }
}
