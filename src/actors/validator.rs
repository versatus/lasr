use std::fmt::Display;

use async_trait::async_trait;
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::OneshotSender};
use thiserror::Error;
use crate::{RegistryMember, Account, Token, Address};
use tokio::sync::mpsc::Sender;
use super::{messages::{RegistryMessage, ValidatorMessage, RegistryActor}, types::ActorType};

#[derive(Clone, Debug)]
pub struct Validator {
    registry: ActorRef<RegistryMessage>,
    account_cache_writer: Sender<Account>,
    account_cache_checker: Sender<Address>,
    pending_transaction_writer: Sender<(Address, Token, OneshotSender<(Address, Address)>)>
}

#[derive(Clone, Debug, Error)]
pub enum ValidatorError {
    Custom(String)
}

impl Display for ValidatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Default for ValidatorError {
    fn default() -> Self {
        ValidatorError::Custom(
            "Validator unable to acquire actor".to_string()
        )
    }
}

impl RegistryMember for Validator {
    type Err = ValidatorError;
}
    

impl Validator {
    pub fn new(
        registry: ActorRef<RegistryMessage>,
        account_cache_writer: Sender<Account>,
        account_cache_checker: Sender<Address>,
        pending_transaction_writer: Sender<(Address, Token, tokio::sync::oneshot::Sender<(Address, Address)>)>,
    ) -> Self {
        Self { 
            registry,
            account_cache_writer,
            account_cache_checker,
            pending_transaction_writer,
        }
    }

    pub fn register_self(&self, myself: ActorRef<ValidatorMessage>) -> Result<(), Box<dyn std::error::Error>> {
        self.registry.cast(
            RegistryMessage::Register(
                ActorType::Validator, 
                RegistryActor::Validator(myself)
            )
        ).map_err(|e| Box::new(e))?;
        Ok(())
    }
}


#[async_trait]
impl Actor for Validator {
    type Msg = ValidatorMessage;
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
        _message: Self::Msg,
        _: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        println!("Validator received message");
        return Ok(())
    }
}
