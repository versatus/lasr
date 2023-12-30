use async_trait::async_trait;
use ethereum_types::U256;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use web3::contract::{Contract, Options};
use web3::types::{Address as EthereumAddress, H256};
use web3::transports::Http;
use web3::Web3;
use sha3::{Digest, Keccak256};
use hex;

use crate::{Address, EoMessage};

#[derive(Clone, Debug)]
pub struct EoClientActor;

#[derive(Builder, Clone)]
pub struct EoClient {
    contract: Contract<Http>,
    web3: Web3<Http>,
    address: EthereumAddress
}

impl EoClient {
    pub async fn new(
        web3: Web3<Http>,
        contract: Contract<Http>,
        address: Address,
    ) -> web3::Result<Self> {
        let address = EoClient::parse_checksum_address(address)?;
        
        Ok(
            EoClient { contract, web3, address }
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
                    let _res = sender.send(
                        EoMessage::AccountBlobIndexAcquired { 
                            address,
                            batch_header_hash,
                            blob_index
                        }
                    );
                } else {
                    log::info!("unable to find blob index for address: 0x{:x}", address);
                    let _res = sender.send(
                        EoMessage::AccountBlobIndexNotFound { address }
                    );
                }
            }
            EoMessage::GetContractBlobIndex { program_id: _, sender: _ } => {
            }
            EoMessage::GetAccountBalance { program_id: _, address: _, sender: _ } => {
            }
            _ => {}
        }
        Ok(())
    }
}
