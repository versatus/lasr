use std::fmt::Display;

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

impl Display for ActorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorType::Registry => write!(f, "registry"),
            ActorType::RpcServer => write!(f, "rpc_server"),
            ActorType::Scheduler => write!(f, "scheduler"),
            ActorType::Validator => write!(f, "validator"),
            ActorType::Engine => write!(f, "engine"),
            ActorType::EoServer => write!(f, "eo_server"),
            ActorType::DaClient => write!(f, "da_client"),
            ActorType::AccountCache => write!(f, "account_cache"),
            ActorType::BlobCache => write!(f, "blob_cache"),
            ActorType::PendingTransactions => write!(f, "pending_transactions"),
            ActorType::EoClient => write!(f, "eo_client"),
            ActorType::Batcher => write!(f, "batcher"),
            ActorType::Executor => write!(f, "executor"),
            ActorType::RemoteExecutor => write!(f, "remote_executor"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialOrd, Ord, PartialEq, Eq)]
pub enum SupervisorType {
    BlobCache,
    AccountCache,
    PendingTransaction,
    LasrRpcServer,
    Scheduler,
    EoServer,
    Engine,
    Validator,
    EoClient,
    DaClient,
    Batcher,
    Executor,
}
impl Display for SupervisorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupervisorType::BlobCache => write!(f, "blob_cache_supervisor"),
            SupervisorType::AccountCache => write!(f, "account_cache_supervisor"),
            SupervisorType::PendingTransaction => write!(f, "pending_transaction_supervisor"),
            SupervisorType::LasrRpcServer => write!(f, "lasr_rpc_server_supervisor"),
            SupervisorType::Scheduler => write!(f, "scheduler_supervisor"),
            SupervisorType::EoServer => write!(f, "eo_server_supervisor"),
            SupervisorType::Engine => write!(f, "engine_supervisor"),
            SupervisorType::Validator => write!(f, "validator_supervisor"),
            SupervisorType::EoClient => write!(f, "eo_client_supervisor"),
            SupervisorType::DaClient => write!(f, "da_client_supervisor"),
            SupervisorType::Batcher => write!(f, "batcher_supervisor"),
            SupervisorType::Executor => write!(f, "executor_supervisor"),
        }
    }
}

pub trait ActorName {
    fn name(&self) -> ractor::ActorName;
}

pub trait ToActorType {
    fn to_actor_type(&self) -> ActorType;
}
impl ToActorType for ractor::ActorName {
    fn to_actor_type(&self) -> ActorType {
        match self.as_str() {
            "registry" => ActorType::Registry,
            "rpc_server" => ActorType::RpcServer,
            "scheduler" => ActorType::Scheduler,
            "validator" => ActorType::Validator,
            "engine" => ActorType::Engine,
            "eo_server" => ActorType::EoServer,
            "da_client" => ActorType::DaClient,
            "account_cache" => ActorType::AccountCache,
            "blob_cache" => ActorType::BlobCache,
            "pending_transactions" => ActorType::PendingTransactions,
            "eo_client" => ActorType::EoClient,
            "batcher" => ActorType::Batcher,
            "executor" => ActorType::Executor,
            "remote_executor" => ActorType::RemoteExecutor,
            _ => unreachable!("Actor name is not an ActorType variant"),
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
