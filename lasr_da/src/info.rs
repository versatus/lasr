use serde::{Serialize, Deserialize};
use crate::proof::BlobVerificationProof;
use crate::header::BlobHeader;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobInfo {
    blob_header: Option<BlobHeader>,
    blob_verification_proof: Option<BlobVerificationProof>,
}

impl BlobInfo {
    pub fn blob_header(&self) -> Option<&BlobHeader> {
        if let Some(header) = &self.blob_header {
            return Some(header)
        }

        None
    }

    pub fn blob_verification_proof(&self) -> Option<&BlobVerificationProof> {
        if let Some(proof) = &self.blob_verification_proof {
            return Some(proof)
        }

        None
    }
}

impl Default for BlobInfo {
    fn default() -> Self {
        BlobInfo {
            blob_header: None,
            blob_verification_proof: None 
        }
    }
}
