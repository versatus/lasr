use std::fmt::Display;

use async_trait::async_trait;
use ractor::{ActorRef, Actor, ActorProcessingErr};
use thiserror::Error;
use super::messages::ValidatorMessage;

#[derive(Clone, Debug)]
pub struct Validator; 

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

impl Validator {
    pub fn new() -> Self {
        Self 
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
