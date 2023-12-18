#![allow(unused)]
use std::fmt::Display;

use async_trait::async_trait;
use ractor::{ActorRef, Actor, ActorProcessingErr};
use thiserror::Error;
use crate::RegistryMember;

use super::{messages::{RegistryMessage, EngineMessage, RegistryActor, EoEvent}, types::ActorType};

#[derive(Clone, Debug)]
pub struct Engine {
    registry: ActorRef<RegistryMessage>,
}

impl RegistryMember for Engine {
    type Err = EngineError;
}

#[derive(Clone, Debug, Error)]
pub enum EngineError {
    Custom(String)
}

impl Default for EngineError {
    fn default() -> Self {
        EngineError::Custom(
            "Engine is unable to acquire actor".to_string()
        )
    }
}

impl Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Engine {
    pub fn new(registry: ActorRef<RegistryMessage>) -> Self {
        Self { registry }
    }

    pub fn register_self(&self, myself: ActorRef<EngineMessage>) -> Result<(), Box<dyn std::error::Error>> {
        self.registry.cast(
            RegistryMessage::Register(
                ActorType::Engine, 
                RegistryActor::Engine(myself)
            )
        ).map_err(|e| Box::new(e))?;
        Ok(())
    }

    async fn handle_bridge_event() {
        // Check if account exists
        //  1. Check cache first
        //  2. If not in cache, ask EO to get blob index for account 
        //      i. If EO doesn't have blob index for account, 
        //          a. Create account
        //              - This includes adding the bridged token(s) in account
        //          b. Ask DA to store account & retain in cache
        //          c. Await batch header hash and blob index
        //          d. Ask EO to store batch header hash and blob index for account
        //          e. Await settlement event
        //          f. Remove from cache (if not other pending txs since)
        //      ii. If EO does have blob index for account,
        //          a. Get blob from DA
        //          b. Decode blob
        //          c. Update account for bridged token(s)
        //          Follow 2.i.c through 2.i.f
        //  3. If in cache
        //      i. Check if bridge event would affect pending tasks
        //      ii. If it would
        //          a. Avoid race condition, await pending tasks to complete
        //      iii. If it would not
        //          b. Update the relevant token(s) in parallel
        //
        todo!()
    }

    async fn handle_settlement_event() {
        todo!()
    }

    async fn handle_call() {
        todo!()
    }

    async fn handle_send() {
        todo!()
    }

    async fn handle_deploy() {
        todo!()
    }
}

#[async_trait]
impl Actor for Engine {
    type Msg = EngineMessage;
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
        match &message {
            EngineMessage::EoEvent { event } => {
                match event {
                    EoEvent::Bridge(log) => {
                        log::info!("Engine Received EO Bridge Event: {:?}", &log);
                    },
                    EoEvent::Settlement(log) => {
                        log::info!("Engine Received EO Settlement Event: {:?}", &log);
                    },
                }
            },
            _ => {}
        }
        return Ok(())
    }
}
