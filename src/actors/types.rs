use crate::account::Address;
use crate::certificate::RecoverableSignature;
use ethereum_types::U256;
use tokio::time::Duration;
use serde::{Serialize, Deserialize};

pub const TIMEOUT_DURATION: Duration = tokio::time::Duration::from_millis(200);

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialOrd, Ord, PartialEq, Eq)] 
pub enum ActorType {
    Registry,
    RpcServer,
    Scheduler,
    Validator,
    Engine,
    EoServer,
    DaClient,
    AccountCache,
    BlobCache,
    PendingTransactions,
}

impl ToString for ActorType {
    fn to_string(&self) -> String {
        match self {
            ActorType::Registry => "registry".to_string(),
            ActorType::RpcServer => "rpc_server".to_string(),
            ActorType::Scheduler => "scheduler".to_string(),
            ActorType::Validator => "validator".to_string(),
            ActorType::Engine => "engine".to_string(),
            ActorType::EoServer => "eo_server".to_string(),
            ActorType::DaClient => "da_client".to_string(),
            ActorType::AccountCache => "account_cache".to_string(),
            ActorType::BlobCache => "blob_cache".to_string(),
            ActorType::PendingTransactions => "pending_transactions".to_string()
        }
    }
}

#[derive(Debug, Clone)]
pub enum RpcRequestMethod {
    Call {
        program_id: Address,
        from: Address,
        to: Address,
        value: U256,
        op: String,
        inputs: String,
        sig: RecoverableSignature,
        nonce: U256, 
    },
    Send {
        program_id: Address,
        from: Address,
        to: Address,
        amount: U256,
        sig: RecoverableSignature,
        nonce: U256
    },
    Deploy {
        program_id: Address,
        sig: RecoverableSignature,
        nonce: U256
    }
}
