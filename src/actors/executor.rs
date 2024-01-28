use std::{collections::HashMap, path::Path, time::Duration, str::FromStr};
use jsonrpsee::{core::client::ClientT, ws_client::WsClient};
use ractor::{Actor, ActorRef, ActorProcessingErr};
use async_trait::async_trait;
use crate::{OciManager, ExecutorMessage, ProgramSchema, Inputs, Required, SchedulerMessage, ActorType, EngineMessage, Transaction};
use serde::{Serialize, Deserialize};
use tokio::{task::JoinHandle, sync::mpsc::{Sender, Receiver}};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteExecutorActor;

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

#[derive(Debug)]
pub struct RemoteExecutionEngine<C: ClientT> {
    client: C,
    pending: HashMap<uuid::Uuid, PendingJob>
}

impl<C: ClientT> RemoteExecutionEngine<C> {
    pub fn new(client: C) -> Self {
        Self {
            client,
            pending: HashMap::new()
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

// This will be a weighted LRU cache that captures the size of the 
// containers and kills/deletes LRU containers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynCache;

#[allow(unused)]
pub struct ExecutionEngine {
    manager: OciManager,
    ipfs_client: ipfs_api::IpfsClient,
    handles: HashMap<(String, String), tokio::task::JoinHandle<std::io::Result<String>>>,
    cache: DynCache
}

impl ExecutionEngine {
    pub fn new(
        manager: OciManager,  
        ipfs_client: ipfs_api::IpfsClient,
    ) -> Self  {
        Self { manager, ipfs_client, handles: HashMap::new(), cache: DynCache }
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

    pub(super) fn parse_inputs(&self, _schema: &ProgramSchema, op: String, inputs: String) -> std::io::Result<Inputs> {
        return Ok(Inputs { version: 1, account_info: None, op, inputs } )
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

#[async_trait]
impl Actor for ExecutorActor {
    type Msg = ExecutorMessage;
    type State = ExecutionEngine; 
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
            ExecutorMessage::Retrieve(_content_id) => {
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
                                match state.parse_inputs(&schema, op, inputs) {
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
                                        dbg!(&output);
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


#[async_trait]
impl Actor for RemoteExecutorActor {
    type Msg = ExecutorMessage;
    type State = RemoteExecutionEngine<WsClient>; 
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
            ExecutorMessage::Retrieve(content_id) => {
                // Used to retrieve Schema under this model
                match state.client.request::<String, &[u8]>("get_object", content_id.as_bytes()).await {
                    Ok(_job_id) => {
                    }
                    Err(_e) => {
                    }
                }
            },
            ExecutorMessage::Exec { transaction } => {
                // Fire off RPC call to compute runtime
                // program_id, op, inputs, transaction_hash
                let program_id = transaction.program_id();
                let op = transaction.op();
                let inputs = transaction.inputs();
                let transaction_hash = transaction.hash_string();
                match state.client.request::<String, (String, String, String)>(
                    "queue_job", (
                        program_id.to_full_string(),
                        op,
                        inputs
                    )
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
                                        let pending_job = PendingJob::new(handle, tx, transaction_hash);
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

            },
            ExecutorMessage::PollJobStatus { job_id } => {
                match state.client.request::<String, &[u8]>(
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
