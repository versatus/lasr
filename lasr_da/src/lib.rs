use ritelinked::LinkedHashSet;
use crate::cache::LruCache;
use crate::response::BlobResponse;

pub mod macros;
pub mod cache;
pub mod methods;
pub mod status;
pub mod error;
pub mod result;
pub mod response;
pub mod info;
pub mod header;
pub mod proof;
pub mod commitment;
pub mod quorum;
pub mod meta;
pub mod batch;
pub mod fee;
pub mod record;
pub mod client;
pub mod payload;
pub mod blob;

pub use client::*;

impl LruCache for LinkedHashSet<BlobResponse> {
    type Value = BlobResponse;

    fn cache(&mut self, item: &Self::Value) {
        self.insert(item.clone());
    }

    fn get(&self, key: &Self::Item) -> Option<&Self::Value> {
        self.get(&key)
    }
}

#[cfg(test)]
mod tests {
    use crate::blob::{EncodedBlob, DecodedBlob};
    use crate::client::{EigenDaGrpcClientBuilder, EigenDaGrpcClient};
    use crate::status::BlobResult;
    use std::thread;
    use std::time::Duration;
    
    #[test]
    fn test_disperse_get_status_and_retrieve_blob() {

        let client = create_client(40, 60); 
        let arbitrary_data = base64::encode("ArbitraryData");
        
        let blob_response = client.disperse_blob(arbitrary_data, &0).unwrap();
        let mut blob_status = client.get_blob_status(&blob_response.request_id()).unwrap(); 
        while blob_status.status() != &BlobResult::Confirmed {
            thread::sleep(Duration::from_secs(30));
            blob_status = client.get_blob_status(&blob_response.request_id()).unwrap();
        }

        let batch_header_hash = blob_status.batch_header_hash().unwrap();
        let blob_index = blob_status.blob_index().unwrap();

        let blob = client.retrieve_blob(batch_header_hash, blob_index).unwrap();

        let blob = EncodedBlob::from_str(&blob).unwrap();

        let decoded_blob = DecodedBlob::from_encoded(blob).unwrap();
        println!("{}", decoded_blob.len());
    }

    fn create_client(adversary_threshold: u32, quorum_threshold: u32) -> EigenDaGrpcClient {
        EigenDaGrpcClientBuilder::default()
            .proto_path("./eigenda/api/proto/disperser/disperser.proto".to_string())
            .server_address("disperser-goerli.eigenda.xyz:443".to_string())
            .adversary_threshold(adversary_threshold)
            .quorum_threshold(quorum_threshold)
            .build()
            .unwrap()
    } 
}
