use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BlobSignatoryRecordHash(String);

impl ToString for BlobSignatoryRecordHash {
    fn to_string(&self) -> String {
        self.0.clone()
    }
}

