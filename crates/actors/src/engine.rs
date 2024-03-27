#![allow(unused)]
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
};

use crate::{check_account_cache, check_da_for_account, create_handler, handle_actor_response};
use async_trait::async_trait;
use eigenda_client::payload::EigenDaBlobPayload;
use futures::{
    stream::{iter, StreamExt, Then},
    TryFutureExt,
};
use lasr_contract::create_program_id;
use lasr_messages::{
    AccountCacheMessage, ActorType, BridgeEvent, DaClientMessage, EngineMessage, EoEvent,
    EoMessage, ExecutorMessage, PendingTransactionMessage, SchedulerMessage, ValidatorMessage,
};
use ractor::{
    concurrency::{oneshot, OneshotReceiver, OneshotSender},
    Actor, ActorProcessingErr, ActorRef,
};
use serde_json::Value;
use sha3::{Digest, Keccak256};
use thiserror::Error;

use jsonrpsee::{core::Error as RpcError, tracing::trace_span};
use lasr_types::{
    Account, AccountType, Address, AddressOrNamespace, ArbitraryData, Metadata, Outputs,
    RecoverableSignature, Status, Token, TokenBuilder, Transaction, TransactionBuilder,
    TransactionType,
};
use tokio::sync::mpsc::Sender;

#[derive(Clone, Debug, Default)]
pub struct Engine;

#[derive(Clone, Debug, Error)]
pub enum EngineError {
    Custom(String),
}
impl Default for EngineError {
    fn default() -> Self {
        EngineError::Custom("Engine is unable to acquire actor".to_string())
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
                return account;
            }

