#![allow(unused)]
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use eigenda_client::batch::BatchHeaderHash;
use futures::stream::{FuturesUnordered, Stream, StreamExt};
use futures::{Future, FutureExt};
use hex;
use ractor::concurrency::OneshotSender;
use ractor::{Actor, ActorCell, ActorProcessingErr, ActorRef, SupervisionEvent};
use secp256k1::SecretKey;
use sha3::{Digest, Keccak256};
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use tokio::sync::{mpsc::Receiver, Mutex};
use web3::contract::{Contract, Options};
use web3::ethabi::Token as EthAbiToken;
use web3::helpers::CallFuture;
use web3::transports::Http;
use web3::types::{Address as EthereumAddress, TransactionId, TransactionReceipt, H256};
use web3::Web3;

use crate::{ActorExt, Coerce, EoServerError, StaticFuture, UnorderedFuturePool};
use lasr_messages::{ActorName, ActorType, EoMessage, HashOrError, SupervisorType};
use lasr_types::{Address, U256};

#[derive(Clone, Debug)]
pub struct EoClientActor {
    future_pool: UnorderedFuturePool<StaticFuture<()>>,
}
impl ActorName for EoClientActor {
    fn name(&self) -> ractor::ActorName {
        ActorType::EoClient.to_string()
    }
}

#[derive(Debug, Error)]
pub enum EoClientError {
    #[error("failed to acquire EoClientActor from registry")]
    RactorRegistryError,

    #[error("{0}")]
    Custom(String),
}
impl Default for EoClientError {
    fn default() -> Self {
        EoClientError::RactorRegistryError
    }
}

impl EoClientActor {
    pub fn new() -> Self {
        Self {
            future_pool: Arc::new(Mutex::new(FuturesUnordered::new())),
        }
    }

    async fn get_account_blob_index(
        eo_client: Arc<Mutex<EoClient>>,
        address: Address,
        sender: OneshotSender<EoMessage>,
    ) {
        let blob = {
            let state = eo_client.lock().await;
            state.get_blob_index(address.into()).await
        };
        if let Some((batch_header_hash, blob_index)) = blob {
            if !batch_header_hash.is_zero() {
                let res = sender.send(EoMessage::AccountBlobIndexAcquired {
                    address,
                    batch_header_hash,
                    blob_index,
                });

                if let Err(e) = res {
                    log::error!("{:?}", e);
                }
            }
        } else {
            log::info!("unable to find blob index for address: 0x{:x}", address);
            let _res = sender.send(EoMessage::AccountBlobIndexNotFound { address });
        }
    }

    async fn get_account_balance(
        eo_client: Arc<Mutex<EoClient>>,
        program_id: Address,
        address: Address,
        sender: OneshotSender<EoMessage>,
        token_type: u8,
    ) {
        if token_type == 0 {
            let balance = {
                let state = eo_client.lock().await;
                state.get_eth_balance(address.into()).await
            };
            let balance = balance.map(|b| b.into());
            let res = sender.send(EoMessage::AccountBalanceAcquired {
                program_id,
                address,
                balance,
            });
            if let Err(e) = res {
                log::error!("{:?}", e);
            }
        } else if token_type == 1 {
            let balance = {
                let state = eo_client.lock().await;
                state
                    .get_erc20_balance(program_id.into(), address.into())
                    .await
            };
            let balance = balance.map(|b| b.into());
            let res = sender.send(EoMessage::AccountBalanceAcquired {
                program_id,
                address,
                balance,
            });
            if let Err(e) = res {
                log::error!("{:?}", e);
            }
        } else if token_type == 2 {
            let holdings = {
                let state = eo_client.lock().await;
                state
                    .get_erc721_holdings(program_id.into(), address.into())
                    .await
            };
            let holdings: Option<Vec<U256>> = holdings.map(|h| {
                h.iter()
                    .map(|eth_u256| {
                        let internal_u256: U256 = eth_u256.into();
                        internal_u256
                    })
                    .collect()
            });
            let res = sender.send(EoMessage::NftHoldingsAcquired {
                program_id,
                address,
                holdings,
            });
            if let Err(e) = res {
                log::error!("{:?}", e);
            }
        }
    }

