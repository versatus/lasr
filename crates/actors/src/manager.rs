use ractor::{concurrency::JoinHandle, ActorCell, ActorName, ActorRef, ActorStatus};
use std::collections::HashMap;
use tokio::sync::mpsc::Receiver;

#[derive(Debug)]
pub struct ActorManager;

impl ActorManager {
    pub async fn start(mut rx: Receiver<ActorCell>) {
        let mut actor_map: HashMap<ActorName, ActorPair> = HashMap::with_capacity(12);
        while let Some(actor) = rx.recv().await {
            if let ActorStatus::Stopped = actor.get_status() {
                let name = actor.get_name();
            }
        }
    }
}
