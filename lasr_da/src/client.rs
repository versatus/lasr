use derive_builder::Builder;
use crate::response::BlobResponse;
use crate::payload::EigenDaBlobPayload;
use crate::status::BlobStatus;
use crate::batch::BatchHeaderHash;
use crate::grpcurl_command;
use std::str::FromStr;

#[derive(Builder, Clone, Debug)]
pub struct EigenDaGrpcClient {
    proto_path: String,
    server_address: String,
    adversary_threshold: u32,
    quorum_threshold: u32,
}

impl EigenDaGrpcClient {
    fn get_payload(&self, encoded_data: String, quorum_id: &u32) -> EigenDaBlobPayload {
        EigenDaBlobPayload::new(  
            encoded_data,
            quorum_id,
            &self.adversary_threshold,
            &self.quorum_threshold,
        ) 
    }

    pub fn disperse_blob(&self, encoded_data: String, quorum_id: &u32) -> Result<BlobResponse, std::io::Error> {
        let payload: String = self.get_payload(encoded_data, quorum_id).into();

        let output = grpcurl_command!(
            "-proto", &self.proto_path,
            "-d", &payload,
            &self.server_address,
            "disperser.Disperser/DisperseBlob"
        )?;

        if output.status.success() {
            let response: BlobResponse = String::from_utf8(output.stdout).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::Other, err.to_string()
                )
            })?.into();
            Ok(response)
        } else {
            let error_message = String::from_utf8(output.stderr).map_err(|err| {
                    std::io::Error::new(std::io::ErrorKind::Other, err.to_string())
                }
            )?;
            Err(std::io::Error::new(std::io::ErrorKind::Other, error_message))
        }
    }

    pub fn get_blob_status(&self, request_id: &str) -> Result<BlobStatus, std::io::Error> {
        let mut payload = String::new();
        payload.push_str(r#"{"#);
        payload.push_str(r#""request_id":"#);
        payload.push_str(&format!(r#""{}""#, request_id));
        payload.push_str(r#"}"#);

        let output = grpcurl_command!(
            "-proto", &self.proto_path,
            "-d", &payload,
            &self.server_address,
            "disperser.Disperser/GetBlobStatus"
        )?;

        if output.status.success() {
            let response = String::from_utf8(output.stdout).map_err(|err| {
                std::io::Error::new(std::io::ErrorKind::Other, err.to_string())
            })?;

            let res = BlobStatus::from_str(&response);
            if let Err(e) = &res {
                dbg!(response);
                log::error!("{}", e);
            }
            Ok(res?)
        } else {
            let error_message = String::from_utf8(output.stderr).map_err(|err| {
                std::io::Error::new(std::io::ErrorKind::Other, err.to_string())
            })?;
            Err(std::io::Error::new(std::io::ErrorKind::Other, error_message))
        }
    }

    pub fn retrieve_blob(&self, batch_header_hash: &BatchHeaderHash, blob_index: u128) -> Result<String, std::io::Error> {
        let mut payload = String::new();
        payload.push_str(r#"{"#);
        payload.push_str(r#""batch_header_hash":"#);
        payload.push_str(&format!(r#""{}""#, batch_header_hash.to_string()));
        payload.push_str(r#", "blob_index":"#);
        payload.push_str(&format!(r#""{}""#, blob_index));
        payload.push_str(r#"}"#);

        let output = grpcurl_command!(
            "-proto", &self.proto_path,
            "-d", &payload,
            &self.server_address,
            "disperser.Disperser/RetrieveBlob"
        )?;

        if output.status.success() {
            let response = String::from_utf8(output.stdout).map_err(|err| {
                std::io::Error::new(std::io::ErrorKind::Other, err.to_string())
            })?;

            Ok(response)
        } else {
            let error_message = String::from_utf8(output.stderr).map_err(|err| {
                std::io::Error::new(std::io::ErrorKind::Other, err.to_string())
            })?;

            Err(std::io::Error::new(std::io::ErrorKind::Other, error_message))
        }
    }
}