    async fn settle(
        eo_client: Arc<Mutex<EoClient>>,
        accounts: HashSet<String>,
        batch_header_hash: H256,
        blob_index: u128,
    ) {
        let mut state = eo_client.lock().await;
        let res = state
            .settle_batch(accounts, batch_header_hash, blob_index)
            .await;
        if let Ok(handle) = res {
            state.insert_pending((batch_header_hash, blob_index), handle);
        } else {
            log::error!("encountered error attempting to settle batch");
        }
    }

    async fn settle_success(
        eo_client: Arc<Mutex<EoClient>>,
        batch_header_hash: H256,
        blob_index: u128,
        accounts: HashSet<String>,
        hash: HashOrError,
        receipt: Option<TransactionReceipt>,
    ) {
        let handle_opt = {
            let mut state = eo_client.lock().await;
            state.remove_pending((batch_header_hash, blob_index))
        };
        if let Some(handle) = handle_opt {
            if let Err(e) = handle.await {
                log::error!("JoinHandle after SettleSuccess message returned an error: {e:?}");
            }
        }
    }
}

pub struct EoClient {
    contract: Contract<Http>,
    web3: Web3<Http>,
    address: EthereumAddress,
    //TODO: read this in from dotenv when signing
    sk: web3::signing::SecretKey,
    pending: HashMap<(H256, u128), tokio::task::JoinHandle<()>>,
}

impl EoClient {
    pub async fn new(
        web3: Web3<Http>,
        contract: Contract<Http>,
        address: Address,
        sk: web3::signing::SecretKey,
    ) -> web3::Result<Self> {
        let address = EoClient::parse_checksum_address(address)?;

        Ok(EoClient {
            contract,
            web3,
            address,
            sk,
            pending: HashMap::new(),
        })
    }

    pub(super) fn insert_pending(
        &mut self,
        blob_info: (H256, u128),
        handle: tokio::task::JoinHandle<()>,
    ) -> Result<(), EoServerError> {
        let _ = self
            .pending
            .insert(blob_info, handle)
            .ok_or(EoServerError::Custom(
                "eo_client.rs: 58: error attempting to insert blob info handle".to_string(),
            ));
        Ok(())
    }

    pub(super) fn remove_pending(
        &mut self,
        blob_info: (H256, u128),
    ) -> Option<tokio::task::JoinHandle<()>> {
        self.pending.remove(&blob_info)
    }

    fn parse_checksum_address(address: Address) -> Result<EthereumAddress, web3::Error> {
        let bytes = address.as_ref();
        let checksum = EoClient::to_checksum_address(bytes);
        checksum.parse().map_err(|_| web3::Error::Internal)
    }

    async fn get_blob_index(&self, address: EthereumAddress) -> Option<(H256, u128)> {
        self.contract
            .query("getBlobIndex", (address,), None, Options::default(), None)
            .await
            .ok()
    }

    async fn get_eth_balance(&self, address: EthereumAddress) -> Option<ethereum_types::U256> {
        self.contract
            .query("getEthBalance", (address,), None, Options::default(), None)
            .await
            .ok()
    }

    async fn get_erc20_balance(
        &self,
        program_id: EthereumAddress,
        address: EthereumAddress,
    ) -> Option<ethereum_types::U256> {
        self.contract
            .query(
                "getERC20Balance",
                (program_id, address),
                None,
                Options::default(),
                None,
            )
            .await
            .ok()
    }

    async fn get_erc721_holdings(
        &self,
        program_id: EthereumAddress,
        address: EthereumAddress,
    ) -> Option<Vec<ethereum_types::U256>> {
        self.contract
            .query(
                "getERC721Holdings",
                (program_id, address),
                None,
                Options::default(),
                None,
            )
            .await
            .ok()
    }

    fn to_checksum_address(bytes: &[u8]) -> String {
        let hex_address = hex::encode(bytes).to_lowercase();
        let mut hasher = Keccak256::new();
        hasher.update(hex_address.as_bytes());
        let hash = hasher.finalize();
        let checksum_address: String = hex_address
            .char_indices()
            .map(|(i, c)| {
                if c.is_ascii_digit() {
                    c.to_string()
                } else if hash[i / 2] >> 4 >= 8 && i % 2 == 0
                    || hash[i / 2] & 0x0f >= 8 && i % 2 != 0
                {
                    c.to_uppercase().to_string()
                } else {
                    c.to_lowercase().to_string()
                }
            })
            .collect();

        format!("0x{}", checksum_address)
    }

