#![allow(unused)]
use std::{fmt::Display, collections::{BTreeMap, HashMap}};

use async_trait::async_trait;
use eigenda_client::payload::EigenDaBlobPayload;
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::{oneshot, OneshotSender, OneshotReceiver}};
use serde_json::Value;
use thiserror::Error;
use sha3::{Digest, Keccak256};
use futures::{stream::{iter, Then, StreamExt}, TryFutureExt};
use crate::{Account, BridgeEvent, Metadata, Status, Address, create_handler, EoMessage, handle_actor_response, DaClientMessage, AccountCacheMessage, Token, TokenBuilder, ArbitraryData, TransactionBuilder, TransactionType, Transaction, PendingTransactionMessage, RecoverableSignature, check_da_for_account, check_account_cache, ExecutorMessage, Outputs, AddressOrNamespace, ValidatorMessage, AccountType, SchedulerMessage};
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

    async fn get_account(&self, address: &Address, account_type: AccountType) -> Account {
        if let AccountType::Program(program_address) = account_type {
            if let Ok(Some(account)) = self.check_cache(address).await { 
                return account
            } 

            if let Ok(mut account) = self.get_account_from_da(&address).await {
                return account
            } 
        } else {
            if let Ok(Some(account)) = self.check_cache(address).await { 
                return account
            } 

            if let Ok(mut account) = self.get_account_from_da(&address).await {
                return account
            } 
        }
        
        let account = Account::new(account_type, None, address.clone(), None);
        return account
    }

    async fn get_caller_account(&self, address: &Address) -> Result<Account, EngineError> {
        if let Ok(Some(account)) = self.check_cache(address).await { 
            return Ok(account)
        } 

        if let Ok(mut account) = self.get_account_from_da(&address).await {
            return Ok(account)
        } 
        
        return Err(EngineError::Custom("caller account does not exist".to_string()))
    }

    async fn get_program_account(&self, account_type: AccountType) -> Result<Account, EngineError> {
        if let AccountType::Program(program_address) = account_type {
            if let Ok(Some(account)) = self.check_cache(&program_address).await { 
                return Ok(account)
            } 

            if let Ok(mut account) = self.get_account_from_da(&program_address).await {
                return Ok(account)
            } 
        } 

        return Err(EngineError::Custom("caller account does not exist".to_string()))
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
        log::info!("checking account cache for account: {} from engine", &address.to_full_string());
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
    ) -> Result<(Address /*user*/, ethereum_types::H256/* batchHeaderHash*/, u128 /*blobIndex*/), EngineError> {
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

    async fn set_pending_transaction(
        &self,
        transaction: Transaction,
        outputs: Option<Outputs>
    ) -> Result<(), EngineError> {
        let actor: ActorRef<PendingTransactionMessage> = ractor::registry::where_is(
            ActorType::PendingTransactions.to_string()
        ).ok_or(
            EngineError::Custom("unable to acquire pending transactions actor".to_string())
        )?.into();
        let message = PendingTransactionMessage::New { 
            transaction, 
            outputs 
        };
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
            self.set_pending_transaction(transaction.clone(), None).await?;
        }

        Ok(())
    }

    async fn handle_call(&self, transaction: Transaction) -> Result<(), EngineError> {
        let message = ExecutorMessage::Exec {
            transaction
        };
        self.inform_executor(message).await?;
        Ok(())
    }

    async fn inform_executor(&self, message: ExecutorMessage) -> Result<(), EngineError> {
        let actor: ActorRef<ExecutorMessage> = ractor::registry::where_is(
            ActorType::Executor.to_string()
        ).ok_or(
            EngineError::Custom("engine.rs 161: Error: unable to acquire executor".to_string())
        )?.into();
        actor.cast(message).map_err(|e| EngineError::Custom(e.to_string()))?;

        Ok(())
    }

    async fn handle_send(&self, transaction: Transaction) -> Result<(), EngineError> {
        self.set_pending_transaction(transaction, None).await?;
        Ok(())
    }

    async fn handle_register_program(&self, transaction: Transaction) -> Result<(), EngineError> {
        let mut transaction_id = [0u8; 32];
        transaction_id.copy_from_slice(&transaction.hash()[..]);
        let json: serde_json::Map<String, Value> = serde_json::from_str(&transaction.inputs()).map_err(|e| {
            EngineError::Custom(e.to_string())
        })?;

        #[cfg(feature = "local")]
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

        #[cfg(feature = "local")]
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

        let content_id = {
            match json.get("contentId").ok_or(EngineError::Custom("content id is required".to_string()))? { 
                Value::String(cid) => cid.clone(),
                _ => {
                    return Err(EngineError::Custom("contentId is incorrect type: Must be String".to_string()))
                }
            }
        };
        

        let program_id = {
            let pubkey = transaction.sig().map_err(|e| {
                EngineError::Custom(
                    "signature unable to be reconstructed".to_string()
                )
            })?.recover(&transaction.hash()).map_err(|e| {
                EngineError::Custom(
                    "pubkey unable to be recovered from signature".to_string()
                )
            })?;

            let mut hasher = Keccak256::new();
            hasher.update(content_id.clone());
            hasher.update(pubkey.to_string());
            let hash = hasher.finalize();
            let mut addr_bytes = [0u8; 20];
            addr_bytes.copy_from_slice(&hash[..20]);
            Address::from(addr_bytes)
        };

        #[cfg(feature = "local")]
        let entrypoint = format!("{}", program_id.to_full_string());
        #[cfg(feature = "local")]
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

        #[cfg(feature = "local")]
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

        #[cfg(feature = "local")]
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

        #[cfg(feature = "local")]
        let message = ExecutorMessage::Create {
            transaction,
            program_id, 
            entrypoint, 
            program_args,
            constructor_op,
            constructor_inputs,
        };

        #[cfg(feature = "remote")]
        let message = ExecutorMessage::Retrieve { content_id, program_id, transaction };
        self.inform_executor(message).await?;
        Ok(())
    }

    async fn handle_call_success(&self, transaction: Transaction, transaction_hash: String, outputs: &String) -> Result<(), EngineError> {
        // Parse the outputs into instructions
        // Outputs { inputs, instructions };
        let outputs = serde_json::from_str(outputs).map_err(|e| {
            EngineError::Custom(e.to_string())
        });
        
        let outputs: Outputs = match outputs {
            Ok(outputs) => {
                log::info!("Output parsed: {:?}", &outputs);
                outputs
            },
            Err(e) => {
                log::error!("Error: engine.rs: 336: Deserialization of outputs failed: {}", e);
                return Err(e);
            }
        };

        // Get transaction from pending transactions
        let pending_transactions: ActorRef<PendingTransactionMessage> = {
            ractor::registry::where_is(ActorType::PendingTransactions.to_string()).ok_or(
                EngineError::Custom("unable to acquire PendingTransactions Actor: engine.rs: 291".to_string())
            )?.into()
        };

        let message = PendingTransactionMessage::New { transaction, outputs: Some(outputs) };
        pending_transactions.cast(message).map_err(|e| {
            EngineError::Custom(e.to_string())
        })?;

        Ok(())
    }

    fn respond_with_error(&self, transaction_hash: String, outputs: String, error: String) -> Result<(), EngineError> {
        let scheduler: ActorRef<SchedulerMessage> = ractor::registry::where_is(ActorType::Scheduler.to_string()).ok_or(
            EngineError::Custom("Error: engine.rs: 367: unable to acquire scheuler".to_string())
        )?.into();

        let message = SchedulerMessage::CallTransactionFailure {
            transaction_hash, outputs, error
        };

        scheduler.cast(message);

        Ok(())
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
            EngineMessage::CallSuccess { transaction, transaction_hash, outputs } => {
                match self.handle_call_success(transaction, transaction_hash.clone(), &outputs).await {
                    Err(e) => {
                        //TODO Handle error cases
                        let _ = self.respond_with_error(transaction_hash, outputs.clone(), e.to_string());
                    }
                    Ok(()) => log::info!("Successfully parsed outputs from {:?} and sent to pending transactions", transaction_hash),
                }
            },
            EngineMessage::RegistrationSuccess { transaction_hash } => {
                self.handle_registration_success(transaction_hash);
            },
            _ => {}
        }
        return Ok(())
    }
}
