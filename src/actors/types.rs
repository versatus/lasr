use crate::account::Address;
use crate::certificate::RecoverableSignature;
use ethereum_types::U256;
use tokio::time::Duration;
use serde::{Serialize, Deserialize};

pub const TIMEOUT_DURATION: Duration = tokio::time::Duration::from_millis(200);

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialOrd, Ord, PartialEq, Eq)] 
pub enum ActorType {
    RpcServer,
    Scheduler,
    Validator,
    Engine,
    EoServer,
    DaClient
}

#[derive(Debug, Clone)]
pub enum RpcRequestMethod {
    Call {
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        op: String,
        inputs: String,
        sig: RecoverableSignature,
        tx_hash: String,
    },
    Send {
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        amount: U256,
        sig: RecoverableSignature,
        tx_hash: String,
    },
    Deploy {
        program_id: Address,
        sig: RecoverableSignature,
        tx_hash: String
    }
}
