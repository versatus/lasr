use std::{fmt::Display, time::Duration};

use async_trait::async_trait;
use eigenda_client::{client::EigenDaGrpcClient, response::BlobResponse, proof::BlobVerificationProof, status::{BlobStatus, BlobResult}};
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::OneshotSender};
use thiserror::Error;
use tokio::task::JoinHandle;
use crate::Batch;
use crate::DaClientMessage;


#[derive(Clone, Debug)]
pub struct DaClient {
    client: EigenDaGrpcClient
}

#[derive(Clone, Debug, Error)]
pub enum DaClientError {
    Custom(String)
}

impl Display for DaClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Default for DaClientError {
    fn default() -> Self {
        DaClientError::Custom(
            "DA Client unable to acquire actor".to_string()
        )
    }
}

impl DaClient {
    pub fn new(
        client: EigenDaGrpcClient,
    ) -> Self {
        Self { 
            client, 
        }
    }

    async fn disperse_blobs(&self, batch: String) -> Result<BlobResponse, std::io::Error> {
        let response = self.client.disperse_blob(batch, &0)?;
        Ok(response)
    }

}


#[async_trait]
impl Actor for DaClient {
    type Msg = DaClientMessage;
    type State = (); 
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            // Optimistically and naively store account blobs
            DaClientMessage::StoreBatch { batch, tx } => {
                let blob_response = self.disperse_blobs(batch).await;
                let _ = tx.send(blob_response);
                // for response in blob_responses {
                    // self.blob_cache_writer.send(response).await?;
                // }
                // Need to send blob responses somewhere to be stored,
                // and checked for dispersal.
            },
            DaClientMessage::ValidateBlob { request_id, tx } => {
                let _ = validate_blob(self.client.clone(), request_id, tx).await;
                // Spawn a tokio task to poll EigenDa for the validated blob
            },
            // Optimistically and naively retreive account blobs
            DaClientMessage::RetrieveAccount { address, batch_header_hash, blob_index, tx } => {
                let blob = self.client.retrieve_blob(&batch_header_hash.to_string().into(), blob_index)?;
                let batch: Batch = Batch::decode_batch(&blob)?;
                let account = batch.get_account(address);
                let _ = tx.send(account.clone()).map_err(|e| Box::new(
                        DaClientError::Custom(format!("{:?}", e))))?;
                log::info!("successfully decoded account blob: {:?}", account);
            },
            DaClientMessage::RetrieveTransaction { .. } => {},
            DaClientMessage::RetrieveContract { .. } => {},
            _ => {}
        } 
        return Ok(())
    }
}


async fn get_blob_status(
    client: &EigenDaGrpcClient, 
    request_id: &String
) -> Result<BlobStatus, std::io::Error> { 
    client.clone().get_blob_status(&request_id.clone()[..])
}

async fn poll_blob_status(
    client: EigenDaGrpcClient, 
    request_id: String, 
    tx: OneshotSender<(String, BlobVerificationProof)>
) -> Result<(), std::io::Error> {
    let mut status = get_blob_status(&client, &request_id).await?;
    while status.status().clone() != BlobResult::Confirmed {
        //log::info!("blob result not yet confirmed... polling again in 10 seconds");
        tokio::time::sleep(Duration::from_secs(60)).await;
        status = get_blob_status(&client, &request_id).await?;
    }
    let proof = status.info().blob_verification_proof(); 
    let _ = tx.send((request_id, proof.clone()));
    Ok(())
}

async fn validate_blob(
    client: EigenDaGrpcClient,
    request_id: String,
    tx: OneshotSender<(String, BlobVerificationProof)>
) -> JoinHandle<Result<(), std::io::Error>> {
    log::info!("spawning blob validation task");
    tokio::task::spawn(async move { 
        poll_blob_status(
            client.clone(), request_id, tx
        ).await
    })
}
