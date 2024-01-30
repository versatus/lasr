use std::str::FromStr;

use serde::{Serialize, Deserialize};
use crate::info::BlobInfo;
use crate::quorum::{
    BlobQuorumParams,
    BlobQuorumIndexes,
    BlobQuorumNumbers,
    BlobQuorumSignedPercentages
};
use crate::batch::{BlobBatchRoot, BatchHeader, BatchHeaderHash};
use crate::header::BlobHeader;
use crate::proof::{BlobInclusionProof, BlobVerificationProof};
use crate::commitment::BlobCommitment;
use crate::meta::BatchMetadata;
use crate::record::BlobSignatoryRecordHash;
use crate::fee::BlobFee;

// TODO: Implement custom Deserialize
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "UPPERCASE")]
pub enum BlobResult {
    Processing,
    Finalized,
    Confirmed,
    Failed,
    Other(String)
}

impl Default for BlobResult {
    fn default() -> Self {
        BlobResult::Other("Default".to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlobStatus {
    status: BlobResult,
    info: Option<BlobInfo>
}

impl BlobStatus {
    pub fn status(&self) -> &BlobResult {
        &self.status
    }

    pub fn info(&self) -> &Option<BlobInfo> {
        &self.info
    }
    
    pub fn blob_header(&self) -> Option<&BlobHeader> {
        if let Some(info) = &self.info() {
            return info.blob_header()
        }
        None
    }

    pub fn blob_verification_proof(&self) -> Option<&BlobVerificationProof> {
        if let Some(info) = &self.info() {
            return info.blob_verification_proof()
        }

        None
    }

    pub fn commitment(&self) -> Option<&BlobCommitment> {
        if let Some(header) = &self.blob_header() {
            return Some(header.commitment())
        }

        None
    }

    pub fn data_length(&self) -> Option<usize> {
        if let Some(header) = &self.blob_header() {
            return Some(header.data_length())
        }

        None
    }

    pub fn blob_quorum_params(&self) -> Option<&Vec<BlobQuorumParams>> {
        if let Some(header) = &self.blob_header() {
            return Some(header.blob_quorum_params())
        }

        None
    }

    pub fn batch_id(&self) -> Option<u128> {
        if let Some(proof) = self.blob_verification_proof() {
            return Some(proof.batch_id())
        }
        None
    }

    pub fn blob_index(&self) -> Option<u128> {
        if let Some(proof) = self.blob_verification_proof() {
            return Some(proof.blob_index())
        }
        None
    }

    pub fn batch_metadata(&self) -> Option<&BatchMetadata> {
        if let Some(proof) = &self.blob_verification_proof() {
            return Some(proof.batch_metadata())
        }
        None
    }

    pub fn inclusion_proof(&self) -> Option<&BlobInclusionProof> {
        if let Some(proof) = &self.blob_verification_proof() {
            if let Some(p) = proof.inclusion_proof() {
                return Some(p)
            }
        }
        None
    }

    pub fn quorum_indexes(&self) -> Option<&BlobQuorumIndexes> {
        if let Some(proof) = &self.blob_verification_proof() {
            return Some(proof.quorum_indexes())
        }
        None
    }

    pub fn batch_header(&self) -> Option<&BatchHeader> {
        if let Some(metadata) = &self.batch_metadata() {
            return Some(metadata.batch_header())
        }
        None
    }

    pub fn signatory_record_hash(&self) -> Option<&BlobSignatoryRecordHash> {
        if let Some(metadata) = self.batch_metadata() {
            return Some(metadata.signatory_record_hash())
        }
        None
    }

    pub fn fee(&self) -> Option<&BlobFee> {
        if let Some(metadata) = &self.batch_metadata() {
            return Some(metadata.fee())
        }

        None
    }

    pub fn confirmation_block_number(&self) -> Option<u128> {
        if let Some(metadata) = self.batch_metadata() {
            return Some(metadata.confirmation_block_number())
        }

        None
    }

    pub fn batch_header_hash(&self) -> Option<&BatchHeaderHash> {
        if let Some(metadata) = &self.batch_metadata() {
            return Some(metadata.batch_header_hash())
        }

        None
    }
    
    pub fn batch_root(&self) -> Option<&BlobBatchRoot> {
        if let Some(header) = &self.batch_header() {
            return Some(header.batch_root())
        }

        None
    }

    pub fn quorum_numbers(&self) -> Option<&BlobQuorumNumbers> {
        if let Some(header) = &self.batch_header() {
            return Some(header.quorum_numbers())
        }
        None
    }

    pub fn quorum_signed_percentages(&self) -> Option<&BlobQuorumSignedPercentages> {
        if let Some(header) = &self.batch_header() {
            return Some(header.quorum_signed_percentages())
        }

        None
    }

    pub fn reference_block_number(&self) -> Option<u128> {
        if let Some(header) = self.batch_header() {
            return Some(header.reference_block_number())
        }
        None
    }
}

impl From<String> for BlobStatus {
    fn from(value: String) -> Self {
        if let Some(start_index) = value.find('{') {
            let json_str = &value[start_index..];
            let blob_status: Result<BlobStatus, serde_json::Error> = serde_json::from_str(json_str);
            match blob_status {
                Ok(status) => {
                    return status
                }
                Err(_) => {
                    return BlobStatus::default()
                }
            }
        } else {
            return BlobStatus::default()
        }
    } }

impl FromStr for BlobStatus {
    type Err = serde_json::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

impl Default for BlobStatus {
    fn default() -> Self {
        BlobStatus {
            status: Default::default(),
            info: Default::default()
        }
    }
}
