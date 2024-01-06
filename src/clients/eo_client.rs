#![allow(unused)]
use std::collections::HashSet;
use std::str::FromStr;

use async_trait::async_trait;
use eigenda_client::batch::BatchHeaderHash;
use ethereum_types::U256;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use secp256k1::SecretKey;
use web3::contract::{Contract, Options};
use web3::types::{Address as EthereumAddress, H256, TransactionReceipt, TransactionId};
use web3::transports::Http;
use web3::Web3;
use web3::ethabi::Token as EthAbiToken;
use sha3::{Digest, Keccak256};
use hex;

use crate::{Address, EoMessage};

#[derive(Clone, Debug)]
pub struct EoClientActor;

#[derive(Builder, Clone)]
pub struct EoClient {
    contract: Contract<Http>,
    web3: Web3<Http>,
    address: EthereumAddress,
    sk: web3::signing::SecretKey
}

impl EoClient {
    pub async fn new(
        web3: Web3<Http>,
        contract: Contract<Http>,
        address: Address,
        sk: web3::signing::SecretKey,
    ) -> web3::Result<Self> {
        let address = EoClient::parse_checksum_address(address)?;
        
        Ok(
            EoClient { contract, web3, address, sk }
        )
    }

    fn parse_checksum_address(address: Address) -> Result<EthereumAddress, web3::Error> {
        let bytes = address.as_ref();
        let checksum = EoClient::to_checksum_address(bytes);
        checksum.parse().map_err(|_| web3::Error::Internal)
    }

    async fn get_blob_index(&self, address: EthereumAddress) -> Option<(H256, u128)> {
        self.contract.query(
            "getBlobIndex",
            (address,),
            None,
            Options::default(),
            None
        ).await.ok()
    }

    async fn get_eth_balance(&self, address: EthereumAddress) -> Option<U256> {
        self.contract.query(
            "getEthBalance",
            (address,),
            None,
            Options::default(),
            None
        ).await.ok()
    }

    async fn get_erc20_balance(
        &self,
        program_id: EthereumAddress,
        address: EthereumAddress
    ) -> Option<U256> {
        self.contract.query(
            "getERC20Balance",
            (program_id, address,),
            None,
            Options::default(),
            None
        ).await.ok()
    }

    async fn get_erc721_holdings(
        &self,
        program_id: EthereumAddress,
        address: EthereumAddress,
    ) -> Option<Vec<U256>> {
        self.contract.query(
            "getERC721Holdings",
            (program_id, address,),
            None,
            Options::default(),
            None
        ).await.ok()
    }
    
    fn to_checksum_address(bytes: &[u8]) -> String {
        let hex_address = hex::encode(bytes).to_lowercase();
        let mut hasher = Keccak256::new();
        hasher.update(hex_address.as_bytes());
        let hash = hasher.finalize();
        let checksum_address: String = hex_address.char_indices().map(|(i, c)| {
            if c >= '0' && c <= '9' {
                c.to_string()
            } else {
                if hash[i / 2] >> 4 >= 8 && i % 2 == 0 || hash[i / 2] & 0x0f >= 8 && i % 2 != 0 {
                    c.to_uppercase().to_string()
                } else {
                    c.to_lowercase().to_string()
                }
            }
        }).collect();

        format!("0x{}", checksum_address)
    }

    pub(super) async fn settle_batch(
        &mut self, 
        accounts: HashSet<Address>,
        batch_header_hash: H256,
        blob_index: u128
    ) -> Result<H256, web3::Error> {
        log::info!("attempting to settle batch");
        let eth_addresses: Vec<EthereumAddress> = accounts.iter().filter_map(|a| {
            EoClient::parse_checksum_address(a.clone()).ok()
        }).collect();

        log::info!("parsed addresses for batch settlement");
        let addresses: Vec<EthAbiToken> = eth_addresses.iter().map(|a| EthAbiToken::Address(a.clone())).collect();

        let n_accounts = addresses.len();

        let accounts = EthAbiToken::Array(
            addresses
        );

        let blob_index = EthAbiToken::Tuple(
            vec![
                EthAbiToken::FixedBytes(batch_header_hash.0.to_vec()), 
                EthAbiToken::Uint(blob_index.into())
            ]
        );

        let mut options = Options {
            gas: Some(web3::types::U256::from(((50_000 * n_accounts) + 50_000))),
            ..Options::default()
        };

        log::info!("sending transaction");
        let res = self.contract.signed_call(
            "settleBlobIndex", 
            (accounts, blob_index),
            options,
            &mut self.sk.clone()
        ).await;

        log::info!("checking result");
        if let Err(e) = &res {
            log::error!("eo_client encountered error: {}", e);
        } else if let Ok(hash) = &res {
            log::info!("eo_client settled batch: {:?}", res);
            let mut receipt = None;
            while receipt.is_none() {
                receipt = self.web3.eth().transaction_receipt(hash.clone()).await?;
            }

            if let Some(receipt) = receipt {
                if receipt.status == Some(web3::types::U64::from(0)) {
                    log::error!("transaction failed");
                    return Err(
                        web3::Error::InvalidResponse(
                            format!("transaction failed: {:?}", receipt)
                        )
                    )
                }
            }
        }

        return res
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
        Ok(args.clone())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            EoMessage::GetAccountBlobIndex { address, sender } => {
                let blob = state.get_blob_index(address.into()).await;
                if let Some((batch_header_hash, blob_index)) = blob {
                    if !batch_header_hash.is_zero() {
                        let res = sender.send(
                            EoMessage::AccountBlobIndexAcquired { 
                                address,
                                batch_header_hash,
                                blob_index
                            }
                        );

                        if let Err(e) = res {
                            log::error!("{:?}", e);
                        }
                    }
                } else {
                    log::info!("unable to find blob index for address: 0x{:x}", address);
                    let _res = sender.send(
                        EoMessage::AccountBlobIndexNotFound { address }
                    );
                }
            }
            EoMessage::GetContractBlobIndex { program_id: _, sender: _ } => {
                log::info!("Received request for contract blob index");
            }
            EoMessage::GetAccountBalance { 
                program_id, 
                address, 
                sender, 
                token_type 
            } => {
                if token_type == 0 {
                    let balance = state.get_eth_balance(address.into()).await;
                    let res = sender.send(
                        EoMessage::AccountBalanceAcquired { program_id, address, balance }
                    );
                    if let Err(e) = res {
                        log::error!("{:?}", e);
                    }
                } else if token_type == 1 {
                    let balance = state.get_erc20_balance(program_id.into(), address.into()).await;
                    let res = sender.send(
                        EoMessage::AccountBalanceAcquired { program_id, address, balance }
                    );
                    if let Err(e) = res {
                        log::error!("{:?}", e);
                    }
                } else if token_type == 2 {
                    let holdings = state.get_erc721_holdings(program_id.into(), address.into()).await;
                    let res = sender.send(
                        EoMessage::NftHoldingsAcquired { program_id, address, holdings }
                    );
                    if let Err(e) = res {
                        log::error!("{:?}", e);
                    }
                }
            }
            EoMessage::Settle { accounts, batch_header_hash, blob_index } => {

                let tx_hash = state.settle_batch(accounts, batch_header_hash, blob_index).await; 
                if let Ok(h) = &tx_hash {
                    log::info!("settled transaction to EO with tx: {:?}", h);
                } else {
                    log::error!("{:?}", &tx_hash);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
