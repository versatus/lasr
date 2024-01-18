use std::{collections::HashMap, path::Path};
use ractor::{Actor, ActorRef, ActorProcessingErr};
use async_trait::async_trait;
use crate::{OciManager, ExecutorMessage};

// This will be a weighted LRU cache that captures the size of the 
// containers and kills/deletes LRU containers
pub struct DynCache;

#[allow(unused)]
pub struct ExecutionEngine {
    manager: OciManager,
    ipfs_client: ipfs_api::IpfsClient,
    handles: HashMap<(String, [u8; 32]), tokio::task::JoinHandle<std::io::Result<String>>>,
    cache: DynCache
}

impl ExecutionEngine {
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
        op: Option<String>,
        inputs: Option<Vec<String>>,
    ) -> std::io::Result<tokio::task::JoinHandle<std::io::Result<String>>> {
        let handle = self.manager.run_container(content_id, op, inputs).await?;
        Ok(handle)
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
            ExecutorMessage::Create(content_id, entrypoint, program_args) => {
                // Build the container spec and create the 
                // sandbox container image
                let res = state.create_bundle(content_id, entrypoint, program_args).await;
                if let Err(e) = res {
                    log::error!("Error executor.rs: 73: {e}");
                }
                // If payload has a constructor method/function should be executed
                // to return a Create instruction.
            },
            ExecutorMessage::Start(_content_id) => {
                // Warm up/start a container image
            }
            ExecutorMessage::Exec(content_id, op, inputs, tx_id) => {
                // Run container
                let res = state.execute(&content_id, op, inputs).await;
                if let Err(e) = &res {
                    log::error!("Error executor.rs: 83: {e}");
                };
                if let Ok(handle) = res {
                    state.handles.insert((content_id, tx_id), handle);
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
