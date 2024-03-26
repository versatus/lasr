#![allow(unused)]
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use async_trait::async_trait;
use eigenda_client::batch::BatchHeaderHash;
use futures::stream::{FuturesUnordered, Stream, StreamExt};
use futures::Future;
use hex;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use secp256k1::SecretKey;
use sha3::{Digest, Keccak256};
use tokio::sync::mpsc::Receiver;
use web3::contract::{Contract, Options};
use web3::ethabi::Token as EthAbiToken;
use web3::helpers::CallFuture;
use web3::transports::Http;
use web3::types::{Address as EthereumAddress, TransactionId, TransactionReceipt, H256};
use web3::Web3;

use lasr_actors::EoServerError;
use lasr_messages::{ActorType, EoMessage, HashOrError};
use lasr_types::{Address, U256};

#[derive(Clone, Debug)]
pub struct EoClientActor;

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
                if c >= '0' && c <= '9' {
                    c.to_string()
                } else {
                    if hash[i / 2] >> 4 >= 8 && i % 2 == 0 || hash[i / 2] & 0x0f >= 8 && i % 2 != 0
                    {
                        c.to_uppercase().to_string()
                    } else {
                        c.to_lowercase().to_string()
                    }
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

        let mut sk = self.sk.clone();
        let mut contract = self.contract.clone();
        let mut web3 = self.web3.clone();

        Ok(tokio::task::spawn(async move {
            log::info!("attempting to settle batch");
            let eth_addresses: Vec<EthereumAddress> = accounts
                .clone()
                .iter()
                .filter_map(|a| {
                    if let Some(addr) = Address::from_hex(a).ok() {
                        EoClient::parse_checksum_address(addr.clone()).ok()
                    } else {
                        None
                    }
                })
                .collect();

            log::info!("parsed addresses for batch settlement");
            let addresses: Vec<EthAbiToken> = eth_addresses
                .iter()
                .map(|a| EthAbiToken::Address(a.clone()))
                .collect();

            let n_accounts = addresses.len();

            let eth_accounts = EthAbiToken::Array(addresses.clone());

            let blob_info = EthAbiToken::Tuple(vec![
                EthAbiToken::FixedBytes(batch_header_hash.clone().0.to_vec()),
                EthAbiToken::Uint(blob_index.clone().into()),
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
                            .transaction_receipt(hash.clone())
                            .await
                            .unwrap_or_else(|e| {
                                log::error!("{}", e);
                                None
                            });
                    }

                    let message =
                        if receipt.clone().unwrap().status == Some(web3::types::U64::from(1)) {
                            let message = EoMessage::SettleSuccess {
                                batch_header_hash,
                                blob_index,
                                accounts,
                                hash: HashOrError::Hash(hash),
                                receipt,
                            };

                            message
                        } else {
                            let message = EoMessage::SettleFailure {
                                batch_header_hash,
                                blob_index,
                                accounts,
                                hash: HashOrError::Hash(hash),
                                receipt,
                            };
                            message
                        };

                    let res = eo_client.cast(message);
                    match res {
                        Err(e) => log::error!("{}", e),
                        _ => {}
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
                    match res {
                        Err(e) => log::error!("{}", e),
                        _ => {}
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
                    match res {
                        Err(e) => log::error!("{}", e),
                        _ => {}
                    }
                }
            }
        }))
    }
}

#[async_trait]
impl Actor for EoClientActor {
    type Msg = EoMessage;
    type State = EoClient;
    type Arguments = EoClient;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: EoClient,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(args)
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            EoMessage::GetAccountBlobIndex { address, sender } => {
                let blob = state.get_blob_index(address.into()).await;
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
                if token_type == 0 {
                    let balance = state.get_eth_balance(address.into()).await;
                    let balance = if let Some(b) = balance {
                        Some(b.into())
                    } else {
                        None
                    };
                    let res = sender.send(EoMessage::AccountBalanceAcquired {
                        program_id,
                        address,
                        balance,
                    });
                    if let Err(e) = res {
                        log::error!("{:?}", e);
                    }
                } else if token_type == 1 {
                    let balance = state
                        .get_erc20_balance(program_id.into(), address.into())
                        .await;
                    let balance = if let Some(b) = balance {
                        Some(b.into())
                    } else {
                        None
                    };
                    let res = sender.send(EoMessage::AccountBalanceAcquired {
                        program_id,
                        address,
                        balance,
                    });
                    if let Err(e) = res {
                        log::error!("{:?}", e);
                    }
                } else if token_type == 2 {
                    let holdings = state
                        .get_erc721_holdings(program_id.into(), address.into())
                        .await;
                    let holdings: Option<Vec<U256>> = if let Some(h) = holdings {
                        Some(
                            h.iter()
                                .map(|eth_u256| {
                                    let internal_u256: U256 = eth_u256.into();
                                    internal_u256
                                })
                                .collect(),
                        )
                    } else {
                        None
                    };
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
            EoMessage::Settle {
                accounts,
                batch_header_hash,
                blob_index,
            } => {
                let res = state
                    .settle_batch(accounts, batch_header_hash, blob_index)
                    .await;
                if let Ok(handle) = res {
                    state.insert_pending((batch_header_hash, blob_index), handle);
                } else {
                    log::error!("eo_client.rs: 355: encountered error attempting to settle batch");
                }
            }
            EoMessage::SettleSuccess {
                batch_header_hash,
                blob_index,
                accounts,
                hash,
                receipt,
            } => {
                if let Some(handle) = state.remove_pending((batch_header_hash, blob_index)) {
                    let res = handle.await;
                    if let Err(e) = res {
                        log::error!("eo_client.rs: 369: JoinHandle after SettleSuccess message returned an error: {}", e);
                    }
                }
            }
            EoMessage::SettleFailure {
                batch_header_hash,
                blob_index,
                accounts,
                hash,
                receipt,
            } => {
                log::error!("eo_client.rs: 374: settlement of batch_header_hash: {:?}, blob_index: {:?} failed", &batch_header_hash, &blob_index);
                log::error!("transaction receipt: {:?}", receipt);
            }
            EoMessage::SettleTimedOut {
                batch_header_hash,
                blob_index,
                accounts,
                elapsed,
            } => {
                log::error!("eo_client.rs: 378: settlement of batch_header_hash: {:?}, blob_index: {:?} timed out", &batch_header_hash, &blob_index);
                log::error!("time elapsed: {:?}", elapsed);
                let message = EoMessage::Settle {
                    accounts,
                    batch_header_hash,
                    blob_index,
                };
                let res = myself.cast(message);
                if let Err(e) = res {
                    log::error!("eo_client.rs: 383: {}", e);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
