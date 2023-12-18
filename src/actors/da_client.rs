use std::fmt::Display;

use async_trait::async_trait;
use ractor::{ActorRef, Actor, ActorProcessingErr};
use thiserror::Error;
use crate::RegistryMember;

use super::{messages::{RegistryMessage, RegistryActor, DaClientMessage}, types::ActorType};

#[derive(Clone, Debug)]
pub struct DaClient {
    registry: ActorRef<RegistryMessage>,
}

#[derive(Clone, Debug, Error)]
pub enum DaClientError {
    Custom(String)
}

impl Display for DaClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Default for DaClientError {
    fn default() -> Self {
        DaClientError::Custom(
            "DA Client unable to acquire actor".to_string()
        )
    }
}

impl RegistryMember for DaClient {
    type Err = DaClientError;
}

impl DaClient {
    pub fn new(registry: ActorRef<RegistryMessage>) -> Self {
        Self { registry }
    }

    pub fn register_self(&self, myself: ActorRef<DaClientMessage>) -> Result<(), Box<dyn std::error::Error>> {
        self.registry.cast(
            RegistryMessage::Register(
                ActorType::DaClient, 
                RegistryActor::DaClient(myself)
            )
        ).map_err(|e| Box::new(e))?;
        Ok(())
    }
}


#[async_trait]
impl Actor for DaClient {
    type Msg = DaClientMessage;
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
        _: ActorRef<Self::Msg>,
        message: Self::Msg,
        _: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        println!("DaClient Received RPC Call");
        return Ok(())
    }
}
