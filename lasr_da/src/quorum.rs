use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BlobQuorumIndexes(String);

impl ToString for BlobQuorumIndexes {
    fn to_string(&self) -> String {
       self.0.clone() 
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BlobQuorumParams {
    adversary_threshold_percentage: usize,
    quorum_threshold_percentage: usize,
    quantization_param: Option<usize>,
    encoded_length: Option<String>,
}

impl BlobQuorumParams {
    pub fn adversary_threshold_percentage(&self) -> usize {
        self.adversary_threshold_percentage
    }

    pub fn quorum_threshold_percentage(&self) -> usize {
        self.quorum_threshold_percentage
    }

    pub fn quantization_param(&self) -> Option<usize> {
        self.quantization_param.clone()
    }

    pub fn encoded_length(&self) -> Option<String> {
        self.encoded_length.clone()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BlobQuorumNumbers(String);

impl ToString for BlobQuorumNumbers {
    fn to_string(&self) -> String {
        self.0.clone()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BlobQuorumSignedPercentages(String);

impl ToString for BlobQuorumSignedPercentages {
    fn to_string(&self) -> String {
        self.0.clone()
    }
}

