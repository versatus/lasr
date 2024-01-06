use std::fmt::Display;

use async_trait::async_trait;
use ractor::{ActorRef, Actor, ActorProcessingErr};
use thiserror::Error;
use crate::{
    Account, 
    Transaction, 
    TransactionType, 
    check_account_cache, 
    check_da_for_account, 
    ActorType, 
    PendingTransactionMessage
};

use super::messages::ValidatorMessage;

#[derive(Debug)]
pub struct ValidatorCore {
    pool: rayon::ThreadPool
}

impl Default for ValidatorCore {
    fn default() -> Self {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(50)
            .build()
            .unwrap();

        Self { pool } 
    }
}

impl ValidatorCore {
    fn validate_bridge_in(&self) -> impl FnOnce(Transaction) -> Result<(), Box<dyn std::error::Error + Send>> {
        |tx| {
            if tx.from() != tx.to() {
                return Err(
                    Box::new(
                        ValidatorError::Custom(
                            "bridge tx from and to must be the same".to_string()
                        )
                    ) as Box<dyn std::error::Error + Send>
                )
            }
            let pending_tx: ActorRef<PendingTransactionMessage> = ractor::registry::where_is(
                ActorType::PendingTransactions.to_string()
            ).ok_or(
                Box::new(
                    ValidatorError::Custom(
                        "unable to acquire pending transaction actor".to_string()
                    )
                ) as Box<dyn std::error::Error + Send>
            )?.into();
            let message = PendingTransactionMessage::Valid { transaction: tx, cert: None };
            pending_tx.cast(message).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            Ok(())
        }
    }

    fn validate_send(&self) -> impl FnOnce(Transaction, Account) -> Result<(), Box<dyn std::error::Error + Send>> {
        |tx, account| {
            account.validate_program_id(&tx.program_id())?;
            account.validate_balance(&tx.program_id(), tx.value())?;
            tx.verify_signature().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

            let actor: ActorRef<PendingTransactionMessage> = ractor::registry::where_is(
                ActorType::PendingTransactions.to_string()
            ).ok_or(
                Box::new(
                    ValidatorError::Custom(
                        "unable to acquire pending transaction actor".to_string()
                    )
                ) as Box<dyn std::error::Error + Send>
            )?.into();
            log::info!("transaction {} is valid, responding", tx.hash_string());
            let message = PendingTransactionMessage::Valid { transaction: tx, cert: None };
            actor.cast(message).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            Ok(())
        }
    }

    #[allow(unused)]
    async fn validate_call(&self) -> impl FnOnce(Transaction) -> Result<bool, Box<dyn std::error::Error>> {
        |_tx| Ok(false)
    }
    
    #[allow(unused)]
    async fn validate_bridge_out(&self) -> impl FnOnce(Transaction) -> Result<bool, Box<dyn std::error::Error>> {
        |_tx| Ok(false)
    }

    #[allow(unused)]
    async fn validate_deploy(&self) -> impl FnOnce(Transaction) -> Result<bool, Box<dyn std::error::Error>> {
        |_tx| Ok(false)
    }
}

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
    type State = ValidatorCore; 
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ValidatorCore::default())
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ValidatorMessage::PendingTransaction { transaction } => {
                log::info!("Received transaction to validate: {}", transaction.hash_string());
                // spin up thread 
                let transaction_type = transaction.transaction_type();
                match transaction_type {
                    TransactionType::Send(_) => {
                        let account = if let Some(account) = check_account_cache(transaction.from()).await {
                            Some(account)
                        } else if let Some(account) = check_da_for_account(transaction.from()).await {
                            Some(account)
                        } else {
                            None
                        };

                        if account.is_none() {
                            let actor: ActorRef<PendingTransactionMessage> = ractor::registry::where_is(
                                ActorType::PendingTransactions.to_string()
                            ).ok_or(
                                Box::new(ValidatorError::Custom("unable to acquire pending transaction actor".to_string()))
                            )?.into();

                            let message = PendingTransactionMessage::Invalid { transaction };
                            actor.cast(message)?;
                        } else {
                            let op = state.validate_send();
                            state.pool.spawn_fifo(move || {
                                let _ = op(transaction.clone(), account.unwrap());
                            });
                        }
                    }
                    TransactionType::Call(_) => {

                        // get account
                        // build op
                        // install op
                    },
                    TransactionType::BridgeIn(_) => {
                        let _account = if let Some(account) = check_account_cache(transaction.from()).await {
                            Some(account)
                        } else if let Some(account) = check_da_for_account(transaction.from()).await {
                            Some(account)
                        } else {
                            None
                        };
                        let op = state.validate_bridge_in();
                        state.pool.spawn_fifo(move || {
                            let _ = op(transaction);
                        });
                        // get account
                        // check bridged balance in EO
                        // for address
                        // naively validate
                    },
                    TransactionType::Deploy(_) => {
                        // get program
                        // build program
                        // validate sender sig
                        // validate sender balance for deployment fees
                        // commit contract blob
                        //
                    },
                    TransactionType::BridgeOut(_) => {
                        // get program
                        // check for corresponding program
                        // validate sender sig
                        // validate sender balance
                        // execute bridge fn in program
                        // settle bridge transaction on settlement networ
                    }
                }
            }
        }
        return Ok(())
    }
}