    pub(super) async fn settle_batch(
        &mut self,
        accounts: HashSet<String>,
        batch_header_hash: H256,
        blob_index: u128,
    ) -> Result<tokio::task::JoinHandle<()>, Box<dyn std::error::Error>> {
        let eo_client: ActorRef<EoMessage> =
            ractor::registry::where_is(ActorType::EoClient.to_string())
                .ok_or(Box::new(EoServerError::Custom(
                    "eo_client.rs: 161: unable to acquire eo_client in settle_batch function"
                        .to_string(),
                )) as Box<dyn std::error::Error>)?
                .into();

        let mut sk = self.sk;
        let mut contract = self.contract.clone();
        let mut web3 = self.web3.clone();

        Ok(tokio::task::spawn(async move {
            log::info!("attempting to settle batch");
            let eth_addresses: Vec<EthereumAddress> = accounts
                .clone()
                .iter()
                .filter_map(|a| {
                    if let Ok(addr) = Address::from_hex(a) {
                        EoClient::parse_checksum_address(addr).ok()
                    } else {
                        None
                    }
                })
                .collect();

            log::info!("parsed addresses for batch settlement");
            let addresses: Vec<EthAbiToken> = eth_addresses
                .iter()
                .map(|a| EthAbiToken::Address(*a))
                .collect();

            let n_accounts = addresses.len();

            let eth_accounts = EthAbiToken::Array(addresses.clone());

            let blob_info = EthAbiToken::Tuple(vec![
                EthAbiToken::FixedBytes(batch_header_hash.0.to_vec()),
                EthAbiToken::Uint(blob_index.into()),
            ]);

            let gas = ethereum_types::U256::from(50000 * n_accounts)
                + ethereum_types::U256::from(100_000);
            log::info!("providing {} gas for transaction", gas);
            let mut options = Options {
                gas: Some(gas),
                ..Options::default()
            };

            log::info!("sending transaction");
            let res = tokio::time::timeout(
                tokio::time::Duration::new(15, 0),
                contract.signed_call(
                    "settleBlobIndex",
                    (eth_accounts, blob_info),
                    options,
                    &mut sk,
                ),
            )
            .await;

            match res {
                Ok(Ok(hash)) => {
                    log::info!("received transaction hash for settlement transaction");
                    log::info!(
                        "for batch_header_hash: {:?}, blob_index: {:?}",
                        &batch_header_hash,
                        &blob_index
                    );
                    log::info!("transaction_hash: {:?}", &hash);
                    let mut receipt = None;

                    while receipt.is_none() {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        receipt = web3
                            .eth()
                            .transaction_receipt(hash)
                            .await
                            .unwrap_or_else(|e| {
                                log::error!("{}", e);
                                None
                            });
                    }

                    let message =
                        if receipt.clone().unwrap().status == Some(web3::types::U64::from(1)) {
                            EoMessage::SettleSuccess {
                                batch_header_hash,
                                blob_index,
                                accounts,
                                hash: HashOrError::Hash(hash),
                                receipt,
                            }
                        } else {
                            EoMessage::SettleFailure {
                                batch_header_hash,
                                blob_index,
                                accounts,
                                hash: HashOrError::Hash(hash),
                                receipt,
                            }
                        };

                    let res = eo_client.cast(message);
                    if let Err(e) = res {
                        log::error!("{}", e)
                    }
                }
                Ok(Err(e)) => {
                    log::info!("encountered error attempting to settle batch: {}", e);
                    let message = EoMessage::SettleFailure {
                        batch_header_hash,
                        blob_index,
                        accounts,
                        hash: HashOrError::Error(e),
                        receipt: None,
                    };
                    let res = eo_client.cast(message);
                    if let Err(e) = res {
                        log::error!("{}", e)
                    }
                }
                Err(timeout) => {
                    log::info!("settlement transaction timed out, try again: {}", timeout);
                    let message = EoMessage::SettleTimedOut {
                        batch_header_hash,
                        blob_index,
                        accounts,
                        elapsed: timeout,
                    };
                    let res = eo_client.cast(message);
                    if let Err(e) = res {
                        log::error!("{}", e)
                    }
                }
            }
        }))
    }
}

