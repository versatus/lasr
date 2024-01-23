#![allow(unused)]
use std::{fmt::Display, collections::{BTreeMap, HashMap}};

use async_trait::async_trait;
use eigenda_client::payload::EigenDaBlobPayload;
use ethereum_types::{U256, H256};
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::{oneshot, OneshotSender, OneshotReceiver}};
use serde_json::Value;
use thiserror::Error;
use futures::{stream::{iter, Then, StreamExt}, TryFutureExt};
use crate::{Account, BridgeEvent, Metadata, Status, Address, create_handler, EoMessage, handle_actor_response, DaClientMessage, AccountCacheMessage, Token, TokenBuilder, ArbitraryData, TransactionBuilder, TransactionType, Transaction, PendingTransactionMessage, RecoverableSignature, check_da_for_account, check_account_cache, ExecutorMessage, Outputs};
use jsonrpsee::{core::Error as RpcError, tracing::trace_span};
use tokio::sync::mpsc::Sender;

use super::{messages::{EngineMessage, EoEvent}, types::ActorType};

#[derive(Clone, Debug)]
pub struct Engine;

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
    pub fn new() -> Self {
        Self 
    }

    async fn get_account(&self, address: &Address) -> Account {
        if let Ok(Some(account)) = self.check_cache(address).await { 
            return account
        } 

        if let Ok(mut account) = self.get_account_from_da(&address).await {
            return account
        } 
        
        let account = Account::new(address.clone(), None);
        return account
    }

    async fn write_to_cache(&self, account: Account) -> Result<(), EngineError> {
        let message = AccountCacheMessage::Write { account };
        let cache_actor = ractor::registry::where_is(ActorType::AccountCache.to_string()).ok_or(
            EngineError::Custom("unable to find AccountCacheActor in registry".to_string())
        )?;
        cache_actor.send_message(message);
        Ok(())
    }

    async fn check_cache(&self, address: &Address) -> Result<Option<Account>, EngineError> {
        Ok(check_account_cache(address.clone()).await)
    }

    async fn handle_cache_response(
        &self, 
        rx: OneshotReceiver<Option<Account>>,
    ) -> Result<Option<Account>, EngineError> {
        tokio::select! {
            reply = rx => {
                match reply {
                    Ok(Some(account)) => {
                        return Ok(Some(account))
                    },
                    Ok(None) => {
                        return Ok(None)
                    }
                    Err(e) => {
                        log::error!("{}", e);
                    },
                }
            }
        }
        Ok(None)
    }

    async fn request_blob_index(
        &self,
        account: &Address
    ) -> Result<(Address /*user*/, H256/* batchHeaderHash*/, u128 /*blobIndex*/), EngineError> {
        let (tx, rx) = oneshot();
        let message = EoMessage::GetAccountBlobIndex { address: account.clone(), sender: tx };
        let actor: ActorRef<EoMessage> = ractor::registry::where_is(
            ActorType::EoServer.to_string()
        ).ok_or(
            EngineError::Custom("unable to acquire EO Server Actor".to_string())
        )?.into();

        actor.cast(message).map_err(|e| EngineError::Custom(e.to_string()))?;
        let handler = create_handler!(retrieve_blob_index);
        let blob_response = handle_actor_response(rx, handler).await.map_err(|e| {
            EngineError::Custom(e.to_string())
        })?;

        Ok(blob_response)
    } 

    async fn get_account_from_da(
        &self,
        address: &Address,
    ) -> Result<Account, EngineError> {
        check_da_for_account(address.clone()).await.ok_or(
            EngineError::Custom(
                format!("unable to find account 0x{:x}", address)
            )
        )
    }

    async fn set_pending_transaction(&self, transaction: Transaction) -> Result<(), EngineError> {
        let actor: ActorRef<PendingTransactionMessage> = ractor::registry::where_is(
            ActorType::PendingTransactions.to_string()
        ).ok_or(
            EngineError::Custom("unable to acquire pending transactions actor".to_string())
        )?.into();
        let message = PendingTransactionMessage::New { transaction: transaction.clone() };
        actor.cast(message).map_err(|e| EngineError::Custom(e.to_string()))?;

        Ok(())
    }

    async fn handle_bridge_event(&self, logs: &Vec<BridgeEvent>) -> Result<(), EngineError> {
        for event in logs {
            // Turn event into Transaction
            let transaction = TransactionBuilder::default()
                .program_id(event.program_id().into())
                .from(event.user().into())
                .to(event.user().into())
                .transaction_type(TransactionType::BridgeIn(event.bridge_event_id()))
                .value(event.amount())
                .inputs(String::new())
                .op(String::new())
                .nonce(event.bridge_event_id())
                .v(0)
                .r([0; 32])
                .s([0; 32])
                .build().map_err(|e| EngineError::Custom(e.to_string()))?;
            self.set_pending_transaction(transaction.clone()).await?;
        }

        Ok(())
    }

    async fn handle_call(&self, transaction: Transaction) -> Result<(), EngineError> {
        let message = ExecutorMessage::Exec {
            program_id: transaction.to(), 
            op: transaction.op(), 
            inputs: transaction.inputs(),
            transaction_hash: transaction.hash_string(), 
        };
        self.inform_executor(transaction, message).await?;
        Ok(())
    }

    async fn inform_executor(&self, transaction: Transaction, message: ExecutorMessage) -> Result<(), EngineError> {
        let actor: ActorRef<ExecutorMessage> = ractor::registry::where_is(
            ActorType::Executor.to_string()
        ).ok_or(
            EngineError::Custom("engine.rs 161: Error: unable to acquire executor".to_string())
        )?.into();
        actor.cast(message).map_err(|e| EngineError::Custom(e.to_string()))?;

        Ok(())
    }

    async fn handle_send(&self, transaction: Transaction) -> Result<(), EngineError> {
        self.set_pending_transaction(transaction).await?;
        Ok(())
    }

    async fn handle_register_program(&self, transaction: Transaction) -> Result<(), EngineError> {
        let mut transaction_id = [0u8; 32];
        transaction_id.copy_from_slice(&transaction.hash()[..]);
        let json: serde_json::Map<String, Value> = serde_json::from_str(&transaction.inputs()).map_err(|e| {
            EngineError::Custom(e.to_string())
        })?;

        let content_id = {
            if let Some(id) = json.get("contentId") {
                match id {
                    Value::String(h) => {
                        if h.starts_with("0x") {
                            hex::decode(&h[2..]).map_err(|e| EngineError::Custom(e.to_string()))?
                        } else {
                            hex::decode(&h).map_err(|e| EngineError::Custom(e.to_string()))?
                        }
                    },
                    _ => {
                        //TODO(asmith): Allow arrays but validate the items in the array
                        return Err(EngineError::Custom("contentId is incorrect type".to_string()))
                    }
                }
            } else {
                return Err(EngineError::Custom("contentId is required".to_string()));
            }
        }; 

        let program_id = if &content_id.len() == &32 {
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&content_id[..]);
            Address::from(buf)
        } else if &content_id.len() == &20 {
            let mut buf = [0u8; 20];
            buf.copy_from_slice(&content_id[..]);
            Address::from(buf)
        } else {
            return Err(EngineError::Custom("invalid length for content id".to_string()))
        };

        let entrypoint = format!("{}", program_id.to_full_string());
        let program_args = if let Some(v) = json.get("programArgs") {
            match v {
                Value::Array(arr) => {
                    let args: Vec<String> = arr.iter().filter_map(|val| {
                        match val {
                           Value::String(arg) => {
                               Some(arg.clone())
                           },
                           _ => None
                        }
                    }).collect();

                    if args.len() != arr.len() {
                        None
                    } else {
                        Some(args)
                    }
                },
                _ => None
            }
        } else {
            None
        };

        let constructor_op = if let Some(op) = json.get("constructorOp") {
            match op {
                Value::String(o) => {
                    Some(o.clone())
                },
                _ => None
            }

        } else {
            None
        };

        let constructor_inputs = if let Some(inputs) = json.get("constructorInputs") {
            match inputs {
                Value::String(i) => {
                    Some(i.clone())
                },
                _ => None
            }
        } else {
            None
        };

        let message = ExecutorMessage::Create {
            transaction_hash: transaction.hash_string(),
            program_id, 
            entrypoint, 
            program_args,
            constructor_op,
            constructor_inputs,
        };
        self.inform_executor(transaction, message).await?;
        Ok(())
    }

    fn handle_call_success(&self, transaction_hash: String, outputs: &String) -> Result<(), EngineError> {
        // Parse the outputs into instructions
        let outputs: Outputs = serde_json::from_str(outputs).map_err(|e| {
            EngineError::Custom(e.to_string())
        })?;

        // Get transaction from pending transactions
        // Send transaction and Outputs to validator
        // Validate transaction structure and caller signature, including nonce
        // balance as it relates to value, and then validate instructions
        // instruction validation includes checking the balance of and Transfer or 
        // Burn instructions to the accounts that will have their balance reduced
        // For creates and updates it includes validating the caller is the owner
        // or has approval, etc.

        todo!()
    }

    fn handle_registration_success(&self, transaction_hash: String) -> Result<(), EngineError> {
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
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            EngineMessage::EoEvent { event } => {
                match event {
                    EoEvent::Bridge(log) => {
                        log::info!("Engine Received EO Bridge Event");
                        self.handle_bridge_event(&log).await.unwrap_or_else(|e| {
                            log::error!("{}", e);
                        });
                    },
                    EoEvent::Settlement(log) => {
                        log::info!("Engine Received EO Settlement Event");
                    }
                }
            },
            EngineMessage::Cache { account, .. } => {
                //TODO(asmith): Use a proper LRU Cache
                self.write_to_cache(account);
            },
            EngineMessage::Call { transaction } => {
                self.handle_call(transaction).await;
            },
            EngineMessage::Send { transaction } => {
                self.handle_send(transaction).await;
            },
            EngineMessage::RegisterProgram { transaction } => {
                self.handle_register_program(transaction).await;
            },
            EngineMessage::CallSuccess { transaction_hash, outputs } => {
                self.handle_call_success(transaction_hash, &outputs);
            },
            EngineMessage::RegistrationSuccess { transaction_hash } => {
                self.handle_registration_success(transaction_hash);
            },
            _ => {}
        }
        return Ok(())
    }
}
