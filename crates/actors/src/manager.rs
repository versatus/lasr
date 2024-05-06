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
    get_actor_ref, AccountCacheActor, Batcher, BatcherActor, BlobCacheActor,
    BlobCacheSupervisorError, DaClient, DaClientActor, EngineActor, EoClient, EoClientActor,
    EoServerActor, ExecutionEngine, ExecutorActor, LasrRpcServerActor, PendingTransactionActor,
    TaskScheduler, ValidatorActor, ValidatorCore,
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
    /// Checks the registry for the supervisor prior to creating a new [`ractor::Actor::spawn_linked`].
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
            let actor_link = ActorSpawn::new(actor);
            {
                let mut guard = actor_manager.lock().await;
                guard.blob_cache = actor_link;
            }
            Ok(())
        } else {
            Err(ActorManagerError::RespawnFailed(actor_name))
        }
    }
}

/// Custom async builder for constructing a [`ActorManager`].
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
        account_cache_supervisor: ActorRef<AccountCacheMessage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut new = self;
        new.account_cache = Some(ActorSpawn::new(
            Actor::spawn_linked(
                Some(account_cache_actor.name()),
                account_cache_actor,
                (),
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
                engine_actor.clone(),
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
                validator_actor.clone(),
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
                eo_client_actor.clone(),
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
                da_client_actor.clone(),
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
                batcher_actor.clone(),
                batcher.clone(),
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
                executor_actor.clone(),
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
