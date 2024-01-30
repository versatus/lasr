use crate::status::BlobResult;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct BlobResponse {
    result: BlobResult,
    #[serde(rename = "requestId")]
    request_id: String
}

impl Default for BlobResponse {
    fn default() -> Self {
        BlobResponse { result: Default::default(), request_id: Default::default() }
    }
}

impl BlobResponse {
    pub fn result(&self) -> &BlobResult {
        &self.result
    }

    pub fn request_id(&self) -> String {
        self.request_id.clone()
    }
}

impl From<String> for BlobResponse {
    fn from(value: String) -> Self {
        if let Some(start_index) = value.find('{') {
            let json_str = &value[start_index..];

            let blob_response: Result<BlobResponse, serde_json::Error> = serde_json::from_str(json_str);

            match blob_response {
                Ok(response) => {
                    return response
                }
                Err(err) => {
                    log::error!("{}", &err);
                    return BlobResponse::default() 
                }
            }
        } else {
            return BlobResponse::default()
        }
    }
}
