use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use lasr_types::{Account, Certificate, Transaction, Outputs};
use crate::RpcRequestMethod;
use lasr_types::{Token, Address, U256};
use eigenda_client::batch::BatchHeaderHash;
use eigenda_client::proof::BlobVerificationProof;
use eigenda_client::response::BlobResponse;
use eo_listener::EventType;
use ethereum_types::H256;
use ractor::concurrency::OneshotSender;
use web3::ethabi::{FixedBytes, Address as EthereumAddress};
use web3::types::TransactionReceipt;
use ractor_cluster::RactorMessage;
use ractor::RpcReplyPort;
use derive_builder::Builder;

pub type ContractBlob = String;


/// An error type for RPC Responses
#[derive(thiserror::Error, Debug, Clone)]
pub struct RpcResponseError {
    pub description: String
}

/// Required trait to be considered an `Error` type
impl Display for RpcResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone)]
pub enum TransactionResponse {
    SendResponse(Token),
    CallResponse(Account),
    AsyncCallResponse(String),
    GetAccountResponse(Account),
    RegisterProgramResponse(Option<String>),
    TransactionError(RpcResponseError)
}

/// A message type that the RpcServer Actor can `handle`
#[derive(Debug, RactorMessage)]
pub enum RpcMessage {
    Request {
        method: RpcRequestMethod, 
        reply: RpcReplyPort<RpcMessage>
    },
    Response{
        response: Result<TransactionResponse, RpcResponseError>,
        reply: Option<RpcReplyPort<RpcMessage>>,
    },
}

/// Message types that the `Scheduler` actor can `handle`
#[derive(Debug, RactorMessage)]
pub enum SchedulerMessage {
    Call {
        transaction: Transaction,
        rpc_reply: RpcReplyPort<RpcMessage>
    },
    Send {
        transaction: Transaction,
        rpc_reply: RpcReplyPort<RpcMessage>
    },
    RegisterProgram {
        transaction: Transaction,
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
    },
    GetAccount {
        address: Address,
        rpc_reply: RpcReplyPort<RpcMessage>
    },
    TransactionApplied {
        transaction_hash: String,
        token: Token
    },
    SendTransactionFailure {
        transaction_hash: String,
        error: Box<dyn std::error::Error + Send>,
    },
    CallTransactionApplied {
        transaction_hash: String,
        account: Account, 
    },
    CallTransactionFailure {
        transaction_hash: String,
        outputs: String,
        error: String,
    },
    CallTransactionAsyncPending { 
        transaction_hash: String 
    },
    RegistrationSuccess {
        transaction: Transaction,
        program_id: Address
    },
    RegistrationFailure {
        transaction_hash: String,
        error_string: String,
    }
}

/// A message type that the `Validator` actor can handle
#[derive(Debug, Clone, RactorMessage)]
pub enum ValidatorMessage {
    PendingTransaction { transaction: Transaction },
    PendingCall { 
        outputs: Option<Outputs>,
        transaction: Transaction,
    },
    PendingRegistration {
        transaction: Transaction,
    }
}

/// A message type that the Engine can `handle`
#[derive(Debug, RactorMessage)]
pub enum EngineMessage {
    Call {
        transaction: Transaction,
    },
    Send {
        transaction: Transaction,
    },
    RegisterProgram {
        transaction: Transaction,
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
    CallSuccess {
        transaction: Transaction,
        transaction_hash: String,
        outputs: String,
    },
    RegistrationSuccess {
        transaction_hash: String,
    },
    CommTest
}

/// An event type that the Executable Oracle contract listener
/// listens for
#[derive(Builder, Clone, Debug)]
#[allow(unused)]
pub struct SettlementEvent {
    accounts: Vec<web3::ethabi::Token>,
    batch_header_hash: FixedBytes,
    blob_index: U256,
    settlement_event_id: U256,
}

/// An event type that the Executable Oracle contract listener
/// listens for 
#[derive(Builder, Clone, Debug)]
pub struct BridgeEvent {
    user: EthereumAddress,
    program_id: EthereumAddress,
    amount: U256,
    token_id: U256,
    token_type: String,
    bridge_event_id: U256,
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
    
