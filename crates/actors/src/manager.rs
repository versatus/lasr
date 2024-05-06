use lasr_messages::{ActorType, BlobCacheMessage, SupervisorType};
use ractor::{concurrency::JoinHandle, Actor, ActorName, ActorRef};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{get_actor_ref, BlobCacheActor, BlobCacheSupervisorError};

#[derive(Debug, Error)]
pub enum ActorManagerError {
    #[error("a panicked {0:?} actor was successfully respawned but the supervisor could not be acquired from the registry, the panicked actor could not be restarted")]
    RespawnFailed(ActorType),

    #[error("{0}")]
    Custom(String),
}

/// Container for managing actor supervisors and their children.
///
/// Each [`ActorPair`] is a container for the supervised actor's `JoinHandle`,
/// and any information necessary to restart it.
/// The [`ActorManager`] is responsible for the liveliness of the supervised actor,
/// respawning actors should it receive a panic signal from the actor's supervisor.
pub struct ActorManager {
    blob_cache: ActorPair<BlobCacheMessage>,
}

impl ActorManager {
    pub fn new(blob_cache: ActorPair<BlobCacheMessage>) -> Self {
        Self { blob_cache }
    }

    pub async fn respawn_blob_cache(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ActorName,
        handler: BlobCacheActor,
    ) -> Result<(), ActorManagerError> {
        let mut guard = actor_manager.lock().await;
        let supervisor = guard.blob_cache.get_supervisor();
        let actor = Actor::spawn_linked(Some(actor_name), handler, (), supervisor.get_cell())
            .await
            .map_err(|e| ActorManagerError::Custom(e.to_string()))?;
        if let Some(supervisor) =
            get_actor_ref::<BlobCacheMessage, BlobCacheSupervisorError>(SupervisorType::BlobCache)
        {
            let actor_link = ActorPair::new(supervisor, actor);
            guard.blob_cache = actor_link;
        } else {
            return Err(ActorManagerError::RespawnFailed(ActorType::BlobCache));
        }
        Ok(())
    }
}

/// A supervisor actor, and an actor linked to it.
///
/// Used to keep the supervised actor alive, and
/// respawn it if it should panic.
pub struct ActorPair<M: Sized> {
    supervisor: ActorRef<M>,
    _actor: (ActorRef<M>, JoinHandle<()>),
}
impl<M: Sized> ActorPair<M> {
    pub fn new(supervisor: ActorRef<M>, actor: (ActorRef<M>, JoinHandle<()>)) -> Self {
        Self {
            supervisor,
            _actor: actor,
        }
    }

    /// Return a reference only to the supervisor actor.
    pub(crate) fn get_supervisor(&self) -> &ActorRef<M> {
        &self.supervisor
    }
}
