use lasr_messages::BlobCacheMessage;
use ractor::{concurrency::JoinHandle, Actor, ActorName, ActorRef};

use crate::BlobCacheActor;

pub struct ActorManager {
    blob_cache: ActorPair<BlobCacheMessage>,
}

impl ActorManager {
    pub fn new(blob_cache: ActorPair<BlobCacheMessage>) -> Self {
        Self { blob_cache }
    }

    pub async fn respawn_blob_cache(
        mut self,
        actor_name: ActorName,
        handler: BlobCacheActor,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let supervisor = self.blob_cache.get_supervisor();
        let actor = Actor::spawn_linked(Some(actor_name), handler, (), supervisor.0.get_cell())
            .await
            .map_err(Box::new)?;
        let actor_link = ActorPair::new(supervisor, actor);
        self.blob_cache = actor_link;
        Ok(())
    }
}

/// A supervisor actor, and an actor linked to it.
pub struct ActorPair<M: Sized> {
    supervisor: (ActorRef<M>, JoinHandle<()>),
    _actor: (ActorRef<M>, JoinHandle<()>),
}
impl<M: Sized> ActorPair<M> {
    pub fn new(
        supervisor: (ActorRef<M>, JoinHandle<()>),
        actor: (ActorRef<M>, JoinHandle<()>),
    ) -> Self {
        Self {
            supervisor,
            _actor: actor,
        }
    }

    /// Consume the [`ActorPair`], returning only the supervisor.
    pub(crate) fn get_supervisor(self) -> (ActorRef<M>, JoinHandle<()>) {
        self.supervisor
    }
}
