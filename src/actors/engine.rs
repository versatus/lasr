#![allow(unused)]
use std::{fmt::Display, collections::{BTreeMap, HashMap}};

use async_trait::async_trait;
use eigenda_client::payload::EigenDaBlobPayload;
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::{oneshot, OneshotSender, OneshotReceiver}};
use thiserror::Error;
use futures::stream::{iter, Then, StreamExt};
use crate::{RegistryMember, Account, BridgeEvent, Token, Metadata, Status, Address, create_handler, EoMessage, handle_actor_response, DaClientMessage};
use jsonrpsee::core::Error as RpcError;

use super::{messages::{RegistryMessage, RegistryResponse, EngineMessage, RegistryActor, EoEvent}, types::ActorType};

#[derive(Clone, Debug)]
pub struct Engine {
    registry: ActorRef<RegistryMessage>,
}

impl RegistryMember for Engine {
    type Err = EngineError;
}

#[derive(Clone, Debug, Error)]
pub enum EngineError {
    Custom(String)
}

impl Default for EngineError {
    fn default() -> Self {
        EngineError::Custom(
            "Engine is unable to acquire actor".to_string()
        )
    }
}

impl Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Engine {
    pub fn new(registry: ActorRef<RegistryMessage>) -> Self {
        Self { registry }
    }

    pub fn register_self(&self, myself: ActorRef<EngineMessage>) -> Result<(), Box<dyn std::error::Error>> {
        self.registry.cast(
            RegistryMessage::Register(
                ActorType::Engine, 
                RegistryActor::Engine(myself)
            )
        ).map_err(|e| Box::new(e))?;
        Ok(())
    }

    async fn get_account(&self, account: &Address) -> Account {
        log::info!("Attempting to get an acocunt");
        if let Ok(mut account) = self.check_cache(account).await { 
            return account
        } else if let Ok(mut account) = self.get_account_from_da(&account).await {
            return account
        } 

        return Account::new(account.clone(), None)
    }

    async fn check_cache(&self, account: &Address) -> Result<Account, EngineError> {
        return Err(
            EngineError::Custom(
                "cache not enabled yet".to_string()    
            )
        )
    }

    async fn handle_cache_response(
        &self, 
        rx: OneshotReceiver<Option<Account>>,
        account: &Address
    ) -> Result<Account, EngineError> {
        log::info!("Attempting to handle cache_response");
        tokio::select! {
            reply = rx => {
                match reply {
                    Ok(Some(account)) => {
                        log::info!("Found account in cache");
                        return Ok(account)
                    },
                    _ => {
                        log::info!("did not find account in cache");
                        return Err(
                            EngineError::Custom(
                                format!(
                                    "account: {:?} not found in cache",
                                    account
                                )
                            )
                        )
                    },
                }
            }
        }
    }

    async fn request_blob_index(
        &self,
        account: &Address
    ) -> Result<(Address /*user*/, String/* batchHeaderHash*/, String /*blobIndex*/), EngineError> {
        let (tx, rx) = oneshot();
        let message = EoMessage::GetAccountBlobIndex { address: account.clone(), sender: tx };
        let actor_type = ActorType::EoServer;
        let handler = create_handler!(get_eo);
        let eo_server = self.send_to_actor::<EoMessage, _, Option<ActorRef<EoMessage>>, ActorRef<EoMessage>>(
            handler, 
            actor_type, 
            message,
            self.registry.clone()
        ).await.map_err(|e| {
            EngineError::Custom(e.to_string())
        })?;

        let handler = create_handler!(retrieve_blob_index);
        let blob_response = handle_actor_response(rx, handler).await.map_err(|e| {
            EngineError::Custom(e.to_string())
        })?;

        Ok(blob_response)
    } 

    async fn get_account_from_da(
        &self,
        account: &Address,
    ) -> Result<Account, EngineError> {
        if let Ok(blob_index) = self.request_blob_index(account).await {
            // Get DA actor 
            // send message to DA actor requesting account blob
            // return account blob or Err if not found
            return Ok(Account::new(account.clone(), None))
        } else {
            return Err(
                EngineError::Custom(
                    format!(
                        "unable to find blob index for account: {:?}",
                        &account
                    )
                )
            )
        }
    }

