use std::{collections::HashMap, path::Path, time::Duration, str::FromStr, fs::metadata};
use jsonrpsee::{core::client::ClientT, ws_client::WsClient};
use ractor::{Actor, ActorRef, ActorProcessingErr};
use async_trait::async_trait;
use crate::{OciManager, ExecutorMessage, ProgramSchema, Inputs, Required, SchedulerMessage, ActorType, EngineMessage, Transaction, get_account, Address, BatcherMessage};
use serde::{Serialize, Deserialize};
use tokio::{task::JoinHandle, sync::mpsc::{Sender, Receiver}};
use internal_rpc::job_queue::job::{ComputeJobExecutionType, ServiceJobType};
use internal_rpc::api::InternalRpcApiClient;

#[derive(Debug)]
#[allow(unused)]
pub struct PendingJob {
    handle: JoinHandle<std::io::Result<()>>,
    sender: Sender<()>,
    transaction_hash: String,
}

impl PendingJob {
    pub fn new(
        handle: JoinHandle<std::io::Result<()>>,
        sender: Sender<()>, 
        transaction_hash: String
    ) -> Self {
        Self { handle, sender, transaction_hash } }

    pub async fn join_handle(self) {
        let _ = self.handle.await;
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
    #[cfg(feature = "local")]
    ipfs_client: ipfs_api::IpfsClient,
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
            cache: DynCache
        }
    }

    pub(super) fn spawn_poll(&self, job_id: uuid::Uuid, mut rx: Receiver<()>) -> std::io::Result<JoinHandle<std::io::Result<()>>> {
        Ok(tokio::task::spawn(async move {
            let mut attempts = 0;
            let actor: ActorRef<ExecutorMessage> = ractor::registry::where_is(ActorType::RemoteExecutor.to_string()).ok_or(
                std::io::Error::new(std::io::ErrorKind::Other, "unable to locate remote executor")
            )?.into();

            while attempts < 20 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if let Ok(()) = rx.try_recv() {
                    break
                }
                let message = ExecutorMessage::PollJobStatus { job_id: job_id.clone() };
                actor.clone().cast(message).map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                })?;
                attempts += 1;
            }

            Ok(())
        }))
    }
}

#[cfg(feature = "local")]
impl<C: ClientT> ExecutionEngine<C> {
    pub fn new(
        manager: OciManager,  
        ipfs_client: ipfs_api::IpfsClient,
    ) -> Self  {
        Self { manager, ipfs_client, handles: HashMap::new(), cache: DynCache, phantom: std::marker::PhantomData }
    }

    pub(super) async fn create_bundle(
        &self,
        content_id: impl AsRef<Path>,
        entrypoint: String,
        program_args: Option<Vec<String>>
    ) -> std::io::Result<()> {
        self.manager.bundle(&content_id, crate::BaseImage::Bin).await?;
        self.manager.add_payload(&content_id).await?;
        self.manager.base_spec(&content_id).await?;
        self.manager.customize_spec(&content_id, &entrypoint, program_args)?;
        Ok(())
    }

    pub(super) async fn execute(
        &self,
        content_id: impl AsRef<Path> + Send + 'static,
        transaction: Transaction,
        inputs: Inputs,
        transaction_hash: &String 
    ) -> std::io::Result<tokio::task::JoinHandle<std::io::Result<String>>> {
        let handle = self.manager.run_container(content_id, Some(transaction), inputs, Some(transaction_hash.clone())).await?;
        Ok(handle)
    }

    pub(super) fn get_program_schema(
        &self,
        content_id: impl AsRef<Path> + Send,
    ) -> std::io::Result<ProgramSchema> {
        self.manager.get_program_schema(content_id)
    }

    pub(super) fn parse_inputs(&self, _schema: &ProgramSchema, transaction: &Transaction, op: String, inputs: String) -> std::io::Result<Inputs> {
        return Ok(Inputs { version: 1, account_info: None, transaction: transaction.clone(), op, inputs } )
    }

