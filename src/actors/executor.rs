use std::{collections::HashMap, path::Path};
use ractor::{Actor, ActorRef, ActorProcessingErr};
use async_trait::async_trait;
use crate::{OciManager, ExecutorMessage, Outputs, ProgramSchema, Inputs, Required};
use serde::{Serialize, Deserialize};

// This will be a weighted LRU cache that captures the size of the 
// containers and kills/deletes LRU containers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynCache;

#[allow(unused)]
pub struct ExecutionEngine {
    manager: OciManager,
    ipfs_client: ipfs_api::IpfsClient,
    handles: HashMap<(String, [u8; 32]), tokio::task::JoinHandle<std::io::Result<Outputs>>>,
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
        content_id: impl AsRef<Path> + Send,
        inputs: Inputs,
    ) -> std::io::Result<tokio::task::JoinHandle<std::io::Result<String>>> {
        let handle = self.manager.run_container(content_id, inputs).await?;
        Ok(handle)
    }

    pub(super) fn get_program_schema(
        &self,
        content_id: impl AsRef<Path> + Send,
    ) -> std::io::Result<ProgramSchema> {
        self.manager.get_program_schema(content_id)
    }

    pub(super) fn parse_inputs(&self, schema: ProgramSchema, inputs: String) -> std::io::Result<Inputs> {
        unimplemented!()
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
                program_id, 
                entrypoint, 
                program_args,
                ..
            } => {
                // Build the container spec and create the container image 
                log::info!("Receieved request to create container image");
                let res = state.create_bundle(program_id.to_string(), entrypoint, program_args).await;
                if let Err(e) = res {
                    log::error!("Error executor.rs: 73: {e}");
                }
                // If payload has a constructor method/function should be executed
                // to return a Create instruction.
            },
            ExecutorMessage::Start(_content_id) => {
                // Warm up/start a container image
            }
            ExecutorMessage::Exec {
                program_id, op, inputs, .. 
            } => {
                // Run container
                //parse inputs
                // get config
                let schema_result = state.get_program_schema(program_id.to_string()); 
                if let Ok(schema) = &schema_result {
                    let pre_requisites = schema.get_prerequisites(&op);
                    if let Ok(reqs) = &pre_requisites {
                        let _ = state.handle_prerequisites(reqs);
                        dbg!(&inputs);
                        //let res = state.execute(program_id, op, inputs).await;
                        //if let Err(e) = &res {
                        //    log::error!("Error executor.rs: 83: {e}");
                        //};
                        //if let Ok(handle) = res {
                            //state.handles.insert((program_id.to_string(), transaction_id), handle);
                        //}
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
            ExecutorMessage::Results(_content_id, _outputs) => {
                // Handle the results of an execution
            },
        }

        Ok(())
    }
}
