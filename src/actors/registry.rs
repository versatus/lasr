use async_trait::async_trait;
use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::HashMap;
use super::types::ActorType;
use super::messages::{RegistryMessage, RegistryActor};

pub struct ActorRegistry;

#[async_trait]
impl Actor for ActorRegistry {
    type Msg = RegistryMessage;
    type State = HashMap<ActorType, RegistryActor>;
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(HashMap::new())
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            RegistryMessage::Register(actor_type, actor) => {
                match actor {
                    RegistryActor::Scheduler(actor_ref) => {
                        let _ = state.entry(actor_type).or_insert(RegistryActor::Scheduler(actor_ref));
                    },
                    RegistryActor::RpcServer(actor_ref) => {
                        let _ = state.entry(actor_type).or_insert(RegistryActor::RpcServer(actor_ref));
                    },
                    RegistryActor::Engine(actor_ref) => {
                        let _ = state.entry(actor_type).or_insert(RegistryActor::Engine(actor_ref));
                    },
                    RegistryActor::Validator(actor_ref) => {
                        let _ = state.entry(actor_type).or_insert(RegistryActor::Validator(actor_ref));
                    },
                    RegistryActor::EoServer(actor_ref) => {
                        let _ = state.entry(actor_type).or_insert(RegistryActor::EoServer(actor_ref));
                    },
                    RegistryActor::DaClient(actor_ref) => {
                        let _ = state.entry(actor_type).or_insert(RegistryActor::DaClient(actor_ref));
                    }
                }
            },
            RegistryMessage::GetActor(actor_type, reply) => {
                if let Some(actor) = state.get(&actor_type) {
                    reply.send(
                        actor.into()
                    ).map_err(|e| Box::new(e))?;
                }
            }
        }

        Ok(())
    }

}