    pub(super) fn handle_prerequisites(&self, pre_requisites: &Vec<Required>) -> std::io::Result<Vec<String>> {
        for req in pre_requisites {
            match req {
                Required::Call(call_map) => { 
                    // CallMap consists of:
                    //  - calling_program: TransactionFields,
                    //  - original_caller: TransactionFields,
                    //  - program_id: String, 
                    //  - inputs: Inputs, 
                    //  this should stand up an unsigned execution of the 
                    //  program id, the op tell us the operation to call
                    //  and the inputs tell us what to pass in
                    dbg!(call_map); 
                },
                Required::Read(read_params) => { dbg!(read_params); },
                Required::Lock(lock_pair) => { dbg!(lock_pair); },
                Required::Unlock(lock_pair) => { dbg!(lock_pair); },
            }
        }
        Ok(Vec::new())
    }
}


pub struct ExecutorActor;

impl ExecutorActor {
    fn registration_error(&self, transaction_hash: String, error_string: String) -> std::io::Result<()> {
        let actor: ActorRef<SchedulerMessage> = ractor::registry::where_is(ActorType::Scheduler.to_string()).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "unable to acquire Scheduler")
        )?.into();

        let message = SchedulerMessage::RegistrationFailure { transaction_hash, error_string };
        actor.cast(message).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        })?;

        Ok(())
    }

    #[cfg(feature = "local")]
    fn registration_success(&self, transaction_hash: String) -> std::io::Result<()> {
        let actor: ActorRef<SchedulerMessage> = ractor::registry::where_is(ActorType::Scheduler.to_string()).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "unable to acquire Scheduler")
        )?.into();

        let message = SchedulerMessage::RegistrationSuccess { transaction_hash };
        actor.cast(message).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        })?;

        Ok(())
    }

    #[cfg(feature = "remote")]
    fn registration_success(&self, content_id: String, program_id: Address, transaction: Transaction) -> std::io::Result<()> {

        let actor: ActorRef<SchedulerMessage> = ractor::registry::where_is(ActorType::Scheduler.to_string()).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "unable to acquire Scheduler")
        )?.into();

        let message = SchedulerMessage::RegistrationSuccess { program_id, transaction: transaction.clone() };
        actor.cast(message).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        })?;

        let message = BatcherMessage::AppendTransaction { transaction, outputs: None };

        let batcher: ActorRef<BatcherMessage> = ractor::registry::where_is(ActorType::Batcher.to_string()).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "unable to acquire Batcher")
        )?.into();

        batcher.cast(message);

        Ok(())
    }


    fn execution_success(
        &self,
        transaction: &Transaction,
        transaction_hash: &String,
        outputs: &String
    ) -> std::io::Result<()> {
        let actor: ActorRef<EngineMessage> = ractor::registry::where_is(ActorType::Engine.to_string()).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "Unable to acquire Engine actor")
        )?.into();

        let message = EngineMessage::CallSuccess { 
            transaction: transaction.clone(),
            transaction_hash: transaction_hash.clone(), 
            outputs: outputs.clone() 
        };

        actor.cast(message).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        })?;

        Ok(())
    }

    #[allow(unused)]
    fn execution_error(
        &self,
        transaction_hash: &String,
        err: impl std::error::Error
    ) -> std::io::Result<()> {
        // Send stderr output to client via RPC return 
        todo!()
    }
}


