use lasr_types::{Address, Transaction};

use serde::{Deserialize, Serialize};
use tokio::time::Duration;

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
    EoClient,
    Batcher,
    Executor,
    RemoteExecutor,
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
            ActorType::PendingTransactions => "pending_transactions".to_string(),
            ActorType::EoClient => "eo_client".to_string(),
            ActorType::Batcher => "batcher".to_string(),
            ActorType::Executor => "executor".to_string(),
            ActorType::RemoteExecutor => "remote_executor".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RpcRequestMethod {
    Call { transaction: Transaction },
    Send { transaction: Transaction },
    RegisterProgram { transaction: Transaction },
    GetAccount { address: Address },
}
