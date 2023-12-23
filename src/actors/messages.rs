use std::fmt::Display;
use crate::{Account, ContractBlob};
use crate::actors::types::RpcRequestMethod;
use crate::account::{Token, Address, TokenDelta};
use crate::certificate::RecoverableSignature;
use eigenda_client::proof::BlobVerificationProof;
use eo_listener::EventType;
use ethereum_types::U256;
use ractor::concurrency::OneshotSender;
use web3::ethabi::{FixedBytes, Address as EthereumAddress};
use ractor_cluster::RactorMessage;
use ractor::{ActorRef, RpcReplyPort};
use super::types::ActorType;

/// An error type for RPC Responses
#[derive(thiserror::Error, Debug, Clone)]
pub struct RpcResponseError;

/// Required trait to be considered an `Error` type
impl Display for RpcResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// A message type that the RpcServer Actor can `handle`
///
/// Variants
///
///    Request {
///        method: RpcRequestMethod, 
///        reply: RpcReplyPort<RpcMessage>
///    },
///    Response{
///        response: Result<Token, RpcResponseError>,
///        reply: Option<RpcReplyPort<RpcMessage>>,
///    },
///    DeploySuccess {
///        reply: Option<RpcReplyPort<RpcMessage>> 
///    },
///
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

/// Message types that the `Scheduler` actor can `handle`
///
///
///    Call {
///        program_id: Address,
///        from: Address,
///        to: Vec<Address>,
///        op: String,
///        inputs: String,
///        sig: RecoverableSignature,
///        tx_hash: String,
///        rpc_reply: RpcReplyPort<RpcMessage>
///    },
///    Send {
///        program_id: Address,
///        from: Address,
///        to: Vec<Address>,
///        amount: U256,
///        content: Option<[u8; 32]>,
///        sig: RecoverableSignature,
///        tx_hash: String,
///        rpc_reply: RpcReplyPort<RpcMessage>
///    },
///    Deploy {
///        program_id: Address,
///        sig: RecoverableSignature,
///        tx_hash: String,
///        rpc_reply: RpcReplyPort<RpcMessage>
///    },
///    ValidatorComplete {
///        task_hash: String,
///        result: bool,
///    },
///    EngineComplete {
///        task_hash: String,
///    },
///    BlobRetrieved { 
///        address: Address,
///        blob: String
///    }, 
///    BlobIndexAcquired {
///        address: Address,
///        blob_index: String,
///        batch_header_hash: String,
///    },
///    EoEvent {
///        event: EoEvent
///    }
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
        blob_index: u128,
        batch_header_hash: String,
    },
    EoEvent {
        event: EoEvent
    }
}

/// A message type that the `Validator` actor can handle
///
/// Variants
///
///
///    Call { 
///        program_id: Address,
///        from: Address,
///        to: Vec<Address>,
///        op: String,
///        inputs: String,
///        tx_hash: String,
///        sig: RecoverableSignature,
///    }, Send {
///        program_id: Address,
///        from: Address,
///        to: Vec<Address>,
///        amount: U256,
///        content: Option<[u8; 32]>,
///        tx_hash: String,
///        sig: RecoverableSignature,
///    },
///    Deploy {
///        program_id: Address,
///        sig: RecoverableSignature,
///    },
///    EoEvent {
///        event: EoEvent
///    },
///    CommTest
///}
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

/// A message type that the Engine can `handle`
///
/// Variants
///
/// 
///    Call {
///        program_id: Address,
///        from: Address,
///        to: Vec<Address>,
///        op: String,
///        inputs: String,
///        sig: RecoverableSignature,
///        tx_hash: String
///    },
///    Send {
///        program_id: Address,
///        from: Address,
///        to: Vec<Address>,
///        amount: U256,
///        content: Option<[u8; 32]>,
///        sig: RecoverableSignature,
///    },
///    EoEvent {
///        event: EoEvent 
///    },
///    BlobIndexAcquired {
///        address: Address,
///        batch_header_hash: String,
///        blob_index: String,
///    },
///    Cache {
///        address: Address,
///        account: Account,
///    },
///    CheckCache {
///        address: Address,
///        reply: OneshotSender<Option<Account>>
///    },
///    CommTest
///
#[derive(Debug, RactorMessage)]
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
    BlobIndexAcquired {
        address: Address,
        batch_header_hash: String,
        blob_index: String,
    },
    Cache {
        address: Address,
        account: Account,
    },
    CheckCache {
        address: Address,
        reply: OneshotSender<Option<Account>>
    },
    CommTest
}

