use crate::{
    get_account, process_group_changed, ActorExt, Coerce, StaticFuture, UnorderedFuturePool,
};
use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};
use futures::FutureExt;
use internal_rpc::api::InternalRpcApiClient;
#[cfg(feature = "remote")]
use internal_rpc::job_queue::job::{ComputeJobExecutionType, ServiceJobState, ServiceJobType};
use jsonrpsee::{core::client::ClientT, ws_client::WsClient};
#[cfg(feature = "local")]
use lasr_compute::OciManager;
use lasr_contract::create_program_id;
use lasr_messages::{ActorName, BatcherMessage, SupervisorType};
use lasr_messages::{
    ActorType, EngineMessage, ExecutorMessage, PendingTransactionMessage, SchedulerMessage,
};
use lasr_types::{Inputs, ProgramSchema, Required, Transaction};
use ractor::{Actor, ActorCell, ActorProcessingErr, ActorRef, SupervisionEvent};
use serde::{Deserialize, Serialize};
#[cfg(feature = "remote")]
use std::time::Duration;
use std::{collections::HashMap, path::Path, sync::Arc};
use thiserror::Error;
#[cfg(feature = "remote")]
use tokio::sync::mpsc::Receiver;
use tokio::{
    sync::{mpsc::Sender, Mutex},
    task::JoinHandle,
};

#[derive(Debug)]
#[allow(unused)]
pub struct PendingJob {
    handle: JoinHandle<std::io::Result<()>>,
    sender: Sender<()>,
    transaction: Transaction,
}

impl PendingJob {
    pub fn new(
        handle: JoinHandle<std::io::Result<()>>,
        sender: Sender<()>,
        transaction: Transaction,
    ) -> Self {
        Self {
            handle,
            sender,
            transaction,
        }
    }

    pub async fn join_handle(self) {
        let _ = self.handle.await;
    }

    pub fn transaction(&self) -> &Transaction {
        &self.transaction
    }

    pub async fn kill_thread(&self) -> std::io::Result<()> {
        self.sender
            .send(())
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        Ok(())
    }
}

// This will be a weighted LRU cache that captures the size of the
// containers and kills/deletes LRU containers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynCache;

#[allow(unused)]
pub struct ExecutionEngine<C: InternalRpcApiClient> {
    #[cfg(feature = "local")]
    manager: OciManager,
    #[cfg(feature = "remote")]
    compute_rpc_client: C,
    #[cfg(feature = "remote")]
    storage_rpc_client: C,
    #[cfg(feature = "remote")]
    pending: HashMap<uuid::Uuid, PendingJob>,
    handles: HashMap<(String, String), tokio::task::JoinHandle<std::io::Result<String>>>,
    cache: DynCache,
    #[cfg(feature = "local")]
    phantom: std::marker::PhantomData<C>,
}

#[cfg(feature = "remote")]
impl<C: InternalRpcApiClient> ExecutionEngine<C> {
    pub fn new(compute_rpc_client: C, storage_rpc_client: C) -> Self {
        Self {
            compute_rpc_client,
            storage_rpc_client,
            pending: HashMap::new(),
            handles: HashMap::new(),
            cache: DynCache,
        }
    }

    pub(super) fn spawn_poll(
        &self,
        job_id: uuid::Uuid,
        mut rx: Receiver<()>,
    ) -> std::io::Result<JoinHandle<std::io::Result<()>>> {
        Ok(tokio::task::spawn(async move {
            let mut attempts = 0;
            let actor: ActorRef<ExecutorMessage> =
                ractor::registry::where_is(ActorType::Executor.to_string())
                    .ok_or(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "unable to locate remote executor",
                    ))?
                    .into();

            while attempts < 20 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if let Ok(()) = rx.try_recv() {
                    break;
                }
                let message = ExecutorMessage::PollJobStatus {
                    job_id: job_id.clone(),
                };
                actor
                    .clone()
                    .cast(message)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                log::info!("attempt number {} polling job status", attempts + 1);
                attempts += 1;
            }

            Ok(())
        }))
    }
}

