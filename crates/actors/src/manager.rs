use jsonrpsee::ws_client::WsClient;
use lasr_messages::{
    AccountCacheMessage, ActorName, BatcherMessage, BlobCacheMessage, DaClientMessage,
    EngineMessage, EoMessage, ExecutorMessage, PendingTransactionMessage, RpcMessage,
    SchedulerMessage, SupervisorType, ValidatorMessage,
};
use ractor::{concurrency::JoinHandle, Actor, ActorRef};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    get_actor_ref, AccountCacheActor, AccountCacheSupervisorError, Batcher, BatcherActor,
    BatcherSupervisorError, BlobCacheActor, BlobCacheSupervisorError, DaClient, DaClientActor,
    DaClientSupervisorError, EngineActor, EngineSupervisorError, EoClient, EoClientActor,
    EoClientSupervisorError, EoServerActor, EoServerSupervisorError, ExecutionEngine,
    ExecutorActor, ExecutorSupervisorError, LasrRpcServerActor, LasrRpcServerSupervisorError,
    PendingTransactionActor, PendingTransactionSupervisorError, StorageRef, TaskScheduler,
    TaskSchedulerSupervisorError, ValidatorActor, ValidatorCore, ValidatorSupervisorError,
};

#[derive(Debug, Error)]
pub enum ActorManagerError {
    #[error("a panicked {0} actor was successfully respawned, but the supervisor could not be acquired from the registry: the panicked actor could not be restarted")]
    RespawnFailed(ractor::ActorName),

    #[error("{0}")]
    Custom(String),
}

/// Container for managing actor supervisors and their children.
///
/// Each [`ActorSpawn`] is a container for the supervised actor's `JoinHandle`.
/// The [`ActorManager`] is responsible for the liveliness of the supervised actor,
/// respawning actors should it receive a panic signal from the actor's supervisor.
pub struct ActorManager {
    blob_cache: ActorSpawn<BlobCacheMessage>,
    account_cache: ActorSpawn<AccountCacheMessage>,
    pending_tx: ActorSpawn<PendingTransactionMessage>,
    lasr_rpc_server: ActorSpawn<RpcMessage>,
    scheduler: ActorSpawn<SchedulerMessage>,
    eo_server: ActorSpawn<EoMessage>,
    engine: ActorSpawn<EngineMessage>,
    validator: ActorSpawn<ValidatorMessage>,
    eo_client: ActorSpawn<EoMessage>,
    da_client: ActorSpawn<DaClientMessage>,
    batcher: ActorSpawn<BatcherMessage>,
    executor: ActorSpawn<ExecutorMessage>,
}

impl ActorManager {
    pub fn get_lasr_rpc_actor_ref(&self) -> ActorRef<RpcMessage> {
        self.lasr_rpc_server.actor.0.clone()
    }

