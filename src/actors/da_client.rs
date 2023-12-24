use std::{fmt::Display, time::Duration};

use async_trait::async_trait;
use eigenda_client::{client::EigenDaGrpcClient, response::BlobResponse, proof::BlobVerificationProof, status::{BlobStatus, BlobResult}, blob::{DecodedBlob, EncodedBlob}};
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::OneshotSender};
use thiserror::Error;
use tokio::task::JoinHandle;
use crate::{Account, Address};
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

    async fn disperse_blobs(&self, accounts: Vec<Account>) -> Vec<(Address, BlobResponse)> {
        let mut responses = Vec::new();
        for account in accounts {
            match bincode::serialize(&account) {
                Ok(bytes) => {
                    log::info!("serialized account is {} bytes", bytes.len());
                    match self.client.disperse_blob(bytes, &0) {
                        Ok(resp) => { 
                            responses.push((account.address(), resp));
                        }
                        Err(e) => log::error!("failed to disperse blob: {}", e)
                    }
                }
                Err(e) => {
                    log::error!("failed to serialize account: {}", e);
                }
            }
        }
        responses
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
            DaClientMessage::StoreAccountBlobs { accounts } => {
                let _blob_responses = self.disperse_blobs(accounts).await;
                // for response in blob_responses {
                    // self.blob_cache_writer.send(response).await?;
                // }
                // Need to send blob responses somewhere to be stored,
                // and checked for dispersal.
            },
            DaClientMessage::StoreContractBlobs { .. /*contracts*/ } => {},
            DaClientMessage::StoreTransactionBlob => {}, 
            DaClientMessage::ValidateBlob { request_id, address, tx } => {
                let _ = validate_blob(self.client.clone(), request_id, address, tx).await;
                // Spawn a tokio task to poll EigenDa for the validated blob
            },
            // Optimistically and naively retreive account blobs
            DaClientMessage::RetrieveBlob { batch_header_hash, blob_index, tx } => {
                let blob = self.client.retrieve_blob(&batch_header_hash.into(), blob_index)?;
                let encoded_blob = EncodedBlob::from_str(&blob)?;
                let decoded = DecodedBlob::from_encoded(encoded_blob)?;
                let account: Account = bincode::deserialize(&decoded.data())?;
                let _ = tx.send(Some(account)).map_err(|e| Box::new(
                        DaClientError::Custom(format!("{:?}", e))))?;
                //log::info!("successfully decoded account blob: {:?}", account);
            },
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
    address: Address,
    tx: OneshotSender<(Address, BlobVerificationProof)>
) -> Result<(), std::io::Error> {
    let mut status = get_blob_status(&client, &request_id).await?;
    while status.status().clone() != BlobResult::Confirmed {
        //log::info!("blob result not yet confirmed... polling again in 10 seconds");
        tokio::time::sleep(Duration::from_secs(60)).await;
        status = get_blob_status(&client, &request_id).await?;
    }
    let proof = status.info().blob_verification_proof(); 
    let _ = tx.send((address, proof.clone()));
    Ok(())
}

async fn validate_blob(
    client: EigenDaGrpcClient,
    request_id: String,
    address: Address, 
    tx: OneshotSender<(Address, BlobVerificationProof)>
) -> JoinHandle<Result<(), std::io::Error>> {
    log::info!("spawning blob validation task");
    tokio::task::spawn(async move { poll_blob_status(
        client.clone(), request_id, address, tx).await
    })
}
