use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BlobCommitment(String);

impl ToString for BlobCommitment {
    fn to_string(&self) -> String {
        self.0.clone()
    }
}