    /// Respawn a panicked [`lasr_actors::BlobCacheActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_blob_cache(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: BlobCacheActor,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) =
            get_actor_ref::<BlobCacheMessage, BlobCacheSupervisorError>(SupervisorType::BlobCache)
        {
            let actor =
                Actor::spawn_linked(Some(actor_name.clone()), handler, (), supervisor.get_cell())
                    .await
                    .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.blob_cache = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::AccountCacheActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_account_cache(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: AccountCacheActor,
        startup_args: StorageRef,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) = get_actor_ref::<AccountCacheMessage, AccountCacheSupervisorError>(
            SupervisorType::AccountCache,
        ) {
            let actor = Actor::spawn_linked(
                Some(actor_name.clone()),
                handler,
                startup_args,
                supervisor.get_cell(),
            )
            .await
            .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.account_cache = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::PendingTransactionActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_pending_tx(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: PendingTransactionActor,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) = get_actor_ref::<
            PendingTransactionMessage,
            PendingTransactionSupervisorError,
        >(SupervisorType::PendingTransaction)
        {
            let actor =
                Actor::spawn_linked(Some(actor_name.clone()), handler, (), supervisor.get_cell())
                    .await
                    .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.pending_tx = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::LasrRpcServerActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_lasr_rpc_server(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: LasrRpcServerActor,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) =
            get_actor_ref::<RpcMessage, LasrRpcServerSupervisorError>(SupervisorType::LasrRpcServer)
        {
            let actor =
                Actor::spawn_linked(Some(actor_name.clone()), handler, (), supervisor.get_cell())
                    .await
                    .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.lasr_rpc_server = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::TaskScheduler`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_scheduler(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: TaskScheduler,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) = get_actor_ref::<SchedulerMessage, TaskSchedulerSupervisorError>(
            SupervisorType::Scheduler,
        ) {
            let actor =
                Actor::spawn_linked(Some(actor_name.clone()), handler, (), supervisor.get_cell())
                    .await
                    .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.scheduler = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::EoServerActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_eo_server(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: EoServerActor,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) =
            get_actor_ref::<EoMessage, EoServerSupervisorError>(SupervisorType::EoServer)
        {
            let actor =
                Actor::spawn_linked(Some(actor_name.clone()), handler, (), supervisor.get_cell())
                    .await
                    .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.eo_server = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::EngineActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_engine(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: EngineActor,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) =
            get_actor_ref::<EngineMessage, EngineSupervisorError>(SupervisorType::Engine)
        {
            let actor =
                Actor::spawn_linked(Some(actor_name.clone()), handler, (), supervisor.get_cell())
                    .await
                    .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.engine = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::ValidatorActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_validator(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: ValidatorActor,
        startup_args: Arc<Mutex<ValidatorCore>>,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) =
            get_actor_ref::<ValidatorMessage, ValidatorSupervisorError>(SupervisorType::Validator)
        {
            let actor = Actor::spawn_linked(
                Some(actor_name.clone()),
                handler,
                startup_args,
                supervisor.get_cell(),
            )
            .await
            .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.validator = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::EoClientActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_eo_client(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: EoClientActor,
        startup_args: Arc<Mutex<EoClient>>,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) =
            get_actor_ref::<EoMessage, EoClientSupervisorError>(SupervisorType::EoClient)
        {
            let actor = Actor::spawn_linked(
                Some(actor_name.clone()),
                handler,
                startup_args,
                supervisor.get_cell(),
            )
            .await
            .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.eo_client = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::DaClientActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_da_client(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: DaClientActor,
        startup_args: Arc<Mutex<DaClient>>,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) =
            get_actor_ref::<DaClientMessage, DaClientSupervisorError>(SupervisorType::DaClient)
        {
            let actor = Actor::spawn_linked(
                Some(actor_name.clone()),
                handler,
                startup_args,
                supervisor.get_cell(),
            )
            .await
            .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.da_client = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::BatcherActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_batcher(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: BatcherActor,
        startup_args: Arc<Mutex<Batcher>>,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) =
            get_actor_ref::<BatcherMessage, BatcherSupervisorError>(SupervisorType::Batcher)
        {
            let actor = Actor::spawn_linked(
                Some(actor_name.clone()),
                handler,
                startup_args,
                supervisor.get_cell(),
            )
            .await
            .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.batcher = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }

    /// Respawn a panicked [`lasr_actors::ExecutorActor`].
    ///
    /// Returns an error if the supervisor can't be acquired from the registry.
    pub async fn respawn_executor(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ractor::ActorName,
        handler: ExecutorActor,
        startup_args: Arc<Mutex<ExecutionEngine<WsClient>>>,
    ) -> Result<(), ActorManagerError> {
        if let Some(supervisor) =
            get_actor_ref::<ExecutorMessage, ExecutorSupervisorError>(SupervisorType::Executor)
        {
            let actor = Actor::spawn_linked(
                Some(actor_name.clone()),
                handler,
                startup_args,
                supervisor.get_cell(),
            )
            .await
            .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
            let actor_spawn = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.executor = actor_spawn;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }
}

/// Custom async builder for constructing an [`ActorManager`].
#[derive(Default)]
pub struct ActorManagerBuilder {
    blob_cache: Option<ActorSpawn<BlobCacheMessage>>,
    account_cache: Option<ActorSpawn<AccountCacheMessage>>,
    pending_tx: Option<ActorSpawn<PendingTransactionMessage>>,
    lasr_rpc_server: Option<ActorSpawn<RpcMessage>>,
    scheduler: Option<ActorSpawn<SchedulerMessage>>,
    eo_server: Option<ActorSpawn<EoMessage>>,
    engine: Option<ActorSpawn<EngineMessage>>,
    validator: Option<ActorSpawn<ValidatorMessage>>,
    eo_client: Option<ActorSpawn<EoMessage>>,
    da_client: Option<ActorSpawn<DaClientMessage>>,
    batcher: Option<ActorSpawn<BatcherMessage>>,
    executor: Option<ActorSpawn<ExecutorMessage>>,
}
impl ActorManagerBuilder {
    pub async fn blob_cache(
        self,
        blob_cache_actor: BlobCacheActor,
        blob_cache_supervisor: ActorRef<BlobCacheMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.blob_cache = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(blob_cache_actor.name()),
                blob_cache_actor,
                (),
                blob_cache_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn account_cache(
        self,
        account_cache_actor: AccountCacheActor,
        startup_args: StorageRef,
        account_cache_supervisor: ActorRef<AccountCacheMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.account_cache = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(account_cache_actor.name()),
                account_cache_actor,
                startup_args,
                account_cache_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn pending_tx(
        self,
        pending_transaction_actor: PendingTransactionActor,
        pending_tx_supervisor: ActorRef<PendingTransactionMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.pending_tx = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(pending_transaction_actor.name()),
                pending_transaction_actor,
                (),
                pending_tx_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn lasr_rpc_server(
        self,
        lasr_rpc_actor: LasrRpcServerActor,
        lasr_rpc_server_supervisor: ActorRef<RpcMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.lasr_rpc_server = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(lasr_rpc_actor.name()),
                lasr_rpc_actor,
                (),
                lasr_rpc_server_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn scheduler(
        self,
        scheduler_actor: TaskScheduler,
        scheduler_supervisor: ActorRef<SchedulerMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.scheduler = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(scheduler_actor.name()),
                scheduler_actor,
                (),
                scheduler_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn eo_server(
        self,
        eo_server_actor: EoServerActor,
        eo_server_supervisor: ActorRef<EoMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.eo_server = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(eo_server_actor.name()),
                eo_server_actor,
                (),
                eo_server_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn engine(
        self,
        engine_actor: EngineActor,
        engine_supervisor: ActorRef<EngineMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.engine = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(engine_actor.name()),
                engine_actor,
                (),
                engine_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn validator(
        self,
        validator_actor: ValidatorActor,
        validator_core: Arc<Mutex<ValidatorCore>>,
        validator_supervisor: ActorRef<ValidatorMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.validator = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(validator_actor.name()),
                validator_actor,
                validator_core,
                validator_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn eo_client(
        self,
        eo_client_actor: EoClientActor,
        eo_client: Arc<Mutex<EoClient>>,
        eo_client_supervisor: ActorRef<EoMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.eo_client = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(eo_client_actor.name()),
                eo_client_actor,
                eo_client,
                eo_client_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn da_client(
        self,
        da_client_actor: DaClientActor,
        da_client: Arc<Mutex<DaClient>>,
        da_client_supervisor: ActorRef<DaClientMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.da_client = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(da_client_actor.name()),
                da_client_actor,
                da_client,
                da_client_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn batcher(
        self,
        batcher_actor: BatcherActor,
        batcher: Arc<Mutex<Batcher>>,
        batcher_supervisor: ActorRef<BatcherMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.batcher = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(batcher_actor.name()),
                batcher_actor,
                batcher,
                batcher_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub async fn executor(
        self,
        executor_actor: ExecutorActor,
        execution_engine: Arc<Mutex<ExecutionEngine<WsClient>>>,
        executor_supervisor: ActorRef<ExecutorMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.executor = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(executor_actor.name()),
                executor_actor,
                execution_engine,
                executor_supervisor.get_cell(),
            )
            .await
            .map_err(Box::new)?,
        ));
        Ok(new)
    }

    pub fn build(self) -> ActorManager {
        ActorManager {
            blob_cache: self.blob_cache.expect("blob cache actor failed to start"),
            account_cache: self
                .account_cache
                .expect("account cache actor failed to start"),
            pending_tx: self
                .pending_tx
                .expect("pending transaction actor failed to start"),
            lasr_rpc_server: self
                .lasr_rpc_server
                .expect("lasr rpc server actor failed to start"),
            scheduler: self
                .scheduler
                .expect("task scheduler actor failed to start"),
            eo_server: self.eo_server.expect("eo server actor failed to start"),
            engine: self.engine.expect("engine actor failed to start"),
            validator: self.validator.expect("validator actor failed to start"),
            eo_client: self.eo_client.expect("eo client actor failed to start"),
            da_client: self.da_client.expect("da client actor failed to start"),
            batcher: self.batcher.expect("batcher actor failed to start"),
            executor: self.executor.expect("executor actor failed to start"),
        }
    }
}

/// A supervised actor spawn.
///
/// Used to keep the supervised actor alive, and
/// respawn it if it should panic.
pub struct ActorSpawn<M: Sized> {
    actor: (ActorRef<M>, JoinHandle<()>),
}
impl<M: Sized> ActorSpawn<M> {
    pub fn new(actor: (ActorRef<M>, JoinHandle<()>)) -> Self {
        Self { actor }
    }
}

#[cfg(test)]
mod actor_manager_tests {
    use std::sync::Arc;

    use super::ActorSpawn;
    use crate::{get_actor_ref, ActorManagerError};

    use async_trait::async_trait;
    use ractor::{
        errors::ActorProcessingErr, Actor, ActorCell, ActorRef, ActorStatus, SupervisionEvent,
    };
    use ractor_cluster::RactorMessage;
    use thiserror::Error;
    use tokio::sync::{
        mpsc::{self, Sender},
        Mutex,
    };

    // Minimal reproduction types for proving the effectiveness
    // of the ActorManager's ability to respawn panicked actors

    struct TestSupervisor {
        panic_tx: Sender<ActorCell>,
    }

    #[async_trait]
    impl Actor for TestSupervisor {
        type Msg = TestMessage;
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
            match message {
                SupervisionEvent::ActorTerminated(who, ..) => {
                    self.panic_tx
                        .send(who)
                        .await
                        .map_err(|e| println!("{e:?}"))
                        .unwrap();
                }
                _ => {}
            }
            Ok(())
        }
    }

    #[derive(Debug, Error, Default)]
    pub enum TestSupervisorError {
        #[default]
        #[error("failed to acquire TestSupervisor from registry")]
        RactorRegistryError,
    }

    #[derive(Clone)]
    struct TestActor;

    #[async_trait]
    impl Actor for TestActor {
        type Msg = TestMessage;
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
            myself: ActorRef<Self::Msg>,
            message: Self::Msg,
            _state: &mut Self::State,
        ) -> Result<(), ActorProcessingErr> {
            match message {
                TestMessage::Panic => myself.get_cell().kill(),
                TestMessage::Continue => {}
            }
            Ok(())
        }
    }

    #[allow(unused)]
    #[derive(RactorMessage)]
    enum TestMessage {
        Panic,
        Continue,
    }

    struct TestActorManager {
        pub(crate) test_actor: ActorSpawn<TestMessage>,
    }
    impl TestActorManager {
        async fn new(test_supervisor: ActorRef<TestMessage>, test_actor: TestActor) -> Self {
            Self {
                test_actor: ActorSpawn::new(
                    Actor::spawn_linked(
                        Some("test_actor".to_string()),
                        test_actor,
                        (),
                        test_supervisor.get_cell(),
                    )
                    .await
                    .expect("failed to spawn test actor"),
                ),
            }
        }

        pub async fn respawn_test_actor(
            actor_manager: Arc<Mutex<TestActorManager>>,
            actor_name: ractor::ActorName,
            handler: TestActor,
        ) -> Result<(), ActorManagerError> {
            if let Some(supervisor) =
                get_actor_ref::<TestMessage, TestSupervisorError>("test_supervisor")
            {
                let actor = Actor::spawn_linked(
                    Some(actor_name.clone()),
                    handler,
                    (),
                    supervisor.get_cell(),
                )
                .await
                .map_err(|e| ActorManagerError::Custom(e.to_string()))
                .unwrap();
                let actor_spawn = ActorSpawn::new(actor);
                {
                    let mut guard = actor_manager.lock().await;
                    guard.test_actor = actor_spawn;
                }
                Ok(())
            } else {
                Err(ActorManagerError::RespawnFailed(actor_name))
            }
        }
    }

    // Constructs a test actor and supervisor, sends a signal to stop the supervised actor
    // and waits up to 10s to restart the stopped actor. In production this process will
    // only occur when an actor panics.
    #[tokio::test]
    async fn test_panicked_actor_can_be_respawned() {
        // create panic mpsc::channel
        let (panic_tx, mut panic_rx) = mpsc::channel(1);
        // start supervisor with panic tx
        let supervisor = TestSupervisor { panic_tx };
        let (supervisor_ref, _handle) =
            Actor::spawn(Some("test_supervisor".to_string()), supervisor, ())
                .await
                .expect("failed to spawn test supervisor");
        // build TestActorManager which spawns a supervised actor
        let test_actor = TestActor;
        let actor_manager = Arc::new(Mutex::new(
            TestActorManager::new(supervisor_ref, test_actor.clone()).await,
        ));
        // send panic message to test actor
        let test_actor_ref = {
            let guard = actor_manager.lock().await;
            guard.test_actor.actor.0.clone()
        };
        test_actor
            .handle(test_actor_ref, TestMessage::Panic, &mut ())
            .await
            .expect("failed to send panic message");
        // spawn thread with panic rx awaiting panicked actor signal
        let actor_manager_clone = actor_manager.clone();
        tokio::spawn(async move {
            while let Some(actor) = panic_rx.recv().await {
                // assert the actor status is stopped & panic actor is the expected actor
                assert!(
                    actor.get_status() == ActorStatus::Stopped
                        || actor.get_status() == ActorStatus::Stopping
                );
                assert_eq!(actor.get_name(), Some("test_actor".to_string()));
                let actor_name = actor.get_name().unwrap();
                let manager_ptr = Arc::clone(&actor_manager_clone);
                TestActorManager::respawn_test_actor(manager_ptr, actor_name, test_actor.clone())
                    .await
                    .map_err(|e| println!("{e:?}"))
                    .unwrap();
            }
        });
        // respawn the actor and assert the actor status is running, fail if the process takes longer than 10s
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
        let mut timeout = 0;
        loop {
            assert_ne!(timeout, 6);
            {
                let guard = actor_manager.lock().await;
                if guard.test_actor.actor.0.get_status() == ActorStatus::Running {
                    break;
                }
            }
            interval.tick().await;
            timeout += 1;
        }
    }
}