#[async_trait]
impl Actor for EoClientActor {
    type Msg = EoMessage;
    type State = Arc<Mutex<EoClient>>;
    type Arguments = Self::State;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::Arguments, ActorProcessingErr> {
        Ok(args)
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let eo_client_ptr = Arc::clone(state);
        match message {
            EoMessage::GetAccountBlobIndex { address, sender } => {
                let fut = EoClientActor::get_account_blob_index(eo_client_ptr, address, sender);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            EoMessage::GetContractBlobIndex {
                program_id: _,
                sender: _,
            } => {
                log::info!("Received request for contract blob index");
            }
            EoMessage::GetAccountBalance {
                program_id,
                address,
                sender,
                token_type,
            } => {
                let fut = EoClientActor::get_account_balance(
                    eo_client_ptr,
                    program_id,
                    address,
                    sender,
                    token_type,
                );
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            EoMessage::Settle {
                accounts,
                batch_header_hash,
                blob_index,
            } => {
                let fut =
                    EoClientActor::settle(eo_client_ptr, accounts, batch_header_hash, blob_index);
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            EoMessage::SettleSuccess {
                batch_header_hash,
                blob_index,
                accounts,
                hash,
                receipt,
            } => {
                let fut = EoClientActor::settle_success(
                    eo_client_ptr,
                    batch_header_hash,
                    blob_index,
                    accounts,
                    hash,
                    receipt,
                );
                let guard = self.future_pool.lock().await;
                guard.push(fut.boxed());
            }
            EoMessage::SettleFailure {
                batch_header_hash,
                blob_index,
                accounts,
                hash,
                receipt,
            } => {
                log::error!(
                    "settlement of batch_header_hash: {:?}, blob_index: {:?} failed",
                    &batch_header_hash,
                    &blob_index
                );
                log::error!("transaction receipt: {:?}", receipt);
            }
            EoMessage::SettleTimedOut {
                batch_header_hash,
                blob_index,
                accounts,
                elapsed,
            } => {
                log::error!(
                    "settlement of batch_header_hash: {:?}, blob_index: {:?} timed out",
                    &batch_header_hash,
                    &blob_index
                );
                log::error!("time elapsed: {:?}", elapsed);
                let message = EoMessage::Settle {
                    accounts,
                    batch_header_hash,
                    blob_index,
                };
                let res = myself.cast(message);
                if let Err(e) = res {
                    log::error!("{e:?}");
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl ActorExt for EoClientActor {
    type Output = ();
    type Future<O> = StaticFuture<Self::Output>;
    type FuturePool<F> = UnorderedFuturePool<Self::Future<Self::Output>>;
    type FutureHandler = tokio_rayon::rayon::ThreadPool;
    type JoinHandle = tokio::task::JoinHandle<()>;

    fn future_pool(&self) -> Self::FuturePool<Self::Future<Self::Output>> {
        self.future_pool.clone()
    }

    fn spawn_future_handler(actor: Self, future_handler: Self::FutureHandler) -> Self::JoinHandle {
        tokio::spawn(async move {
            loop {
                let futures = actor.future_pool();
                let mut guard = futures.lock().await;
                future_handler
                    .install(|| async move { guard.next().await })
                    .await;
            }
        })
    }
}

pub struct EoClientSupervisor {
    panic_tx: Sender<ActorCell>,
}
impl EoClientSupervisor {
    pub fn new(panic_tx: Sender<ActorCell>) -> Self {
        Self { panic_tx }
    }
}
impl ActorName for EoClientSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::EoClient.to_string()
    }
}
#[derive(Debug, Error, Default)]
pub enum EoClientSupervisorError {
    #[default]
    #[error("failed to acquire EoClientSupervisor from registry")]
    RactorRegistryError,
}

#[async_trait]
impl Actor for EoClientSupervisor {
    type Msg = EoMessage;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: SupervisionEvent,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        log::warn!("Received a supervision event: {:?}", message);
        match message {
            SupervisionEvent::ActorStarted(actor) => {
                log::info!(
                    "actor started: {:?}, status: {:?}",
                    actor.get_name(),
                    actor.get_status()
                );
            }
            SupervisionEvent::ActorPanicked(who, reason) => {
                log::error!("actor panicked: {:?}, err: {:?}", who.get_name(), reason);
                self.panic_tx.send(who).await.typecast().log_err(|e| e);
            }
            SupervisionEvent::ActorTerminated(who, _, reason) => {
                log::error!("actor terminated: {:?}, err: {:?}", who.get_name(), reason);
            }
            SupervisionEvent::PidLifecycleEvent(event) => {
                log::info!("pid lifecycle event: {:?}", event);
            }
            SupervisionEvent::ProcessGroupChanged(m) => {
                log::warn!("process group changed: {:?}", m.get_group());
            }
        }
        Ok(())
    }
}
