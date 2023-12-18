use std::fmt::Display;

use crate::actors::types::RpcRequestMethod;
use crate::account::{Token, Address};
use crate::certificate::RecoverableSignature;
use eo_listener::EventType;
use ethereum_types::U256;
use web3::ethabi::{FixedBytes, Address as EthereumAddress};
use ractor_cluster::RactorMessage;
use ractor::{ActorRef, RpcReplyPort};
use super::types::ActorType;

#[derive(Debug, RactorMessage)]
pub enum RegistryResponse {
    Scheduler(Option<ActorRef<SchedulerMessage>>),
    RpcServer(Option<ActorRef<RpcMessage>>),
    EoServer(Option<ActorRef<EoMessage>>),
    DaClient(Option<ActorRef<DaClientMessage>>),
    Engine(Option<ActorRef<EngineMessage>>),
    Validator(Option<ActorRef<ValidatorMessage>>)
}

#[derive(Debug, RactorMessage)]
pub enum RegistryActor {
    Scheduler(ActorRef<SchedulerMessage>),
    RpcServer(ActorRef<RpcMessage>),
    EoServer(ActorRef<EoMessage>),
    DaClient(ActorRef<DaClientMessage>),
    Engine(ActorRef<EngineMessage>),
    Validator(ActorRef<ValidatorMessage>)
}

impl From<RegistryActor> for RegistryResponse {
    fn from(value: RegistryActor) -> Self {
        match value {
            RegistryActor::Scheduler(actor_ref) => {
                RegistryResponse::Scheduler(
                    Some(actor_ref)
                )
            }
            RegistryActor::RpcServer(actor_ref) => {
                RegistryResponse::RpcServer(
                    Some(actor_ref)
                )
            }
            RegistryActor::EoServer(actor_ref) => {
                RegistryResponse::EoServer(
                    Some(actor_ref)
                )
            }
            RegistryActor::Engine(actor_ref) => {
                RegistryResponse::Engine(
                    Some(actor_ref)
                )
            }
            RegistryActor::Validator(actor_ref) => {
                RegistryResponse::Validator(
                    Some(actor_ref)
                )
            }
            RegistryActor::DaClient(actor_ref) => {
                RegistryResponse::DaClient(
                    Some(actor_ref)
                )
            }
        }
    }
}


impl From<&RegistryActor> for RegistryResponse {
    fn from(value: &RegistryActor) -> Self {
        match value {
            RegistryActor::Scheduler(actor_ref) => {
                RegistryResponse::Scheduler(
                    Some(actor_ref.clone())
                )
            }
            RegistryActor::RpcServer(actor_ref) => {
                RegistryResponse::RpcServer(
                    Some(actor_ref.clone())
                )
            }
            RegistryActor::EoServer(actor_ref) => {
                RegistryResponse::EoServer(
                    Some(actor_ref.clone())
                )
            }
            RegistryActor::Engine(actor_ref) => {
                RegistryResponse::Engine(
                    Some(actor_ref.clone())
                )
            }
            RegistryActor::Validator(actor_ref) => {
                RegistryResponse::Validator(
                    Some(actor_ref.clone())
                )
            }
            RegistryActor::DaClient(actor_ref) => {
                RegistryResponse::DaClient(
                    Some(actor_ref.clone())
                )
            }
        }
    }
}

#[derive(Debug, RactorMessage)]
pub enum RegistryMessage {
    Register(ActorType, RegistryActor),
    GetActor(ActorType, RpcReplyPort<RegistryResponse>),
}

#[derive(thiserror::Error, Debug, Clone)]
pub struct RpcResponseError;

impl Display for RpcResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, RactorMessage)]
pub enum RpcMessage {
    Request {
        method: RpcRequestMethod, 
        reply: RpcReplyPort<RpcMessage>
    },
    Response{
        response: Result<Token, RpcResponseError>,
        reply: Option<RpcReplyPort<RpcMessage>>,
    },
    DeploySuccess {
        reply: Option<RpcReplyPort<RpcMessage>> 
    },
}