    async fn create_token(&self, event: &BridgeEvent) -> Result<Token, EngineError> {
        todo!()
    }

    async fn update_token(&self, token: &mut Token) -> Result<&mut Token, EngineError> {
        todo!()
    }

    async fn update_account(&self, account: &mut Account) -> Result<&mut Account, EngineError> {
        todo!()
    }

    async fn handle_bridge_event(&self, logs: &Vec<BridgeEvent>) -> Result<Vec<Account>, EngineError> {
        let mut accounts: Vec<Account> = Vec::new();
        for event in logs {
            let mut account = self.get_account(&event.user().into()).await;
            let token = if event.token_id() != 0.into() {
                Token::NonFungible { 
                    program_id: event.program_id().into(), 
                    owner_id: event.user().into(), 
                    content_address: [0u8; 32], 
                    metadata: Metadata::new(vec![]), 
                    approvals: BTreeMap::new(), 
                    status: Status::Free 
                } 
            } else {
                Token::Fungible { 
                    program_id: event.program_id().into(), 
                    owner_id: event.user().into(), 
                    amount: event.amount(), 
                    metadata: Metadata::new(vec![]), 
                    allowance: BTreeMap::new(), 
                    status: Status::Free 
                }
            };
            account.update_programs(&event.program_id().into(), &token);
            accounts.push(account)
        };
        Ok(accounts)
        // Check if account exists
        //
        //  1. Check cache first
        //  2. If not in cache, ask EO to get blob index for account 
        //      i. If EO doesn't have blob index for account, 
        //          a. Create account
        //              - This includes adding the bridged token(s) in account
        //          b. Ask DA to store account & retain in cache
        //          c. Await batch header hash and blob index
        //          d. Ask EO to store batch header hash and blob index for account
        //          e. Await settlement event
        //          f. Remove from cache (if not other pending txs since)
        //      ii. If EO does have blob index for account,
        //          a. Get blob from DA
        //          b. Decode blob
        //          c. Update account for bridged token(s)
        //          Follow 2.i.c through 2.i.f
        //  3. If in cache
        //      i. Check if bridge event would affect pending tasks
        //      ii. If it would
        //          a. Avoid race condition, await pending tasks to complete
        //      iii. If it would not
        //          b. Update the relevant token(s) in parallel
        //
    }

    async fn store_blobs(&self, accounts: &Vec<Account>) -> Result<(), EngineError> {
        log::info!(
            "storing blobs for accounts: {:?}, in DA", accounts
        );
        let handler = create_handler!(get_da);
        let actor_type = ActorType::DaClient;
        let message = DaClientMessage::StoreAccountBlobs { accounts: accounts.clone() };
        self.send_to_actor::<DaClientMessage, _, Option<ActorRef<DaClientMessage>>, ActorRef<DaClientMessage>>(
            handler, actor_type, message, self.registry.clone()
        ).await?;

        Ok(())
    }

    async fn handle_settlement_event() {
        todo!()
    }

    async fn handle_call() {
        todo!()
    }

    async fn handle_send() {
        todo!()
    }

    async fn handle_deploy() {
        todo!()
    }
}

#[async_trait]
impl Actor for Engine {
    type Msg = EngineMessage;
    type State = HashMap<Address, Account>; 
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(HashMap::new())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            EngineMessage::EoEvent { event } => {
                match event {
                    EoEvent::Bridge(log) => {
                        log::info!("Engine Received EO Bridge Event: {:?}", &log);
                        let accounts = self.handle_bridge_event(&log).await?;
                        log::info!("Engine created new account(s) after bridge event: {:?}", &accounts);
                        self.store_blobs(&accounts).await?;
                    },
                    EoEvent::Settlement(log) => {
                        log::info!("Engine Received EO Settlement Event: {:?}", &log);
                    }
                }
            },
            EngineMessage::Cache { address, account } => {
                //TODO: Use a proper LRU Cache
                state.insert(address.clone(), account.clone());
            },
            _ => {}
        }
        return Ok(())
    }
}
