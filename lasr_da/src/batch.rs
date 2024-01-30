use serde::{Serialize, Deserialize};
use crate::quorum::{BlobQuorumNumbers, BlobQuorumSignedPercentages};

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BatchHeaderHash(String);

impl BatchHeaderHash {
    pub fn new(value: String) -> Self {
        Self(value)
    }
}

impl ToString for BatchHeaderHash {
    fn to_string(&self) -> String {
        self.0.clone()
    }
}

impl From<String> for BatchHeaderHash {
    fn from(value: String) -> Self {
        BatchHeaderHash::new(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BatchHeader {
    batch_root: BlobBatchRoot,
    quorum_numbers: BlobQuorumNumbers,
    quorum_signed_percentages: BlobQuorumSignedPercentages,
    reference_block_number: u128
}

impl BatchHeader {
    pub fn batch_root(&self) -> &BlobBatchRoot {
        &self.batch_root
    }

    pub fn quorum_numbers(&self) -> &BlobQuorumNumbers {
        &self.quorum_numbers
    }

    pub fn quorum_signed_percentages(&self) -> &BlobQuorumSignedPercentages {
        &self.quorum_signed_percentages
    }

    pub fn reference_block_number(&self) -> u128 {
        self.reference_block_number
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BlobBatchRoot(String);

impl ToString for BlobBatchRoot {
    fn to_string(&self) -> String {
        self.0.clone()
    }
}