#[cfg(feature = "local")]
impl<C: ClientT> ExecutionEngine<C> {
    pub fn new(manager: OciManager) -> Self {
        Self {
            manager,
            handles: HashMap::new(),
            cache: DynCache,
            phantom: std::marker::PhantomData,
        }
    }

    pub(super) async fn pin_object(
        &self,
        content_id: &str,
        recursive: bool,
    ) -> std::io::Result<()> {
        self.manager.pin_object(content_id, recursive).await
    }

    pub(super) async fn _create_bundle(&self, content_id: impl AsRef<Path>) -> std::io::Result<()> {
        self.manager.bundle(&content_id).await?;
        Ok(())
    }

    pub(super) async fn execute(
        &self,
        content_id: impl AsRef<Path> + Send + 'static,
        program_id: String,
        transaction: Transaction,
        inputs: Inputs,
        transaction_hash: &str,
    ) -> std::io::Result<tokio::task::JoinHandle<std::io::Result<String>>> {
        let handle = self
            .manager
            .run_container(
                content_id,
                program_id,
                Some(transaction),
                inputs,
                Some(transaction_hash.to_owned()),
            )
            .await?;
        log::warn!("returning handle to executor");
        Ok(handle)
    }

    pub fn get_program_schema(
        &self,
        content_id: impl AsRef<Path> + Send,
    ) -> std::io::Result<ProgramSchema> {
        self.manager.get_program_schema(content_id)
    }

    pub async fn parse_inputs(
        &self,
        /*_schema: &ProgramSchema,*/
        transaction: &Transaction,
        op: String,
        inputs: String,
    ) -> std::io::Result<Inputs> {
        if let Some(program_account) = get_account(transaction.to()).await {
            Ok(Inputs {
                version: 1,
                account_info: program_account.clone(),
                transaction: transaction.clone(),
                op,
                inputs,
            })
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "program account {} does not exist",
                    transaction.to().to_full_string()
                ),
            ))
        }
    }

    fn set_pending_call(&self, transaction: Transaction) -> std::io::Result<()> {
        let pending_transactions: ActorRef<PendingTransactionMessage> =
            ractor::registry::where_is(ActorType::PendingTransactions.to_string())
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "unable to acquire PendingTransactions actor",
                ))?
                .into();

        let message = PendingTransactionMessage::NewCall { transaction };

        let _ = pending_transactions.cast(message);

        Ok(())
    }

    pub fn handle_prerequisites(
        &self,
        pre_requisites: &Vec<Required>,
    ) -> std::io::Result<Vec<String>> {
        for req in pre_requisites {
            match req {
                Required::Call(_call_map) => {
                    // CallMap consists of:
                    //  - calling_program: TransactionFields,
                    //  - original_caller: TransactionFields,
                    //  - program_id: String,
                    //  - inputs: Inputs,
                    //  this should stand up an unsigned execution of the
                    //  program id, the op tell us the operation to call
                    //  and the inputs tell us what to pass in
                }
                Required::Read(read_params) => {
                    dbg!(read_params);
                }
                Required::Lock(lock_pair) => {
                    dbg!(lock_pair);
                }
                Required::Unlock(lock_pair) => {
                    dbg!(lock_pair);
                }
            }
        }
        Ok(Vec::new())
    }
}

#[derive(Clone)]
pub struct ExecutorActor {
    future_pool: UnorderedFuturePool<StaticFuture<()>>,
}
impl ActorName for ExecutorActor {
    fn name(&self) -> ractor::ActorName {
        ActorType::Executor.to_string()
    }
}