            if let Ok(mut account) = self.get_account_from_da(address).await {
                return account;
            }
        } else {
            if let Ok(Some(account)) = self.check_cache(address).await {
                return account;
            }

            if let Ok(mut account) = self.get_account_from_da(address).await {
                return account;
            }
        }

        Account::new(account_type, None, *address, None)
    }

    async fn get_caller_account(&self, address: &Address) -> Result<Account, EngineError> {
        if let Ok(Some(account)) = self.check_cache(address).await {
            return Ok(account);
        }

        if let Ok(mut account) = self.get_account_from_da(address).await {
            return Ok(account);
        }

        Err(EngineError::Custom(
            "caller account does not exist".to_string(),
        ))
    }

    async fn get_program_account(&self, account_type: AccountType) -> Result<Account, EngineError> {
        if let AccountType::Program(program_address) = account_type {
            if let Ok(Some(account)) = self.check_cache(&program_address).await {
                return Ok(account);
            }

            if let Ok(mut account) = self.get_account_from_da(&program_address).await {
                return Ok(account);
            }
        }

        Err(EngineError::Custom(
            "caller account does not exist".to_string(),
        ))
    }

    async fn write_to_cache(&self, account: Account) -> Result<(), EngineError> {
        let message = AccountCacheMessage::Write { account };
        let cache_actor = ractor::registry::where_is(ActorType::AccountCache.to_string()).ok_or(
            EngineError::Custom("unable to find AccountCacheActor in registry".to_string()),
        )?;
        cache_actor.send_message(message);
        Ok(())
    }

    async fn check_cache(&self, address: &Address) -> Result<Option<Account>, EngineError> {
        log::info!(
            "checking account cache for account: {} from engine",
            &address.to_full_string()
        );
        Ok(check_account_cache(*address).await)
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
        account: &Address,
    ) -> Result<
        (
            Address,              /*user*/
            ethereum_types::H256, /* batchHeaderHash*/
            u128,                 /*blobIndex*/
        ),
        EngineError,
    > {
        let (tx, rx) = oneshot();
        let message = EoMessage::GetAccountBlobIndex {
            address: *account,
            sender: tx,
        };
        let actor: ActorRef<EoMessage> =
            ractor::registry::where_is(ActorType::EoServer.to_string())
                .ok_or(EngineError::Custom(
                    "unable to acquire EO Server Actor".to_string(),
                ))?
                .into();

        actor
            .cast(message)
            .map_err(|e| EngineError::Custom(e.to_string()))?;
        let handler = create_handler!(retrieve_blob_index);
        let blob_response = handle_actor_response(rx, handler)
            .await
            .map_err(|e| EngineError::Custom(e.to_string()))?;

        Ok(blob_response)
    }

    async fn get_account_from_da(&self, address: &Address) -> Result<Account, EngineError> {
        check_da_for_account(*address)
            .await
            .ok_or(EngineError::Custom(format!(
                "unable to find account 0x{:x}",
                address
            )))
    }

    async fn set_pending_transaction(
        &self,
        transaction: Transaction,
        outputs: Option<Outputs>,
    ) -> Result<(), EngineError> {
        log::info!(
            "acquiring pending transaction actor to set transaction: {}",
            transaction.hash_string()
        );
        let actor: ActorRef<PendingTransactionMessage> =
            ractor::registry::where_is(ActorType::PendingTransactions.to_string())
                .ok_or(EngineError::Custom(
                    "unable to acquire pending transactions actor".to_string(),
                ))?
                .into();
        let message = PendingTransactionMessage::New {
            transaction,
            outputs,
        };
        actor
            .cast(message)
            .map_err(|e| EngineError::Custom(e.to_string()))?;

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
                .build()
                .map_err(|e| EngineError::Custom(e.to_string()))?;
            self.set_pending_transaction(transaction.clone(), None)
                .await?;
        }

        Ok(())
    }

    async fn handle_call(&self, transaction: Transaction) -> Result<(), EngineError> {
        log::info!("handling call transaction: {}", transaction.hash_string());
        let message = ExecutorMessage::Set { transaction };
        self.inform_executor(message).await?;
        Ok(())
    }

    async fn inform_executor(&self, message: ExecutorMessage) -> Result<(), EngineError> {
        log::info!("acquiring Executor actor");
        let actor: ActorRef<ExecutorMessage> =
            ractor::registry::where_is(ActorType::Executor.to_string())
                .ok_or(EngineError::Custom(
                    "engine.rs 161: Error: unable to acquire executor".to_string(),
                ))?
                .into();
        log::info!(
            "acquired Executor Actor, attempting to send message: {:?}",
            &message
        );
        actor
            .cast(message)
            .map_err(|e| EngineError::Custom(e.to_string()))?;

        Ok(())
    }

    async fn handle_send(&self, transaction: Transaction) -> Result<(), EngineError> {
        log::info!("scheduler handling send: {}", transaction.hash_string());
        self.set_pending_transaction(transaction, None).await?;
        Ok(())
    }

    async fn handle_register_program(&self, transaction: Transaction) -> Result<(), EngineError> {
        log::info!("Creating program address");
        let mut transaction_id = [0u8; 32];
        transaction_id.copy_from_slice(&transaction.hash()[..]);
        let json: serde_json::Map<String, Value> = serde_json::from_str(&transaction.inputs())
            .map_err(|e| EngineError::Custom(e.to_string()))?;

        let content_id = {
            if let Some(id) = json.get("contentId") {
                match id {
                    Value::String(h) => h,
                    _ => {
                        //TODO(asmith): Allow arrays but validate the items in the array
                        return Err(EngineError::Custom(
                            "contentId is incorrect type".to_string(),
                        ));
                    }
                }
            } else {
                return Err(EngineError::Custom("contentId is required".to_string()));
            }
        }
        .to_string();

        #[cfg(feature = "local")]
        let message = ExecutorMessage::Create {
            transaction: transaction.clone(),
            content_id: content_id.clone(),
        };

        let program_id = create_program_id(content_id.clone(), &transaction)
            .map_err(|e| EngineError::Custom(e.to_string()))?;

        #[cfg(feature = "remote")]
        let message = ExecutorMessage::Retrieve {
            content_id,
            program_id,
            transaction,
        };

        log::info!("informing executor");
        self.inform_executor(message).await?;
        Ok(())
    }

    async fn handle_call_success(
        &self,
        transaction: Transaction,
        transaction_hash: String,
        outputs: &str,
    ) -> Result<(), EngineError> {
        // Parse the outputs into instructions
        // Outputs { inputs, instructions };
        log::warn!(
            "transaction: {} received by engine as success",
            transaction_hash.clone()
        );
        let outputs_json =
            serde_json::from_str(outputs).map_err(|e| EngineError::Custom(e.to_string()));

        let parsed_outputs: Outputs = match outputs_json {
            Ok(outputs) => {
                log::info!("Output parsed: {:?}", &outputs);
                outputs
            }
            Err(e) => {
                let error_string = e.to_string();
                log::error!(
                    "Error: engine.rs: 336: Deserialization of outputs failed: {}",
                    e
                );
                self.respond_with_error(
                    transaction.hash_string(),
                    outputs.to_owned(),
                    error_string,
                );
                return Err(e);
            }
        };

        let pending_transactions: ActorRef<PendingTransactionMessage> = {
            ractor::registry::where_is(ActorType::PendingTransactions.to_string())
                .ok_or(EngineError::Custom(
                    "unable to acquire PendingTransactions Actor: engine.rs: 291".to_string(),
                ))?
                .into()
        };

        // Get transaction from pending transactions
        log::warn!("Forwarding to pending transactions");
        let message = PendingTransactionMessage::New {
            transaction,
            outputs: Some(parsed_outputs),
        };
        pending_transactions
            .cast(message)
            .map_err(|e| EngineError::Custom(e.to_string()))?;

        log::warn!("Forwarded to pending transactions");
        Ok(())
    }

    fn respond_with_error(
        &self,
        transaction_hash: String,
        outputs: String,
        error: String,
    ) -> Result<(), EngineError> {
        let scheduler: ActorRef<SchedulerMessage> =
            ractor::registry::where_is(ActorType::Scheduler.to_string())
                .ok_or(EngineError::Custom(
                    "Error: engine.rs: 367: unable to acquire scheuler".to_string(),
                ))?
                .into();

        let message = SchedulerMessage::CallTransactionFailure {
            transaction_hash,
            outputs,
            error,
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
            EngineMessage::EoEvent { event } => match event {
                EoEvent::Bridge(log) => {
                    log::info!("Engine Received EO Bridge Event");
                    self.handle_bridge_event(&log).await.unwrap_or_else(|e| {
                        log::error!("{}", e);
                    });
                }
                EoEvent::Settlement(log) => {
                    log::info!("Engine Received EO Settlement Event");
                }
            },
            EngineMessage::Cache { account, .. } => {
                self.write_to_cache(account);
            }
            EngineMessage::Call { transaction } => {
                self.handle_call(transaction).await;
            }
            EngineMessage::Send { transaction } => {
                self.handle_send(transaction).await;
            }
            EngineMessage::RegisterProgram { transaction } => {
                log::info!(
                    "Received register program transaction: {}",
                    transaction.hash_string()
                );
                self.handle_register_program(transaction).await;
            }
            EngineMessage::CallSuccess {
                transaction,
                transaction_hash,
                outputs,
            } => {
                match self
                    .handle_call_success(transaction, transaction_hash.clone(), &outputs)
                    .await
                {
                    Err(e) => {
                        //TODO Handle error cases
                        let _ = self.respond_with_error(
                            transaction_hash,
                            outputs.clone(),
                            e.to_string(),
                        );
                    }
                    Ok(()) => log::info!(
                        "Successfully parsed outputs from {:?} and sent to pending transactions",
                        transaction_hash
                    ),
                }
            }
            EngineMessage::RegistrationSuccess { transaction_hash } => {
                self.handle_registration_success(transaction_hash);
            }
            _ => {}
        }
        return Ok(());
    }
}
