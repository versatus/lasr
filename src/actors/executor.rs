use std::{collections::HashMap, path::Path};
use ractor::{Actor, ActorRef, ActorProcessingErr};
use async_trait::async_trait;
use crate::{OciManager, ExecutorMessage, Outputs, ProgramSchema, Inputs, Required, SchedulerMessage, ActorType};
use serde::{Serialize, Deserialize};

// This will be a weighted LRU cache that captures the size of the 
// containers and kills/deletes LRU containers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynCache;

#[allow(unused)]
pub struct ExecutionEngine {
    manager: OciManager,
    ipfs_client: ipfs_api::IpfsClient,
    handles: HashMap<(String, Option<[u8; 32]>), tokio::task::JoinHandle<std::io::Result<String>>>,
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
        log::info!("creating bundle for {:?}", &content_id.as_ref().to_str());
        self.manager.bundle(&content_id, crate::BaseImage::Bin).await?;
        log::info!("adding payload for {:?}", &content_id.as_ref().to_str());
        self.manager.add_payload(&content_id).await?;
        log::info!("writing base spec {:?}", &content_id.as_ref().to_str());
        self.manager.base_spec(&content_id).await?;
        log::info!("customizing spec {:?}", &content_id.as_ref().to_str());
        self.manager.customize_spec(&content_id, &entrypoint, program_args)?;
        Ok(())
    }

    pub(super) async fn execute(
        &self,
        content_id: impl AsRef<Path> + Send + 'static,
        inputs: Inputs,
        transaction_id: Option<[u8; 32]>
    ) -> std::io::Result<tokio::task::JoinHandle<std::io::Result<String>>> {
        let handle = self.manager.run_container(content_id, inputs, transaction_id).await?;
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
    fn respond_with_registration_error(&self, transaction_hash: String, error_string: String) -> std::io::Result<()> {
        let actor: ActorRef<SchedulerMessage> = ractor::registry::where_is(ActorType::Scheduler.to_string()).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "unable to acquire Scheduler")
        )?.into();

        let message = SchedulerMessage::RegistrationFailure { transaction_hash, error_string };
        actor.cast(message).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        })?;

        Ok(())
    }

    fn respond_with_registration_success(&self, transaction_hash: String) -> std::io::Result<()> {
        let actor: ActorRef<SchedulerMessage> = ractor::registry::where_is(ActorType::Scheduler.to_string()).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "unable to acquire Scheduler")
        )?.into();

        let message = SchedulerMessage::RegistrationSuccess { transaction_hash };
        actor.cast(message).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        })?;

        Ok(())
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
                let res = state.create_bundle(program_id.to_full_string(), entrypoint, program_args).await;
                if let Err(e) = res {
                    log::error!("Error: executor.rs: 153: {e}");
                    if let Err(e) = self.respond_with_registration_error(transaction_hash, e.to_string()) {
                        log::error!("Error: executor.rs: 156: {e}");
                    };
                } else {
                    if let Err(e) = self.respond_with_registration_success(transaction_hash) {
                        log::error!("Error: executor.rs: 160: {e}");
                    };
                }
                // If payload has a constructor method/function should be executed
                // to return a Create instruction.
                //

            },
            ExecutorMessage::Start(_content_id) => {
                // Warm up/start a container image
            }
            ExecutorMessage::Exec {
                program_id, op, inputs, transaction_id 
            } => {
                // Run container
                // parse inputs
                // get config
                let schema_result = state.get_program_schema(program_id.to_full_string()); 
                if let Ok(schema) = &schema_result {
                    let pre_requisites = schema.get_prerequisites(&op);
                    if let Ok(reqs) = &pre_requisites {
                        let _ = state.handle_prerequisites(reqs);
                        dbg!(&inputs);
                        let inputs_res = state.parse_inputs(schema, op, inputs);
                        if let Ok(inputs) = inputs_res {
                            let res = state.execute(program_id.to_full_string(), inputs, Some(transaction_id)).await;
                            if let Err(e) = &res {
                                log::error!("Error executor.rs: 83: {e}");
                            };
                            if let Ok(handle) = res {
                                log::info!("result successful, placing handle in handles");
                                state.handles.insert((program_id.to_full_string(), Some(transaction_id)), handle);
                            }
                        }
                    }

                    if let Err(e) = &pre_requisites {
                        log::error!("{}",e);
                    }
                } 
                if let Err(e) = &schema_result {
                    log::error!("{}",e);
                }
            },
            ExecutorMessage::Kill(_content_id) => {
                // Kill a container that is running
            },
            ExecutorMessage::Delete(_content_id) => {
                // Delete a container
            },
            ExecutorMessage::Results { content_id, transaction_id } => {
                // Handle the results of an execution
                log::info!("Received results for execution of container: content_id: {:?}, transaction_id: {:?}", content_id, transaction_id);
                dbg!(&state.handles);
                if let Some(handle) = state.handles.remove(&(content_id, transaction_id)) {
                    let res = handle.await;
                    log::info!("container results from ExecutorActor: {:?}", res);
                }
                log::info!("Forwarding results to engine to handle application");
            },
        }

        Ok(())
    }
}
