use lasr_messages::{BlobCacheMessage, SupervisorType};
use ractor::{concurrency::JoinHandle, Actor, ActorName, ActorRef};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{get_actor_ref, BlobCacheActor, BlobCacheSupervisorError};

#[derive(Debug, Error)]
pub enum ActorManagerError {
    #[error("a panicked {0} actor was successfully respawned but the supervisor could not be acquired from the registry, the panicked actor could not be restarted")]
    RespawnFailed(ActorName),

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
}

impl ActorManager {
    pub fn new(blob_cache: ActorSpawn<BlobCacheMessage>) -> Self {
        Self { blob_cache }
    }

    /// Respawn a panicked [`lasr_actors::BlobCacheActor`].
    ///
    /// Checks the registry for the supervisor prior to creating a new [`ractor::Actor::spawn_linked`].
    pub async fn respawn_blob_cache(
        actor_manager: Arc<Mutex<ActorManager>>,
        actor_name: ActorName,
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

/// A supervised actor spawn.
///
/// Used to keep the supervised actor alive, and
/// respawn it if it should panic.
pub struct ActorSpawn<M: Sized> {
    _actor: (ActorRef<M>, JoinHandle<()>),
}
impl<M: Sized> ActorSpawn<M> {
    pub fn new(actor: (ActorRef<M>, JoinHandle<()>)) -> Self {
        Self { _actor: actor }
    }
}