#[cfg(feature = "local")]
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
                ..
            }=> {
                // Retrieve the package from IPFS
                // Convert the package into a payload
                // Build container
            },
            ExecutorMessage::Create { 
                transaction_hash,
                program_id, 
                entrypoint, 
                program_args,
                ..
            } => {
                // Build the container spec and create the container image 
                log::info!("Receieved request to create container image");
                match state.create_bundle(
                    program_id.to_full_string(),
                    entrypoint, 
                    program_args
                ).await {
                    Ok(()) => {
                        match self.registration_success(transaction_hash) {
                            Err(e) => {
                                log::error!("Error: executor.rs: 225: {e}");
                            }
                            _ => {
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Error: executor.rs: 219: {e}");
                        match self.registration_error(transaction_hash, e.to_string()) {
                            Err(e) => {
                                log::error!("Error: executor.rs: 235: {e}");
                            }
                            _ => {
                            }
                        }
                    }
                }
                // If payload has a constructor method/function should be executed
                // to return a Create instruction.
                //

            },
            ExecutorMessage::Exec {
                transaction
            } => {
                // Run container
                // parse inputs
                // program_id, op, inputs, transaction_hash 
                // get config
                let program_id = transaction.program_id();
                let op = transaction.op();
                let inputs = transaction.inputs();
                let transaction_hash = transaction.hash_string();
                match state.get_program_schema(program_id.to_full_string()) {
                    Ok(schema) => {
                        match schema.get_prerequisites(&op) {
                            Ok(pre_requisites) => {
                                let _ = state.handle_prerequisites(&pre_requisites);
                                match state.parse_inputs(&schema, &transaction, op, inputs) {
                                    Ok(inputs) => {
                                        match state.execute(
                                            program_id.to_full_string(),
                                            transaction,
                                            inputs,
                                            &transaction_hash
                                        ).await {
                                            Ok(handle) => {
                                                log::info!("result successful, placing handle in handles");
                                                state.handles.insert(
                                                    (program_id.to_full_string(), transaction_hash), 
                                                    handle
                                               );
                                            },
                                            Err(e) => {
                                                log::error!(
                                                    "Error calling state.execute: executor.rs: 265: {}", e
                                                );
                                            } 
                                        }
                                    },
                                    Err(e) => {
                                        log::error!(
                                            "Error calling state.parse_inputs: executor.rs: 263: {}", e
                                        );
                                    }
                                }
                            },
                            Err(e) => {
                                log::error!(
                                    "Error calling schema.get_prerequisites: executor.rs: 260: {}", e
                                );
                            }
                        }
                    },
                    Err(e) => {
                        log::error!("Error calling state.get_program_schema: executor.rs: 259: {}", e);
                    }
                }
            },
            ExecutorMessage::Results { content_id, transaction_hash, transaction } => {
                // Handle the results of an execution
                log::info!("Received results for execution of container:"); 
                log::info!("content_id: {:?}, transaction_id: {:?}", content_id, transaction_hash);
                dbg!(&state.handles);
                match transaction_hash {
                    Some(hash) => {
                        match state.handles.remove(&(content_id.clone(), hash.clone())) {
                            Some(handle) => {
                                match handle.await {
                                    Ok(Ok(output)) => {
                                        log::info!("Outputs: {:#?}", &output);
                                        match transaction {
                                            Some(tx) => { 
                                                match self.execution_success(
                                                    &tx, 
                                                    &hash, 
                                                    &output
                                                ) {
                                                    Ok(_) => {
                                                        log::info!(
                                                            "Forwarded output and transaction hash to Engine"
                                                        );
                                                    }
                                                    Err(e) => {
                                                        log::error!(
                                                            "Error calling self.execution_success: executor.rs: 326: {}", e
                                                        );
                                                    }
                                                }
                                            }
                                            None => {
                                                log::error!(
                                                    "transaction not provided to the executor"
                                                );
                                            }
                                        }
                                    },
                                    Ok(Err(e)) => {
                                        log::error!(
                                            "Error returned from `call` transaction: {}, program: {}: {}",
                                            &hash, &content_id, &e
                                        );
                                        match self.execution_error(&hash, e) {
                                            Err(e) => {
                                                log::error!(
                                                    "Error calling self.execution_error: executor.rs: 338: {}", e
                                                );
                                            },
                                            _ => {
                                            }
                                        }
                                    },
                                    Err(e) => { 
                                        log::error!(
                                            "Error inside program call thread: {}: transaction: {}, program: {}",
                                            &e, &hash, &content_id
                                        );
                                        match self.execution_error(&hash, e) {
                                            Err(e) => {
                                                log::error!(
                                                    "Error calling self.exeuction_error: executor.rs: 357: {}", e
                                                );       
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            None => { 
                                log::error!(
                                    "Error: Handle does not exist for transaction: {}, program: {}",
                                    &hash, &content_id
                                )
                            }
                        }
                    }
                    None => {
                        log::error!(
                            "Error: No transaction hash provided"
                        );
                    }
                }
            },
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
            ExecutorMessage::Retrieve { content_id, program_id, transaction } => {
                // Used to retrieve Schema under this model
                //    #[method(name = "pin_object")]
                match state.storage_rpc_client.is_pinned(&content_id).await {
                    Ok(true) => {
                        log::info!("Item: {content_id} is already pinned, inform requestor");
                    }
                    Ok(false) => {
                        match state.storage_rpc_client.pin_object(&content_id, true).await {
                            Ok(results) => {
                                if let Err(e) = self.registration_success(content_id, program_id, transaction) {
                                    log::error!("Error in state.handle_registration_success: {e}");
                                }
                                log::info!("Pinned Object to Storage Agent: {:?}", results);
                            }
                            Err(e) => {
                                log::error!("Error pinning object to storage agent: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Error in state.storage_rpc_client.is_pinned: {e}");
                        match state.storage_rpc_client.pin_object(&content_id, true).await {
                            Ok(results) => {
                                if let Err(e) = self.registration_success(content_id, program_id, transaction) {
                                    log::error!("Error in state.handle_registration_success: {e}");
                                }
                                log::info!("Pinned Object to Storage Agent: {:?}", results);
                            }
                            Err(e) => {
                                log::error!("Error pinning object to storage agent: {e}");
                            }
                        }
                    }
                }
            },
            ExecutorMessage::Exec { transaction } => {
                // Fire off RPC call to compute runtime
                // program_id, op, inputs, transaction_hash
                let inputs = Inputs {
                    version: 1,
                    account_info: None,
                    transaction: transaction.clone(),
                    op: transaction.op(),
                    inputs: transaction.inputs()
                };
                if let Some(account) = get_account(transaction.to()).await {
                    let metadata = account.program_account_metadata();
                    if let Some(cid) = metadata.inner().get("cid") {
                        if let Ok(json_inputs) = serde_json::to_string(&inputs) {
                            match state.compute_rpc_client.request::<String, (&str, ServiceJobType, String)>(
                                "queue_job", 
                                (cid, ServiceJobType::Compute(ComputeJobExecutionType::SmartContract), json_inputs)
                            ).await {
                                // Spin a thread to periodically poll for the result
                                Ok(job_id_string) => {
                                    let job_id_res = uuid::Uuid::from_str(&job_id_string);
                                    match job_id_res {
                                        Ok(job_id) => {
                                            let (tx, rx) = tokio::sync::mpsc::channel(1); 
                                            let poll_spawn_result = state.spawn_poll(job_id.clone(), rx);
                                            match poll_spawn_result {
                                                Ok(handle) => {
                                                // Stash the job_id
                                                let pending_job = PendingJob::new(handle, tx, transaction.hash_string());
                                                state.pending.insert(job_id.clone(), pending_job);
                                                }
                                                Err(_e) => {}
                                            }
                                        },
                                        Err(_e) => {}
                                    }
                                }
                                Err(_e) => {}
                            }
                        } else {
                            log::error!("Unable to serialize JSON inputs")
                        }
                    }
                }
            },
            ExecutorMessage::PollJobStatus { job_id } => {
                match state.compute_rpc_client.request::<String, &[u8]>(
                    "job_status", 
                    job_id.to_string().as_bytes()
                ).await {
                    Ok(_outputs) => {
                    },
                    Err(_e) => {
                    }
                }
            }
            ExecutorMessage::Results { .. } => {},
            _ => {}
        }

        Ok(())
    }
}
