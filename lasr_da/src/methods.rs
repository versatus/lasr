use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum EigenDaGrpcMethod {
    DisperseBlob,
    GetBlobStatus,
}

impl ToString for EigenDaGrpcMethod {
    fn to_string(&self) -> String {
        match self {
            EigenDaGrpcMethod::DisperseBlob => {
                return "disperser.Disperser/DisperseBlob".to_string()
            },
            EigenDaGrpcMethod::GetBlobStatus => {
                return "disperser.Disperser/GetBlobStatus".to_string()
            }
        }
    }
}

