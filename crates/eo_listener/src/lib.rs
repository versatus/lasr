#![allow(unused)]
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{mpsc::UnboundedSender, oneshot::Receiver};
use web3::types::U64;
use web3::{
    contract::{
        tokens::{Detokenize, Tokenize},
        Contract, Options,
    },
    ethabi::RawLog as ParsedLog,
    transports::Http,
    types::{Address, BlockId, BlockNumber, Filter, FilterBuilder, Log, H160, H256, U256},
    Error as Web3Error, Transport, Web3,
};

#[macro_export]
macro_rules! log_handler {
    () => {
        |logs| match logs {
            Ok(l) => l,
            Err(_) => Vec::new(),
        }
    };
}

pub async fn get_abi() -> Result<web3::ethabi::Contract, EoServerError> {
    let abi_bytes = tokio::fs::read("../eo_contract_abi.json")
        .await
        .map_err(|e| EoServerError::Other(e.to_string()))?;
    let full_json_with_abi: serde_json::Value =
        serde_json::from_slice(&abi_bytes).map_err(|e| EoServerError::Other(e.to_string()))?;
    let x = serde_json::to_vec(full_json_with_abi.get("abi").expect("create abi bytes err"))
        .map_err(|e| EoServerError::Other(e.to_string()))?;

    web3::ethabi::Contract::load(&*x).map_err(|e| EoServerError::Other(e.to_string()))
}

pub fn get_blob_index_settled_topic() -> Option<Vec<H256>> {
    let mut hasher = Keccak256::new();
    let blob_index_settled_sig = b"BlobIndexSettled(address[],bytes32,uint128,uint256)";
    hasher.update(blob_index_settled_sig);
    let res: [u8; 32] = hasher.finalize().try_into().ok()?;
    let blob_settled_topic = H256::from(res);
    Some(vec![blob_settled_topic])
}