    pub fn bridge_event_id(&self) -> U256 {
        self.bridge_event_id
    }
}

/// An Enum representing the two types of Executable Oracle events
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

#[derive(Clone, Debug)]
pub enum HashOrError {
    Hash(H256),
    Error(web3::Error),
}

/// A message type that the `EoServer` can `handle
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
        accounts: HashSet<String>,
        batch_header_hash: H256,
        blob_index: u128
    },
    GetAccountBlobIndex {
        address: Address,
        sender: OneshotSender<EoMessage>
    },
    GetAccountBalance {
        program_id: Address,
        address: Address,
        sender: OneshotSender<EoMessage>,
        token_type: u8,
    },
    GetContractBlobIndex {
        program_id: Address,
        sender: OneshotSender<EoMessage>
    },
    AccountBlobIndexAcquired {
        address: Address,
        batch_header_hash: H256, 
        blob_index: u128 
    },
    ContractBlobIndexAcquired {
        program_id: Address,
        batch_header_hash: H256,
        blob_index: u128 
    },
    AccountBalanceAcquired {
        program_id: Address,
        address: Address,
        balance: Option<U256>,
    },
    NftHoldingsAcquired {
        program_id: Address,
        address: Address,
        holdings: Option<Vec<U256>>
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
    SettleSuccess {
        batch_header_hash: H256,
        blob_index: u128,
        accounts: HashSet<String>,
        hash: HashOrError,
        receipt: Option<TransactionReceipt>,
    },
    SettleFailure {
        batch_header_hash: H256,
        blob_index: u128,
        accounts: HashSet<String>,
        hash: HashOrError,
        receipt: Option<TransactionReceipt>,
    },
    SettleTimedOut {
        batch_header_hash: H256,
        blob_index: u128,
        accounts: HashSet<String>,
        elapsed: tokio::time::error::Elapsed
    },
    CommTest
}

/// Message types that the `DaClient` can `handle
#[derive(Debug, RactorMessage)]
pub enum DaClientMessage {
    StoreBatch {
        batch: String,
        tx: OneshotSender<Result<BlobResponse, std::io::Error>>,
    },
    StoreTransactionBlob, 
    ValidateBlob {
        request_id: String,
        tx: OneshotSender<(String/*request_id*/, BlobVerificationProof)>
    },
    RetrieveAccount {
        address: Address,
        batch_header_hash: H256,
        blob_index: u128,
        tx: OneshotSender<Option<Account>>,
    },
    RetrieveTransaction {
        transaction_hash: [u8; 32],
        batch_header_hash: H256,
        blob_index: u128,
        tx: OneshotSender<Option<Transaction>>,
    },
    RetrieveContract {
        address: Address,
        batch_header_hash: H256,
        blob_index: u128,
        tx: OneshotSender<Option<ContractBlob>>,
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
    Update { account: Account },
    TryGetAccount { address: Address, reply: RpcReplyPort<RpcMessage> }
}

#[derive(Debug, RactorMessage)]
pub enum BlobCacheMessage {
    Cache { 
        blob_response: BlobResponse,
        accounts: HashSet<Address>,
        transactions: HashSet<Transaction>,
    },
    Get,
    Remove
}

#[derive(Debug, RactorMessage)]
pub enum PendingTransactionMessage {
    New {
        transaction: Transaction,
        outputs: Option<Outputs>,
    },
    NewCall {
        transaction: Transaction,
    },
    ExecSuccess {
        transaction: Transaction
    },
    Valid {
        transaction: Transaction,
        cert: Option<Certificate>
    },
    Invalid {
        transaction: Transaction,
        e: Box<dyn std::error::Error + Send>
    },
    Confirmed {
        map: HashMap<Address, Transaction>,
        batch_header_hash: BatchHeaderHash,
        blob_index: u128
    },
    GetPendingTransaction {
        transaction_hash: String,
        sender: OneshotSender<Option<Transaction>>
    },
    ValidCall {
        outputs: Outputs,
        transaction: Transaction,
        cert: Option<Certificate>
    }
}

#[derive(Debug, RactorMessage)]
pub enum BatcherMessage {
    AppendTransaction { 
        transaction: Transaction, 
        outputs: Option<Outputs>
    },
    GetNextBatch,
    BlobVerificationProof { request_id: String, proof: BlobVerificationProof }
}

#[derive(Debug, RactorMessage)]
pub enum ExecutorMessage {
    Retrieve {
        transaction: Transaction,
        content_id: String,
        program_id: Address,
    },
    Create {
        transaction: Transaction,
        content_id: String,
    },
    Start(String /*ContentId*/),
    Set { transaction: Transaction },
    Exec {
        transaction: Transaction,
    },
    Kill(String /*ContentId*/),
    Delete(String /*ContentId*/),
    Results {
        transaction: Option<Transaction>,
        content_id: String, 
        program_id: String,
        transaction_hash: Option<String>,
    },
    PollJobStatus { job_id: uuid::Uuid },
}
