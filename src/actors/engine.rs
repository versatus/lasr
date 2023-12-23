#![allow(unused)]
use std::{fmt::Display, collections::{BTreeMap, HashMap}};

use async_trait::async_trait;
use eigenda_client::payload::EigenDaBlobPayload;
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::{oneshot, OneshotSender, OneshotReceiver}};
use thiserror::Error;
use futures::stream::{iter, Then, StreamExt};
use crate::{Account, BridgeEvent, Metadata, Status, Address, create_handler, EoMessage, handle_actor_response, DaClientMessage, AccountCacheMessage, Token};
use jsonrpsee::core::Error as RpcError;
use tokio::sync::mpsc::Sender;

use super::{messages::{EngineMessage, EoEvent}, types::ActorType};

#[derive(Clone, Debug)]
pub struct Engine;

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
    pub fn new() -> Self {
        Self 
    }

    async fn get_account(&self, address: &Address) -> Account {
        if let Ok(Some(account)) = self.check_cache(address).await { 
            return account
        } else if let Ok(mut account) = self.get_account_from_da(&address).await {
            // self.account_cache_writer.send(account.clone());
            return account
        } 
        
        let account = Account::new(address.clone(), None);
        return account
    }

    async fn write_to_cache(&self, account: Account) -> Result<(), EngineError> {
        let message = AccountCacheMessage::Write { account };
        let cache_actor = ractor::registry::where_is(ActorType::AccountCache.to_string()).ok_or(
            EngineError::Custom("unable to find AccountCacheActor in registry".to_string())
        )?;
        cache_actor.send_message(message);
        Ok(())
    }

    async fn check_cache(&self, address: &Address) -> Result<Option<Account>, EngineError> {
        let (tx, rx) = oneshot();
        let message = AccountCacheMessage::Read { address: address.clone(), tx }; 
        let cache_actor = ractor::registry::where_is(ActorType::AccountCache.to_string()).ok_or(
            EngineError::Custom("unable to find AccountCacheActor in registry".to_string())
        )?;
        cache_actor.send_message(message);
        return self.handle_cache_response(rx).await
    }

    async fn handle_cache_response(
        &self, 
        rx: OneshotReceiver<Option<Account>>,
    ) -> Result<Option<Account>, EngineError> {
        tokio::select! {
            reply = rx => {
                match reply {
                    Ok(Some(account)) => {
                        return Ok(Some(account))
                    },
                    Ok(None) => {
                        return Ok(None)
                    }
                    Err(e) => {
                        log::error!("{}", e);
                    },
                }
            }
        }
        Ok(None)
    }

    async fn request_blob_index(
        &self,
        account: &Address
    ) -> Result<(Address /*user*/, String/* batchHeaderHash*/, String /*blobIndex*/), EngineError> {
        let (tx, rx) = oneshot();
        let message = EoMessage::GetAccountBlobIndex { address: account.clone(), sender: tx };
        let actor: ActorRef<EoMessage> = ractor::registry::where_is(
            ActorType::EoServer.to_string()
        ).ok_or(
            EngineError::Custom("unable to acquire EO Server Actor".to_string())
        )?.into();

        actor.cast(message).map_err(|e| EngineError::Custom(e.to_string()))?;
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
                    format!( "unable to find blob index for account: {:?}",
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
            if let Some(entry) = account.programs().get(&event.program_id().into()) {
                log::info!(
                    "account current token_id: {:x} balance: {}", 
                    event.program_id(), 
                    entry.balance()
                );
            }
        };
        Ok(accounts)
    }

    async fn store_blobs(&self, accounts: &Vec<Account>) -> Result<(), EngineError> {
        // let handler = create_handler!(get_da);
        // let actor_type = ActorType::DaClient;
        // let message = DaClientMessage::StoreAccountBlobs { accounts: accounts.to_vec() };
        // self.send_to_actor::<DaClientMessage, _, Option<ActorRef<DaClientMessage>>, ActorRef<DaClientMessage>>(
        //    handler, actor_type, message, self.registry.clone()
        //).await?;

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
                        log::info!("Engine Received EO Bridge Event");
                        let accounts = self.handle_bridge_event(&log).await?;
                        log::info!("Engine created new account(s) after bridge event");
                        self.store_blobs(&accounts).await?;
                    },
                    EoEvent::Settlement(log) => {
                        log::info!("Engine Received EO Settlement Event");
                    }
                }
            },
            EngineMessage::Cache { address, account } => {
                //TODO: Use a proper LRU Cache
                state.insert(address.clone(), account);
            },
            _ => {}
        }
        return Ok(())
    }
}