/// An event type that the Executable Oracle contract listener
/// listens for
#[derive(Builder, Clone, Debug)]
pub struct SettlementEvent {
    user: EthereumAddress,
    batch_header_hash: FixedBytes,
    blob_index: String
}

/// An event type that the Executable Oracle contract listener
/// listens for 
#[derive(Builder, Clone, Debug)]
pub struct BridgeEvent {
    user: EthereumAddress,
    program_id: EthereumAddress,
    amount: ethereum_types::U256,
    token_id: ethereum_types::U256,
    token_type: String,
}

impl BridgeEvent {
    /// A getter for the `user` field in a bridge event
    pub fn user(&self) -> EthereumAddress {
        self.user.clone()
    }

    /// A getter for the `program_id` field in a bridge event
    pub fn program_id(&self) -> EthereumAddress {
        self.program_id.clone()
    }

    /// A getter for the `amount` field in a bridge event
    pub fn amount(&self) -> U256 {
        self.amount
    }

    /// A getter for the `token_id` field in a bridge event
    pub fn token_id(&self) -> U256 {
        self.token_id
    }

    /// A getter for the `token_type` field in a bridge event
    pub fn token_type(&self) -> String {
        self.token_type.clone()
    }
}

/// An Enum representing the two types of Executable Oracle events
/// 
/// Variants
///
///    Bridge(Vec<BridgeEvent>),
///    Settlement(Vec<SettlementEvent>),
///    
/// Both take a `Vec` of the inner event, because this is how the `Log` 
/// gets returned
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

/// A message type that the `EoServer` can `handle
///
/// Variants
///
///    Log {
///        log: Vec<web3::ethabi::Log>,
///        log_type: EventType
///    },
///    Bridge {
///        program_id: Address,
///        address: Address,
///        amount: U256,
///        content: Option<[u8; 32]> 
///    },
///    Settle {
///        address: Address,
///        batch_header_hash: String,
///        blob_index: String
///    },
///    GetAccountBlobIndex {
///        address: Address,
///        sender: OneshotSender<EoMessage>
///    },
///    GetContractBlobIndex {
///        program_id: Address,
///        sender: OneshotSender<EoMessage>
///    },
///    AccountBlobIndexAcquired {
///        address: Address,
///        batch_header_hash: String,
///        blob_index: String
///    },
///    ContractBlobIndexAcquired {
///        program_id: Address,
///        batch_header_hash: String,
///        blob_index: String
///    },
///    AccountBlobIndexNotFound { 
///        address: Address 
///    },
///    ContractBlobIndexNotFound { 
///        program_id: Address 
///    },
///    CommTest
///
#[derive(Debug, RactorMessage)]
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
        blob_index: u128
    },
    GetAccountBlobIndex {
        address: Address,
        sender: OneshotSender<EoMessage>
    },
    GetContractBlobIndex {
        program_id: Address,
        sender: OneshotSender<EoMessage>
    },
    AccountBlobIndexAcquired {
        address: Address,
        batch_header_hash: String,
        blob_index: String
    },
    ContractBlobIndexAcquired {
        program_id: Address,
        batch_header_hash: String,
        blob_index: String
    },
    AccountBlobIndexNotFound { 
        address: Address 
    },
    ContractBlobIndexNotFound { 
        program_id: Address 
    },
    AccountCached {
        address: Address,
        removal_tx: OneshotSender<Address>
    },
    CommTest
}

/// Message types that the `DaClient` can `handle
///
/// Variants
///
///    StoreBlob {
///        blob: String
///    },
///    ValidateBlob {
///        request_id: String,
///    },
///    RetrieveBlob {
///        batch_header_hash: String,
///        blob_index: String
///    },
///    EoEvent {
///        event: EoEvent
///    },
///    CommTest
///
#[derive(Debug, RactorMessage)]
pub enum DaClientMessage {
    StoreAccountBlobs {
        accounts: Vec<Account> 
    },
    StoreContractBlobs {
        contracts: Vec<ContractBlob>
    },
    StoreTransactionBlob, 
    ValidateBlob {
        request_id: String,
        address: Address, 
        tx: OneshotSender<(Address, BlobVerificationProof)>
    },
    RetrieveBlob {
        batch_header_hash: String,
        blob_index: u128
    },
    EoEvent {
        event: EoEvent
    },
    CommTest
}

#[derive(Debug, RactorMessage)]
pub enum AccountCacheMessage {
    Write { account: Account },
    Read { address: Address, tx: OneshotSender<Option<Account>> },
    Remove { address: Address },
    Update { address: Address, delta: TokenDelta }
}