impl ExecutorActor {
    pub fn new() -> Self {
        Self {
            future_pool: Arc::new(Mutex::new(FuturesUnordered::new())),
        }
    }
    pub fn registration_error(
        transaction_hash: String,
        error_string: String,
    ) -> std::io::Result<()> {
        let actor: ActorRef<SchedulerMessage> =
            ractor::registry::where_is(ActorType::Scheduler.to_string())
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "unable to acquire Scheduler",
                ))?
                .into();

        let message = SchedulerMessage::RegistrationFailure {
            transaction_hash,
            error_string,
        };
        actor
            .cast(message)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        Ok(())
    }

    #[cfg(feature = "local")]
    fn registration_success(transaction: Transaction) -> std::io::Result<()> {
        let actor: ActorRef<BatcherMessage> =
            ractor::registry::where_is(ActorType::Batcher.to_string())
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "unable to acquire batcher",
                ))?
                .into();

        let message = BatcherMessage::AppendTransaction(transaction);
        actor
            .cast(message)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        Ok(())
    }

    #[cfg(feature = "remote")]
    fn registration_success(transaction: Transaction) -> std::io::Result<()> {
        let message = BatcherMessage::AppendTransaction(transaction);

        let batcher: ActorRef<BatcherMessage> =
            ractor::registry::where_is(ActorType::Batcher.to_string())
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "unable to acquire Bratcher",
                ))?
                .into();

        let _ = batcher.cast(message);

        Ok(())
    }

    fn execution_success(transaction: &Transaction, outputs: &str) -> std::io::Result<()> {
        let pending_transactions_actor: ActorRef<PendingTransactionMessage> =
            ractor::registry::where_is(ActorType::PendingTransactions.to_string())
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "unable to acquire PendingTransactions actor",
                ))?
                .into();

        let message = PendingTransactionMessage::ExecSuccess {
            transaction: transaction.clone(),
        };

        log::warn!("call was sucessful, forwarding to pending transaction");
        pending_transactions_actor
            .cast(message)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        log::warn!("call was successful, forwarding to engine");
        let engine_actor: ActorRef<EngineMessage> =
            ractor::registry::where_is(ActorType::Engine.to_string())
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Unable to acquire Engine actor",
                ))?
                .into();

        let message = EngineMessage::CallSuccess {
            transaction: transaction.clone(),
            transaction_hash: transaction.hash_string(),
            outputs: outputs.to_owned(),
        };

        engine_actor
            .cast(message)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        log::warn!("informed both pending transactions and engine actors of successful call");
        Ok(())
    }

    pub fn execution_error(
        transaction_hash: &String,
        err: impl std::error::Error,
    ) -> std::io::Result<()> {
        let actor: ActorRef<SchedulerMessage> =
            ractor::registry::where_is(ActorType::Scheduler.to_string())
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Unable to acquire Engine actor",
                ))?
                .into();

        let message = SchedulerMessage::CallTransactionFailure {
            transaction_hash: transaction_hash.to_string(),
            outputs: String::new(),
            error: err.to_string(),
        };

        actor
            .cast(message)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        Ok(())
    }

    pub fn call_queued_for_exec(transaction: Transaction) -> std::io::Result<()> {
        let actor: ActorRef<SchedulerMessage> =
            ractor::registry::where_is(ActorType::Scheduler.to_string())
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Unable to acquire Scheduler actor",
                ))?
                .into();

        let message = SchedulerMessage::CallTransactionAsyncPending {
            transaction_hash: transaction.hash_string(),
        };

        actor
            .cast(message)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        Ok(())
    }

    async fn create(
        engine: Arc<Mutex<ExecutionEngine<WsClient>>>,
        transaction: Transaction,
        content_id: String,
    ) {
        log::info!("Received create program bundle request");
        // Build the container spec and create the container image
        log::info!("attempting to pin object from IPFS");
        let manager = {
            let state = engine.lock().await;
            let manager = state.manager.clone();
            if let Err(_) = manager.check_pinned_status(&content_id).await {
                match state.pin_object(&content_id.clone(), true).await {
                    Ok(()) => {
                        log::info!("Successfully pinned objects");
                    }
                    Err(e) => {
                        log::error!("Error pinning objects: {e:?}");
                        let _ = ExecutorActor::registration_error(
                            transaction.hash_string(),
                            e.to_string(),
                        );
                    }
                }
            }

            manager
        };

        match manager.bundle(content_id.clone()).await {
            Ok(()) => match create_program_id(content_id, &transaction) {
                Ok(_program_id) => {
                    if let Err(e) = ExecutorActor::registration_success(transaction.clone()) {
                        log::error!("Executor Error: Registration failed: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Executor Error: Registration failed: {e:?}");
                    if let Err(e) =
                        ExecutorActor::registration_error(transaction.hash_string(), e.to_string())
                    {
                        log::error!(
                            "Executor Error: Failure while handling registration error: {e:?}"
                        );
                    }
                }
            },
            Err(e) => {
                log::error!("Executor Error: Registration failed: {e:?}");
                if let Err(e) =
                    ExecutorActor::registration_error(transaction.hash_string(), e.to_string())
                {
                    log::error!("Executor Error: Failure while handling registration error: {e:?}");
                }
            }
        }
    }
    async fn set(engine: Arc<Mutex<ExecutionEngine<WsClient>>>, transaction: Transaction) {
        let program_id = transaction.program_id();
        let op = transaction.op();
        let inputs = transaction.inputs();
        let transaction_hash = transaction.hash_string();

        match get_account(program_id).await {
            Some(_) => {
                let state = engine.lock().await;
                match state
                    .parse_inputs(/*&schema,*/ &transaction, op, inputs)
                    .await
                {
                    Ok(_) => {
                        // set in pending transactions
                        // will need to add optional inputs to pending
                        // transactions
                        //
                        // if the transaction is next up, simply execute
                        // if not send response to user that the tx is pending
                        match state.set_pending_call(transaction.clone()) {
                            Ok(()) => {
                                // return user the transaction_hash
                                let _ = ExecutorActor::call_queued_for_exec(transaction);
                            }
                            Err(e) => {
                                // return error to user
                                let error_string =
                                    format!("unable to set transaction in pending: {e:?}");
                                log::error!("{}", error_string);
                                let _ = ExecutorActor::execution_error(
                                    &transaction_hash,
                                    std::io::Error::new(std::io::ErrorKind::Other, error_string),
                                );
                            }
                        }
                    }
                    Err(e) => {
                        let error_string = format!("unable to set transaction in pending: {e:?}");
                        log::error!("{}", error_string);
                        let _ = ExecutorActor::execution_error(
                            &transaction_hash,
                            std::io::Error::new(std::io::ErrorKind::Other, error_string),
                        );
                    }
                }
            }
            None => {
                let error_string = "program account does not exist, unable to execute";
                log::error!("{}", &error_string);
                let _ = ExecutorActor::execution_error(
                    &transaction_hash,
                    std::io::Error::new(std::io::ErrorKind::Other, error_string),
                );
            }
        }
    }
    async fn exec(engine: Arc<Mutex<ExecutionEngine<WsClient>>>, transaction: Transaction) {
        let program_id = transaction.program_id();
        let op = transaction.op();
        let inputs = transaction.inputs();
        let transaction_hash = transaction.hash_string();

        match get_account(program_id).await {
            Some(account) => {
                let content_id = account
                    .program_account_metadata()
                    .inner()
                    .get("content_id")
                    .unwrap_or(&program_id.to_full_string())
                    .to_owned();
                let mut state = engine.lock().await;
                match state
                    .parse_inputs(/*&schema,*/ &transaction, op, inputs)
                    .await
                {
                    Ok(inputs) => {
                        match state
                            .execute(
                                content_id,
                                program_id.to_full_string(),
                                transaction,
                                inputs,
                                &transaction_hash,
                            )
                            .await
                        {
                            Ok(handle) => {
                                log::warn!("result successful, placing handle in handles");
                                state.handles.insert(
                                    (program_id.to_full_string(), transaction_hash),
                                    handle,
                                );
                            }
                            Err(e) => {
                                let error_string = format!(
                                    "Executor Error: Error while attempting to run container: {e:?}"
                                );
                                log::error!("{}", &error_string);
                                let _ = ExecutorActor::execution_error(&transaction_hash, e);
                            }
                        }
                    }
                    Err(e) => {
                        let error_string = format!("Executor Error: Error parsing inputs: {e:?}");
                        log::error!("{}", &error_string);
                        let _ = ExecutorActor::execution_error(&transaction_hash, e);
                    }
                }
            }
            None => {
                let error_string = "program account does not exist, unable to execute";
                log::error!("{}", &error_string);
                let _ = ExecutorActor::execution_error(
                    &transaction_hash,
                    std::io::Error::new(std::io::ErrorKind::Other, error_string),
                );
            }
        }
    }
    async fn results(
        engine: Arc<Mutex<ExecutionEngine<WsClient>>>,
        transaction: Option<Transaction>,
        content_id: String,
        program_id: String,
        transaction_hash: Option<String>,
    ) {
        // Handle the results of an execution
        log::warn!(
            "content_id: {:?}, transaction_id: {:?}",
            content_id,
            transaction_hash
        );
        match transaction_hash {
            Some(hash) => {
                let handle_res = {
                    let mut state = engine.lock().await;
                    state.handles.remove(&(program_id.clone(), hash.clone()))
                };
                match handle_res {
                    Some(handle) => {
                        log::warn!("discovered handle");
                        match handle.await {
                            Ok(Ok(output)) => {
                                log::warn!("Outputs: {:#?}", &output);
                                match transaction {
                                    Some(tx) => {
                                        match ExecutorActor::execution_success(&tx, &output) {
                                            Ok(_) => {
                                                log::info!(
                                                    "Forwarded output and transaction hash to Engine"
                                                );
                                            }
                                            Err(e) => {
                                                log::error!(
                                                    "Execution Error: Execution process failed: {e:?}"
                                                );
                                            }
                                        }
                                    }
                                    None => {
                                        log::error!("transaction not provided to the executor");
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                log::error!(
                                    "Error returned from `call` transaction: {}, program: {}: {:?}",
                                    &hash,
                                    &content_id,
                                    e
                                );
                                if let Err(e) = ExecutorActor::execution_error(&hash, e) {
                                    log::error!(
                                        "Executor Error: Error while handling execution error: {e:?}"
                                    );
                                }
                            }
                            Err(e) => {
                                log::error!(
                                            "Error inside program call thread: {:?}: transaction: {}, program: {}",
                                            e, &hash, &content_id
                                        );
                                if let Err(e) = ExecutorActor::execution_error(&hash, e) {
                                    log::error!(
                                        "Executor Error: Error while handling execution error: {e:?}"
                                    );
                                }
                            }
                        }
                    }
                    None => {
                        log::error!(
                            "Error: Handle does not exist for transaction: {}, program: {}",
                            &hash,
                            &content_id
                        )
                    }
                }
            }
            None => {
                log::error!("Error: No transaction hash provided");
            }
        }
    }
}

#[cfg(feature = "local")]
#[async_trait]
impl Actor for ExecutorActor {
    type Msg = ExecutorMessage;
    type State = Arc<Mutex<ExecutionEngine<WsClient>>>;
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
        _: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let engine_ptr = Arc::clone(state);
        match message {
            ExecutorMessage::Retrieve { .. } => {
                // Retrieve the package from IPFS
                // Convert the package into a payload
                // Build container
            }
            ExecutorMessage::Create {
                transaction,
                content_id,
            } => {
                let fut = ExecutorActor::create(engine_ptr, transaction, content_id);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            ExecutorMessage::Set { transaction } => {
                let fut = ExecutorActor::set(engine_ptr, transaction);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            ExecutorMessage::Exec { transaction } => {
                let fut = ExecutorActor::exec(engine_ptr, transaction);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            ExecutorMessage::Results {
                content_id,
                program_id,
                transaction_hash,
                transaction,
            } => {
                let fut = ExecutorActor::results(
                    engine_ptr,
                    transaction,
                    content_id,
                    program_id,
                    transaction_hash,
                );
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            _ => {}
        }

        Ok(())
    }
}

#[cfg(feature = "remote")]
#[async_trait]
impl Actor for ExecutorActor {
    type Msg = ExecutorMessage;
    type State = ExecutionEngine<WsClient>;
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
        _: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ExecutorMessage::Retrieve {
                content_id,
                program_id: _,
                transaction,
            } => {
                // Used to retrieve Schema under this model
                //    #[method(name = "pin_object")]
                match state.storage_rpc_client.pinned_status(&content_id).await {
                    Ok(()) => {
                        log::info!("Item: {content_id} is already pinned, inform requestor");
                        // check if the program account exists
                        if let Err(e) = self.registration_success(transaction) {
                            log::error!("Error in in self.registration_success: {e}");
                        }
                    }
                    Err(e) => {
                        log::error!("Error in state.storage_rpc_client.is_pinned: {e}");
                        match state.storage_rpc_client.pin_object(&content_id, true).await {
                            Ok(results) => {
                                if let Err(e) = self.registration_success(transaction) {
                                    log::error!("Error in self.registration_success: {e}");
                                }
                                log::info!("Pinned Object to Storage Agent: {:?}", results);
                            }
                            Err(e) => {
                                log::error!("Error pinning object to storage agent: {e}");
                            }
                        }
                    }
                }
            }
            ExecutorMessage::Exec { transaction } => {
                // Fire off RPC call to compute runtime
                // program_address, op, inputs, transaction_hash
                let inputs = Inputs {
                    version: 1,
                    account_info: None,
                    transaction: transaction.clone(),
                    op: transaction.op(),
                    inputs: transaction.inputs(),
                };
                if let Some(account) = get_account(transaction.to()).await {
                    let metadata = account.program_account_metadata();
                    if let Some(cid) = metadata.inner().get("content_id") {
                        log::info!("found cid, converting inputs to json");
                        if let Ok(json_inputs) = serde_json::to_string(&inputs) {
                            log::info!("successfully converted inputs to json, queueing job");
                            match state
                                .compute_rpc_client
                                .queue_job(
                                    cid,
                                    ServiceJobType::Compute(ComputeJobExecutionType::SmartContract),
                                    json_inputs,
                                )
                                .await
                            {
                                // Spin a thread to periodically poll for the result
                                Ok(job_id) => {
                                    log::info!(
                                        "successfully queued job, spawning thread to poll status"
                                    );
                                    let (tx, rx) = tokio::sync::mpsc::channel(1);
                                    let poll_spawn_result = state.spawn_poll(job_id.clone(), rx);
                                    match poll_spawn_result {
                                        Ok(handle) => {
                                            // Stash the job_id
                                            log::info!("Received handle, stashing in PendingJob");
                                            let pending_job =
                                                PendingJob::new(handle, tx, transaction);
                                            state.pending.insert(job_id.clone(), pending_job);
                                        }
                                        Err(_e) => {}
                                    }
                                }
                                Err(_e) => {}
                            }
                        } else {
                            log::error!("Unable to serialize JSON inputs")
                        }
                    }
                } else {
                    log::error!("Program account does not exist, not a valid call");
                    let _ = self.execution_error(
                        &transaction.hash_string(),
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "Program account does not exist, not a valid call",
                        ),
                    );

                    log::error!("Returning error to scheduler to propagate to RPC");
                }
            }
            ExecutorMessage::PollJobStatus { job_id } => {
                match state.compute_rpc_client.job_status(job_id.clone()).await {
                    Ok(Some(status_response)) => match status_response.status() {
                        ServiceJobState::Complete(outputs) => {
                            log::info!("Received service job status response: {:?}", outputs);
                            if let Some(pending_job) = state.pending.remove(&job_id) {
                                let transaction = pending_job.transaction();
                                if let Err(e) = pending_job.kill_thread().await {
                                    log::error!("Error trying to kill job polling thread: {e}")
                                }
                                if let Err(e) = self.execution_success(transaction, outputs) {
                                    log::error!("Error attempting to handle a successful execution result: {e}");
                                }
                            }
                        }
                        ServiceJobState::Failed(err) => {
                            log::error!("Execution of job {job_id} failed: {err}");
                            if let Some(pending_job) = state.pending.remove(&job_id) {
                                let transaction = pending_job.transaction();
                                if let Err(e) = pending_job.kill_thread().await {
                                    log::error!("Error trying to kill job polling thread: {e}")
                                }
                                if let Err(e) = self.execution_error(
                                    &transaction.hash_string(),
                                    std::io::Error::new(std::io::ErrorKind::Other, err.to_string()),
                                ) {
                                    log::error!(
                                        "Unable to inform RPC client of execution failure: {e}"
                                    );
                                }
                            }
                        }
                        _ => {}
                    },
                    Ok(None) => {
                        log::error!("Unable to acquire status response from compute agent");
                    }
                    Err(e) => {
                        if let Some(pending_job) = state.pending.remove(&job_id) {
                            let transaction = pending_job.transaction();
                            if let Err(e) = pending_job.kill_thread().await {
                                log::error!("Error trying to kill job polling thread: {e}")
                            }
                            if let Err(e) = self.execution_error(&transaction.hash_string(), e) {
                                log::error!(
                                    "Unable to inform RPC client of execution failure: {e}"
                                );
                            }
                        }
                    }
                }
            }
            ExecutorMessage::Results { .. } => {}
            _ => {}
        }

        Ok(())
    }
}

impl ActorExt for ExecutorActor {
    type Output = ();
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
                    .install(|| async move { guard.next().await })
                    .await;
            }
        })
    }
}