pub fn get_bridge_event_topic() -> Option<Vec<H256>> {
    let mut hasher = Keccak256::new();
    let bridge_sig = b"Bridge(address,address,uint256,uint256,string,uint256)";
    hasher.update(bridge_sig);
    let res: [u8; 32] = hasher.finalize().try_into().ok()?;
    let bridge_topic = H256::from(res);
    Some(vec![bridge_topic])
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BlocksProcessed {
    pub bridge: Option<U64>,
    pub settle: Option<U64>,
    pub bridge_processed: BTreeSet<U64>,
    pub settled_processed: BTreeSet<U64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SettlementLayer {
    Ethereum,
    Other(usize),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    Log(Log),
    Tx {
        content_id: String,
        token_address: Option<String>,
        contract_abi: Option<String>,
        from: String,
        op: String,
        inputs: String,
        settlement_layer: SettlementLayer,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EventType {
    Bridge(web3::ethabi::Event),
    Settlement(web3::ethabi::Event),
}

#[derive(Clone, Debug)]
pub struct EventLogResult {
    pub event_type: EventType,
    pub log_result: web3::Result<Vec<web3::ethabi::Log>>,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct StopToken;

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum EoServerError {
    Other(String),
}

impl From<EoServerBuilderError> for EoServerError {
    fn from(value: EoServerBuilderError) -> Self {
        Self::Other(value.to_string())
    }
}

impl std::fmt::Display for EoServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for EoServerError {
    fn description(&self) -> &str {
        match self {
            EoServerError::Other(desc) => &desc[..],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractAddress([u8; 32]);

impl From<String> for ContractAddress {
    fn from(value: String) -> Self {
        let arr = value[..64].as_bytes();
        let mut bytes = [0u8; 32];
        for (idx, byte) in arr.iter().enumerate() {
            bytes[idx] = *byte;
        }

        ContractAddress(bytes)
    }
}

/// An ExecutableOracle server that listens for events emitted from
/// Smart Contracts
#[derive(Builder, Debug, Clone)]
pub struct EoServer {
    web3: Web3<Http>,
    eo_address: EoAddress,
    block_time: Duration,
    bridge_processed_blocks: BTreeSet<U64>,
    settled_processed_blocks: BTreeSet<U64>,
    contract: web3::contract::Contract<Http>,
    bridge_topic: Option<Vec<H256>>,
    blob_settled_topic: Option<Vec<H256>>,
    blob_settled_filter: Filter,
    bridge_filter: Filter,
    current_bridge_filter_block: U64,
    current_blob_settlement_filter_block: U64,
    blob_settled_event: web3::ethabi::Event,
    bridge_event: web3::ethabi::Event,
    path: std::path::PathBuf,
}

impl EoServer {
    pub async fn load_processed_blocks(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut buf = Vec::new();
        let mut file = std::fs::OpenOptions::new().read(true).open(&self.path)?;

        file.read_to_end(&mut buf)?;

        let blocks_processed: BlocksProcessed = bincode::deserialize(&buf)?;
        if let Some(b) = blocks_processed.bridge {
            self.current_bridge_filter_block = b;
        }

        if let Some(b) = blocks_processed.settle {
            self.current_blob_settlement_filter_block = b;
        }

        self.bridge_processed_blocks = blocks_processed.bridge_processed;
        self.settled_processed_blocks = blocks_processed.settled_processed;

        Ok(())
    }

    pub async fn run(mut self) -> Result<(), web3::Error> {
        self.run_loop().await
    }

    pub async fn next(&mut self) -> EventLogResult {
        let log_handler = log_handler!();
        tokio::select!(
            blob_settled_logs = self.web3.eth().logs(
                self.blob_settled_filter.clone()
            ) => {
                let log_result = self.process_logs(
                    EventType::Settlement(
                        self.blob_settled_event.clone()
                    ), blob_settled_logs,
                    log_handler,
                ).await.map_err(|e| Web3Error::from(e.to_string()));

                return EventLogResult {
                    event_type: EventType::Settlement(self.blob_settled_event.clone()),
                    log_result
                };
            },
            bridge_logs = self.web3.eth().logs(
                self.bridge_filter.clone()
            ) => {
                let log_result = self.process_logs(
                    EventType::Bridge(
                        self.bridge_event.clone()
                    ), bridge_logs,
                    log_handler,
                ).await.map_err(|e| Web3Error::from(e.to_string()));
                return EventLogResult {
                    event_type: EventType::Bridge(self.bridge_event.clone()),
                    log_result
                };
            },
        )
    }

    async fn run_loop(&mut self) -> Result<(), web3::Error> {
        let blob_settled_event = self
            .contract
            .abi()
            .event("BlobIndexSettled")
            .map_err(|e| Web3Error::from(e.to_string()))?
            .clone();

        let bridge_event = self
            .contract
            .abi()
            .event("Bridge")
            .map_err(|e| Web3Error::from(e.to_string()))?
            .clone();

        loop {
            let log_handler = log_handler!();
            tokio::select!(
                blob_settled_logs = self.web3.eth().logs(self.blob_settled_filter.clone()) => {
                    self.process_logs(EventType::Settlement(blob_settled_event.clone()), blob_settled_logs, log_handler).await;
                },
                bridge_logs = self.web3.eth().logs(self.bridge_filter.clone()) => {
                    self.process_logs(
                        EventType::Bridge(
                            bridge_event.clone()
                        ), bridge_logs, log_handler
                    ).await;
                },
            );
            tokio::time::sleep(self.block_time).await;
        }
        Ok(())
    }

    async fn process_logs<F>(
        &mut self,
        event_type: EventType,
        logs: Result<Vec<Log>, Web3Error>,
        handler: F,
    ) -> Result<Vec<web3::ethabi::Log>, EoServerError>
    where
        F: FnOnce(Result<Vec<Log>, Web3Error>) -> Vec<Log>,
    {
        let block_number = self.web3.eth().block_number().await.map_err(|e| {
            EoServerError::Other(format!("Error attempting to get block number: {}", e))
        })?;

        let events = handler(logs);
        match event_type {
            EventType::Bridge(event_abi) => {
                let bridge_log = self.handle_bridge_event(events, &event_abi);
                if let Ok(logs) = &bridge_log {
                    if logs.len() > 0 {
                        log::info!("discovered logs: logs.len() = {}", logs.len());
                        self.increment_bridge_filter(block_number, true);
                    } else {
                        self.increment_bridge_filter(block_number, false);
                    }
                }
                return bridge_log;
            }
            EventType::Settlement(event_abi) => {
                let blob_log = self.handle_settlement_event(events, &event_abi);
                if let Ok(logs) = &blob_log {
                    if logs.len() > 0 {
                        self.increment_blob_filter(block_number, true);
                    } else {
                        self.increment_blob_filter(block_number, false);
                    }
                }
                return blob_log;
            }
        }

        Ok(vec![])
    }

    fn handle_bridge_event(
        &mut self,
        events: Vec<Log>,
        event_abi: &web3::ethabi::Event,
    ) -> Result<Vec<web3::ethabi::Log>, EoServerError> {
        let mut parsed_logs = Vec::new();
        let mut blocks_processed = Vec::new();
        for event in events {
            let block_number = event
                .block_number
                .ok_or(EoServerError::Other("Log missing block number".to_string()))?;
            let log = self.parse_bridge_event(event, event_abi)?;
            parsed_logs.push(log);
            blocks_processed.push(block_number);
        }
        self.bridge_processed_blocks.extend(blocks_processed);
        Ok(parsed_logs)
    }

    fn handle_settlement_event(
        &mut self,
        events: Vec<Log>,
        event_abi: &web3::ethabi::Event,
    ) -> Result<Vec<web3::ethabi::Log>, EoServerError> {
        let mut parsed_logs = Vec::new();
        let mut blocks_processed = Vec::new();
        for event in events {
            let block_number = event
                .block_number
                .ok_or(EoServerError::Other("Log missing block number".to_string()))?;
            let log = self.parse_settlement_event(event, event_abi)?;
            parsed_logs.push(log);
            blocks_processed.push(block_number);
        }
        self.settled_processed_blocks.extend(blocks_processed);
        Ok(parsed_logs)
    }

    fn parse_bridge_event(
        &self,
        event: Log,
        event_abi: &web3::ethabi::Event,
    ) -> Result<web3::ethabi::Log, EoServerError> {
        let parsed_log = event_abi
            .parse_log(web3::ethabi::RawLog {
                topics: event.topics.clone(),
                data: event.data.0.clone(),
            })
            .map_err(|e| EoServerError::Other(e.to_string()))?;
        Ok(parsed_log)
    }

    fn parse_settlement_event(
        &self,
        event: Log,
        event_abi: &web3::ethabi::Event,
    ) -> Result<web3::ethabi::Log, EoServerError> {
        let parsed_log = event_abi
            .parse_log(web3::ethabi::RawLog {
                topics: event.topics.clone(),
                data: event.data.0.clone(),
            })
            .map_err(|e| EoServerError::Other(e.to_string()))?;

        Ok(parsed_log)
    }

    fn increment_bridge_filter(
        &mut self,
        block_number: U64,
        log_discovered: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let contract_address = self
            .eo_address
            .clone()
            .parse()
            .map_err(|err| EoServerError::Other(err.to_string()))?;

        let default: U64 = U64::from(0);
        let from_block = self
            .bridge_processed_blocks
            .last()
            .unwrap_or_else(|| &default);
        let to_block = self.current_bridge_filter_block + U64::from(1);
        log::info!(
            "filtering from block {} to block {}",
            &from_block,
            &to_block
        );

        let new_filter = FilterBuilder::default()
            .from_block(BlockNumber::Number(*from_block)) // Last processed block
            .to_block(BlockNumber::Number(to_block))
            .address(vec![contract_address])
            .topics(self.bridge_topic.clone(), None, None, None)
            .build();

        if log_discovered {
            self.bridge_processed_blocks.insert(block_number);
        }

        self.current_bridge_filter_block = block_number;
        self.bridge_filter = new_filter;

        Ok(())
    }

    fn increment_blob_filter(
        &mut self,
        block_number: U64,
        log_discovered: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let contract_address = self
            .eo_address
            .clone()
            .parse()
            .map_err(|err| EoServerError::Other(err.to_string()))?;

        let default: U64 = U64::from(0);
        let from_block = self
            .settled_processed_blocks
            .last()
            .unwrap_or_else(|| &default);
        let to_block = self.current_blob_settlement_filter_block + U64::from(1);

        let new_filter = FilterBuilder::default()
            .from_block(BlockNumber::Number(*from_block))
            .to_block(BlockNumber::Number(to_block))
            .address(vec![contract_address])
            .topics(self.blob_settled_topic.clone(), None, None, None)
            .build();

        if log_discovered {
            self.settled_processed_blocks.insert(block_number);
        }
        self.current_blob_settlement_filter_block = block_number;
        self.blob_settled_filter = new_filter;

        Ok(())
    }

    fn inner_highest_bridge_block_processed(&self) -> Option<&U64> {
        self.bridge_processed_blocks.last()
    }

    fn inner_highest_bridge_block_processed_owned(&self) -> Option<U64> {
        if let Some(b) = self.bridge_processed_blocks.last() {
            return Some(b.clone());
        }

        None
    }

    fn inner_lowest_bridge_block_processed(&self) -> Option<&U64> {
        self.bridge_processed_blocks.first()
    }

    fn inner_lowest_bridge_block_processed_owned(&self) -> Option<U64> {
        if let Some(b) = self.bridge_processed_blocks.first() {
            return Some(b.clone());
        }

        None
    }

    fn bridge_block_processed(&self, block_number: &U64) -> bool {
        self.bridge_processed_blocks.contains(block_number)
    }

    async fn get_account_balance_eth(
        &mut self,
        address: H160,
        block: Option<BlockNumber>,
    ) -> Result<web3::types::U256, web3::Error> {
        self.web3.eth().balance(address, block).await
    }

    async fn get_batch_account_balance_eth(
        &mut self,
        addresses: impl IntoIterator<Item = H160>,
        block: Option<BlockNumber>,
    ) -> Result<Vec<(H160, web3::types::U256)>, web3::Error> {
        let mut balances = Vec::new();
        for address in addresses {
            let balance = self.get_account_balance_eth(address, block).await?;
            balances.push((address, balance));
        }

        Ok(balances)
    }

    async fn get_account_contract_data<T, R, A, B, P>(
        &mut self,
        address: A,
        contract: Contract<T>,
        block: B,
        function: &str,
        params: P,
        options: Options,
    ) -> Result<R, web3::contract::Error>
    where
        T: Transport,
        R: Detokenize,
        A: Into<Option<Address>>,
        B: Into<Option<BlockId>>,
        P: Tokenize,
    {
        contract
            .query::<R, A, B, P>(function, params, address, options, block)
            .await
    }

    async fn get_batch_account_contract_data<T, R, A, B, P>(
        &mut self,
        account_contract_data: impl IntoIterator<Item = (A, Contract<T>, &str, P, Options)>,
        block: B,
    ) -> Result<Vec<(A, R)>, web3::contract::Error>
    where
        T: Transport,
        R: Detokenize,
        A: Into<Option<Address>> + Clone,
        B: Into<Option<BlockId>> + Clone,
        P: Tokenize,
    {
        let mut results = Vec::new();
        for p in account_contract_data {
            let res = self
                .get_account_contract_data(p.0.clone(), p.1, block.clone(), p.2, p.3, p.4)
                .await?;
            results.push((p.0, res));
        }

        Ok(results)
    }

    pub fn contract(&self) -> &Contract<Http> {
        &self.contract
    }

    pub fn save_blocks_processed(&self) -> Result<(), Box<dyn std::error::Error>> {
        let blocks_processed = BlocksProcessed {
            bridge: Some(self.current_bridge_filter_block.clone()),
            settle: Some(self.current_blob_settlement_filter_block.clone()),
            bridge_processed: self.bridge_processed_blocks.clone(),
            settled_processed: self.settled_processed_blocks.clone(),
        };

        let bytes = bincode::serialize(&blocks_processed)?;

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.path)?;

        file.write(&bytes)?;

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct EoAddress(String);

impl EoAddress {
    pub fn new(address: &str) -> Self {
        EoAddress(address.to_string())
    }

    pub fn parse(&self) -> Result<H160, rustc_hex::FromHexError> {
        self.0.parse()
    }
}

impl EventSignatureHash {
    pub fn parse(&self) -> Result<H256, rustc_hex::FromHexError> {
        self.0.parse()
    }
}

#[derive(Clone, Debug)]
pub struct EventSignatureHash(String);