#[derive(Debug, RactorMessage)]
pub enum SchedulerMessage {
    Call {
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        op: String,
        inputs: String,
        sig: RecoverableSignature,
        tx_hash: String,
        rpc_reply: RpcReplyPort<RpcMessage>
    },
    Send {
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        amount: U256,
        content: Option<[u8; 32]>,
        sig: RecoverableSignature,
        tx_hash: String,
        rpc_reply: RpcReplyPort<RpcMessage>
    },
    Deploy {
        program_id: Address,
        sig: RecoverableSignature,
        tx_hash: String,
        rpc_reply: RpcReplyPort<RpcMessage>
    },
    ValidatorComplete {
        task_hash: String,
        result: bool,
    },
    EngineComplete {
        task_hash: String,
    },
    BlobRetrieved { 
        address: Address,
        blob: String
    }, 
    BlobIndexAcquired {
        address: Address,
        blob_index: String,
        batch_header_hash: String,
    },
    EoEvent {
        event: EoEvent
    }
}

#[derive(Debug, Clone, RactorMessage)]
pub enum ValidatorMessage {
    Call { 
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        op: String,
        inputs: String,
        tx_hash: String,
        sig: RecoverableSignature,
    },
    Send {
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        amount: U256,
        content: Option<[u8; 32]>,
        tx_hash: String,
        sig: RecoverableSignature,
    },
    Deploy {
        program_id: Address,
        sig: RecoverableSignature,
    },
    EoEvent {
        event: EoEvent
    },
    CommTest
}

#[derive(Debug, Clone, RactorMessage)]
pub enum EngineMessage {
    Call {
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        op: String,
        inputs: String,
        sig: RecoverableSignature,
        tx_hash: String
    },
    Send {
        program_id: Address,
        from: Address,
        to: Vec<Address>,
        amount: U256,
        content: Option<[u8; 32]>,
        sig: RecoverableSignature,
    },
    EoEvent {
        event: EoEvent 
    },
    CommTest
}

#[derive(Builder, Clone, Debug)]
pub struct SettlementEvent {
    user: EthereumAddress,
    batch_header_hash: FixedBytes,
    blob_index: String
}

#[derive(Builder, Clone, Debug)]
pub struct BridgeEvent {
    user: EthereumAddress,
    program_id: EthereumAddress,
    amount: ethereum_types::U256,
    token_id: ethereum_types::U256,
    token_type: String,
}

#[derive(Clone, Debug)]
pub enum EoEvent {
    Bridge(Vec<BridgeEvent>),
    Settlement(Vec<SettlementEvent>),
}

impl Into<EoEvent> for Vec<SettlementEvent> {
    fn into(self) -> EoEvent {
        EoEvent::Settlement(self)
    }
}

impl Into<EoEvent> for Vec<BridgeEvent> {
    fn into(self) -> EoEvent {
        EoEvent::Bridge(self)
    }
}



#[derive(Debug, Clone, RactorMessage)]
pub enum EoMessage {
    Log {
        log: Vec<web3::ethabi::Log>,
        log_type: EventType
    },
    Bridge {
        program_id: Address,
        address: Address,
        amount: U256,
        content: Option<[u8; 32]> 
    },
    Settle {
        address: Address,
        batch_header_hash: String,
        blob_index: String
    },
    GetAccountBlobIndex {
        address: Address,
    },
    GetContractBlobIndex {
        program_id: Address,
    },
    CommTest
}

#[derive(Debug, Clone, RactorMessage)]
pub enum DaClientMessage {
    StoreBlob {
        blob: String
    },
    ValidateBlob {
        request_id: String,
    },
    RetrieveBlob {
        batch_header_hash: String,
        blob_index: String
    },
    EoEvent {
        event: EoEvent
    },
    CommTest
}
