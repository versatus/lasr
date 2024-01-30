use serde::{Serialize, Deserialize};
use crate::commitment::BlobCommitment;
use crate::quorum::BlobQuorumParams;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobHeader {
    commitment: BlobCommitment,
    data_length: usize,
    blob_quorum_params: Vec<BlobQuorumParams>,
}

impl BlobHeader {
    pub fn commitment(&self) -> & BlobCommitment {
        &self.commitment
    }

    pub fn data_length(&self) -> usize {
        self.data_length
    }

    pub fn blob_quorum_params(&self) -> &Vec<BlobQuorumParams> {
        &self.blob_quorum_params
    }
}

impl Default for BlobHeader {
    fn default() -> Self {
        BlobHeader { 
            commitment: Default::default(),
            data_length: Default::default(),
            blob_quorum_params: Default::default()
        }
    }
}
