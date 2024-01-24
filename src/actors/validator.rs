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
    PendingTransactionMessage, AddressOrNamespace, Outputs, Instruction
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
            account.validate_nonce(tx.nonce())?;
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
    fn validate_call(&self) -> impl FnOnce(Vec<AddressOrNamespace>, Vec<Option<Account>>, Outputs, Transaction) -> Result<bool, Box<dyn std::error::Error + Send>> {
        |accounts_involved, accounts, outputs, tx| {
            tx.verify_signature().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            let instructions = outputs.instructions();
            for instruction in instructions {
                match instruction {
                    Instruction::Transfer(transfer) => {
                    }
                    Instruction::Burn(burn) => {
                    }
                    Instruction::Update(update) => {
                    }
                    Instruction::Create(create) => {
                    }
                    Instruction::Log(log) => {
                    }
                }
            }
            // for any account that is having funds transfered out, based on an instruction
            // check that it's either the caller account and the signature is valid,
            // that the caller has and approval/allowance to the account for this particular token
            // and that the amount is less than or equal to the allowance (if an allowance)
            // or that the program that was called has approval/allowance on the account
            //
            // Also check that the account actually exists, i.e. is Some(account) in the 
            // accounts variable from the closure
            //
            // Also check that the balance for the token being transfered out is valid
            //
            // For any updates, check that the account is owned by the caller, or that the 
            // caller has an approval over the account/token, or if not the caller, and the 
            // caller does not have approval over the account/token, that the program that , 
            // was called has approval over the account/token
            //
            // For any create instructions, check that the caller is the program owner
            // or is approved,
            Ok(false)
        }
        // Validate transaction structure and caller signature, including nonce
        // balance as it relates to value, and then validate instructions
        // instruction validation includes checking the balance of and Transfer or 
        // Burn instructions to the accounts that will have their balance reduced
        // For creates and updates it includes validating the caller is the owner
        // or has approval, or the program has approval etc.
        // After transaction is validated, send the transaction and instructions 
        // to the batcher to apply to the accounts in question
    }
    
    #[allow(unused)]
    async fn validate_bridge_out(&self) -> impl FnOnce(Transaction) -> Result<bool, Box<dyn std::error::Error>> {
        |_tx| Ok(false)
    }

    fn validate_register_program(&self) -> impl FnOnce(Transaction) -> Result<bool, Box<dyn std::error::Error>> {
        |_tx| Ok(false)
        // Validate signature
        // Validate schema
        // validate account nonce in transaction
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
                        log::info!("Received send transaction, checking account_cache");
                        let account = if let Some(account) = check_account_cache(transaction.from()).await {
                            log::info!("found account in cache");
                            Some(account)
                        } else if let Some(account) = check_da_for_account(transaction.from()).await {
                            log::info!("found account in da");
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
                            log::info!("validating send transaction");
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
                    TransactionType::RegisterProgram(_) => {
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
            ValidatorMessage::PendingCall { accounts_involved, outputs, transaction } => {
                // Acquire all relevant accounts.
                let mut validator_accounts: Vec<Option<Account>> = Vec::new();
                for address in &accounts_involved {
                    //TODO(asmith): Replace this block with a parallel iterator to optimize
                    match address {
                        AddressOrNamespace::Address(addr) => {
                            if let Some(account) = check_account_cache(addr.clone()).await {
                                log::info!("found account in cache");
                                validator_accounts.push(Some(account));
                            } else if let Some(account) = check_da_for_account(addr.clone()).await {
                                log::info!("found account in da");
                                validator_accounts.push(Some(account));
                            } else {
                                validator_accounts.push(None);
                            };
                        },
                        AddressOrNamespace::Namespace(_namespace) => {
                            //if let Some(account) = check_account_cache(&address).await {
                            //    log::info!("found account in cache");
                            //    validator_accounts.push(Some(account));
                            //} else if let Some(account) = check_da_for_account(&address).await {
                            //    log::info!("found account in da");
                            //    validator_accounts.push(Some(account));
                            //} else {
                            //    validator_accounts.push(None);
                            //};
                            //TODO(asmith): implement check_account_cache and check_da_for_account for
                            //Namespaces
                            validator_accounts.push(None);
                        }
                    }
                }

                let op = state.validate_call();
                state.pool.spawn_fifo(move || {
                    let _ = op(accounts_involved, validator_accounts, outputs, transaction);
                });
                
            },
            ValidatorMessage::PendingRegistration { transaction } => {
                let op = state.validate_register_program();
                state.pool.spawn_fifo(move || {
                    let _ = op(transaction);
                });
            }
        }
        return Ok(())
    }
}
