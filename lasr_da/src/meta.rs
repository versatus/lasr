use serde::{Serialize, Deserialize};
use crate::batch::{BatchHeader, BatchHeaderHash};
use crate::record::BlobSignatoryRecordHash;
use crate::fee::BlobFee;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BatchMetadata {
    batch_header: BatchHeader,
    signatory_record_hash: BlobSignatoryRecordHash,
    fee: BlobFee,
    confirmation_block_number: u128,
    batch_header_hash: BatchHeaderHash 
}

impl BatchMetadata {
    pub fn batch_header(&self) -> &BatchHeader {
        &self.batch_header
    }

    pub fn signatory_record_hash(&self) -> &BlobSignatoryRecordHash {
        &self.signatory_record_hash
    }

    pub fn fee(&self) -> &BlobFee {
        &self.fee
    }

    pub fn confirmation_block_number(&self) -> u128 {
        self.confirmation_block_number
    }

    pub fn batch_header_hash(&self) -> &BatchHeaderHash {
        &self.batch_header_hash
    }
}

