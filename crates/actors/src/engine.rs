#![allow(unused)]
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    sync::Arc,
};

use crate::{
    check_account_cache, check_da_for_account, create_handler, handle_actor_response, ActorExt,
    StaticFuture, UnorderedFuturePool,
};
use async_trait::async_trait;
use eigenda_client::payload::EigenDaBlobPayload;
use futures::{
    stream::{iter, FuturesUnordered, StreamExt, Then},
    FutureExt, TryFutureExt,
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
use tokio::sync::{mpsc::Sender, Mutex};

#[derive(Clone, Debug, Default)]
pub struct EngineActor {
    future_pool: UnorderedFuturePool<StaticFuture<Result<(), EngineError>>>,
}

#[derive(Clone, Debug, Error)]
pub enum EngineError {
    #[error("{0:?}")]
    Custom(String),
}
impl Default for EngineError {
    fn default() -> Self {
        EngineError::Custom("Engine is unable to acquire actor".to_string())
    }
}

impl EngineActor {
    pub fn new() -> Self {
        Self {
            future_pool: Arc::new(Mutex::new(FuturesUnordered::new())),
        }
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

    fn write_to_cache(account: Account) {
        let owner = account.owner_address();
        let message = AccountCacheMessage::Write { account };
        if let Some(cache_actor) = ractor::registry::where_is(ActorType::AccountCache.to_string()) {
            if let Err(e) = cache_actor.send_message(message) {
                log::error!("AccountCacheActor Error: failed to send write message for account address: {owner}: {e:?}");
            }
        } else {
            log::error!("unable to find AccountCacheActor in registry");
        }
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
                        log::error!("{e:?}");
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

    async fn handle_bridge_event(logs: &Vec<BridgeEvent>) -> Result<(), EngineError> {
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
            EngineActor::set_pending_transaction(transaction.clone(), None).await?;
        }

        Ok(())
    }

    async fn handle_call(transaction: Transaction) -> Result<(), EngineError> {
        log::info!("handling call transaction: {}", transaction.hash_string());
        let message = ExecutorMessage::Set { transaction };
        EngineActor::inform_executor(message).await?;
        Ok(())
    }

    async fn inform_executor(message: ExecutorMessage) -> Result<(), EngineError> {
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

    async fn handle_send(transaction: Transaction) -> Result<(), EngineError> {
        log::info!("scheduler handling send: {}", transaction.hash_string());
        EngineActor::set_pending_transaction(transaction, None).await
    }

    async fn handle_register_program(transaction: Transaction) -> Result<(), EngineError> {
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
        EngineActor::inform_executor(message).await?;
        Ok(())
    }

    async fn call_success(
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
                log::error!("Engine Error: Deserialization of outputs failed: {}", e);
                EngineActor::respond_with_error(
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
                    "unable to acquire PendingTransactions Actor".to_string(),
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

    async fn handle_call_success(
        transaction: Transaction,
        transaction_hash: String,
        outputs: String,
    ) -> Result<(), EngineError> {
        match EngineActor::call_success(transaction, transaction_hash.clone(), &outputs).await {
            Err(e) => {
                //TODO Handle error cases
                let _ = EngineActor::respond_with_error(transaction_hash, outputs, e.to_string());
            }
            Ok(()) => log::info!(
                "Successfully parsed outputs from {:?} and sent to pending transactions",
                transaction_hash
            ),
        }
        Ok(())
    }

    fn respond_with_error(
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

    fn handle_registration_success(transaction_hash: String) -> Result<(), EngineError> {
        todo!()
    }

    async fn handle_eo_event(event: EoEvent) -> Result<(), EngineError> {
        match event {
            EoEvent::Bridge(log) => {
                log::info!("Engine Received EO Bridge Event");
                EngineActor::handle_bridge_event(&log)
                    .await
                    .unwrap_or_else(|e| {
                        log::error!("{e:?}");
                    });
            }
            EoEvent::Settlement(log) => {
                log::info!("Engine Received EO Settlement Event");
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Actor for EngineActor {
    type Msg = EngineMessage;
    type State = ();
    type Arguments = Self::State;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::State,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(args)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            EngineMessage::EoEvent { event } => {
                let fut = EngineActor::handle_eo_event(event);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            EngineMessage::Cache { account, .. } => EngineActor::write_to_cache(account),
            EngineMessage::Call { transaction } => {
                let fut = EngineActor::handle_call(transaction);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            EngineMessage::Send { transaction } => {
                let fut = EngineActor::handle_send(transaction);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            EngineMessage::RegisterProgram { transaction } => {
                log::info!(
                    "Received register program transaction: {}",
                    transaction.hash_string()
                );
                let fut = EngineActor::handle_register_program(transaction);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            EngineMessage::CallSuccess {
                transaction,
                transaction_hash,
                outputs,
            } => {
                let fut = EngineActor::handle_call_success(transaction, transaction_hash, outputs);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            EngineMessage::RegistrationSuccess { transaction_hash } => {
                EngineActor::handle_registration_success(transaction_hash);
            }
            _ => {}
        }
        Ok(())
    }
}

impl ActorExt for EngineActor {
    type Output = Result<(), EngineError>;
    type Future<O> = StaticFuture<Self::Output>;
    type FuturePool<F> = UnorderedFuturePool<Self::Future<Self::Output>>;
    type FutureHandler = tokio_rayon::rayon::ThreadPool;
    type JoinHandle = tokio::task::JoinHandle<()>;

    fn future_pool(&self) -> Self::FuturePool<Self::Future<Self::Output>> {
        self.future_pool.clone()
    }

    fn spawn_future_handler(actor: Self, future_handler: Self::FutureHandler) -> Self::JoinHandle {
        tokio::spawn(async move {
            loop {
                let futures = actor.future_pool();
                let mut guard = futures.lock().await;
                future_handler
                    .install(|| async move {
                        if let Some(Err(err)) = guard.next().await {
                            log::error!("{err:?}");
                        }
                    })
                    .await;
            }
        })
    }
}

#[cfg(test)]
mod engine_tests {
    use crate::{ActorExt, EngineActor};
    use lasr_messages::{ActorType, EngineMessage};
    use lasr_types::Transaction;
    use ractor::Actor;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_engine_future_handler() {
        let engine_actor = EngineActor::new();

        let (engine_actor_ref, _engine_handle) = Actor::spawn(
            Some(ActorType::Engine.to_string()),
            engine_actor.clone(),
            (),
        )
        .await
        .expect("failed to spawn engine actor");

        engine_actor
            .handle(
                engine_actor_ref.clone(),
                EngineMessage::Call {
                    transaction: Transaction::default(),
                },
                &mut (),
            )
            .await
            .unwrap();
        engine_actor
            .handle(
                engine_actor_ref.clone(),
                EngineMessage::Send {
                    transaction: Transaction::default(),
                },
                &mut (),
            )
            .await
            .unwrap();
        engine_actor
            .handle(
                engine_actor_ref,
                EngineMessage::RegisterProgram {
                    transaction: Transaction::default(),
                },
                &mut (),
            )
            .await
            .unwrap();
        // TODO: Add other messages in the handle method
        {
            let guard = engine_actor.future_pool.lock().await;
            assert!(!guard.is_empty());
        }

        let future_thread_pool = tokio_rayon::rayon::ThreadPoolBuilder::new()
            .num_threads(num_cpus::get())
            .build()
            .unwrap();

        let actor_clone = engine_actor.clone();
        EngineActor::spawn_future_handler(actor_clone, future_thread_pool);

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            {
                let guard = engine_actor.future_pool.lock().await;
                if guard.is_empty() {
                    break;
                }
            }
            interval.tick().await;
        }
    }
}