pub struct ExecutorSupervisor {
    panic_tx: Sender<ActorCell>,
}
impl ExecutorSupervisor {
    pub fn new(panic_tx: Sender<ActorCell>) -> Self {
        Self { panic_tx }
    }
}
impl ActorName for ExecutorSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::Executor.to_string()
    }
}
#[derive(Debug, Error, Default)]
pub enum ExecutorSupervisorError {
    #[default]
    #[error("failed to acquire ExecutorSupervisor from registry")]
    RactorRegistryError,
}

#[async_trait]
impl Actor for ExecutorSupervisor {
    type Msg = ExecutorMessage;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        log::info!("Executor Supervisor running prestart routine");
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: SupervisionEvent,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        log::warn!("Received a supervision event: {:?}", message);
        match message {
            SupervisionEvent::ActorStarted(actor) => {
                log::info!(
                    "actor started: {:?}, status: {:?}",
                    actor.get_name(),
                    actor.get_status()
                );
            }
            SupervisionEvent::ActorPanicked(who, reason) => {
                log::error!("actor panicked: {:?}, err: {:?}", who.get_name(), reason);
                self.panic_tx.send(who).await.typecast().log_err(|e| e);
            }
            SupervisionEvent::ActorTerminated(who, _, reason) => {
                log::error!("actor terminated: {:?}, err: {:?}", who.get_name(), reason);
            }
            SupervisionEvent::PidLifecycleEvent(event) => {
                log::info!("pid lifecycle event: {:?}", event);
            }
            SupervisionEvent::ProcessGroupChanged(m) => {
                process_group_changed(m);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod executor_tests {
    use crate::{ActorExt, ExecutionEngine, ExecutorActor};
    use jsonrpsee::ws_client::WsClient;
    use lasr_compute::{OciBundler, OciBundlerBuilder, OciManager};
    use lasr_messages::{ActorType, ExecutorMessage};
    use lasr_types::Transaction;
    use ractor::Actor;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_executor_future_handler() {
        let executor_actor = ExecutorActor::new();
        let bundler: OciBundler<String, String> = OciBundlerBuilder::default()
            .runtime("/usr/local/bin/runsc".to_string())
            .base_images("./base_image".to_string())
            .containers("./containers".to_string())
            .payload_path("./payload".to_string())
            .build()
            .expect("failed to build bundler");
        let store = if let Ok(addr) = std::env::var("VIPFS_ADDRESS") {
            Some(addr.clone())
        } else {
            None
        };
        let oci_manager = OciManager::new(bundler, store);
        let mut engine = Arc::new(Mutex::new(ExecutionEngine::<WsClient>::new(oci_manager)));

        let (executor_actor_ref, _executor_handle) = Actor::spawn(
            Some(ActorType::Executor.to_string()),
            executor_actor.clone(),
            engine.clone(),
        )
        .await
        .expect("failed to spawn executor actor");

        executor_actor
            .handle(
                executor_actor_ref,
                ExecutorMessage::Set {
                    transaction: Transaction::default(),
                },
                &mut engine,
            )
            .await
            .unwrap();
        // TODO: Add other messages in the handle method
        {
            let guard = executor_actor.future_pool.lock().await;
            assert!(!guard.is_empty());
        }

        let future_thread_pool = tokio_rayon::rayon::ThreadPoolBuilder::new()
            .num_threads(num_cpus::get())
            .build()
            .unwrap();

        let actor_clone = executor_actor.clone();
        ExecutorActor::spawn_future_handler(actor_clone, future_thread_pool);

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            {
                let guard = executor_actor.future_pool.lock().await;
                if guard.is_empty() {
                    break;
                }
            }
            interval.tick().await;
        }
    }
}
