#![allow(unused)]
use std::{fmt::Display, collections::{BTreeMap, HashMap}};

use async_trait::async_trait;
use eigenda_client::payload::EigenDaBlobPayload;
use ethereum_types::U256;
use ractor::{ActorRef, Actor, ActorProcessingErr, concurrency::{oneshot, OneshotSender, OneshotReceiver}};
use thiserror::Error;
use futures::{stream::{iter, Then, StreamExt}, TryFutureExt};
use crate::{Account, BridgeEvent, Metadata, Status, Address, create_handler, EoMessage, handle_actor_response, DaClientMessage, AccountCacheMessage, Token, TokenBuilder, ArbitraryData, TransactionBuilder, TransactionType, Transaction, PendingTransactionMessage, RecoverableSignature};
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
        } 

        if let Ok(mut account) = self.get_account_from_da(&address).await {
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
    ) -> Result<(Address /*user*/, String/* batchHeaderHash*/, u128 /*blobIndex*/), EngineError> {
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
        address: &Address,
    ) -> Result<Account, EngineError> {
        if let Ok((address, batch_header_hash, blob_index)) = self.request_blob_index(address).await {
            let da_actor: ActorRef<DaClientMessage> = ractor::registry::where_is(ActorType::DaClient.to_string()).ok_or(
                EngineError::Custom("unable to acquire DA actor".to_string())
            )?.into();
            let (tx, rx) = oneshot();
            let message = DaClientMessage::RetrieveBlob { batch_header_hash, blob_index, tx };
            let _ = da_actor.cast(message).map_err(|e| EngineError::Custom(e.to_string()))?;
            let handler = create_handler!(retrieve_blob);
            let account = handle_actor_response(rx, handler).await.map_err(|e| {
                EngineError::Custom(e.to_string())
            })?.ok_or(
                EngineError::Custom("unable to acquire account from blob storage".to_string())
            )?;
            return Ok(account)
        } else {
            return Err(
                EngineError::Custom(
                    format!( "unable to find blob index for account: {:?}",
                        &address
                    )
                )
            )
        }
    }

    async fn set_pending_transaction(&self, transaction: Transaction) -> Result<(), EngineError> {
        let actor: ActorRef<PendingTransactionMessage> = ractor::registry::where_is(
            ActorType::PendingTransactions.to_string()
        ).ok_or(
            EngineError::Custom("unable to acquire pending transactions actor".to_string())
        )?.into();
        let message = PendingTransactionMessage::New { transaction: transaction.clone() };
        actor.cast(message).map_err(|e| EngineError::Custom(e.to_string()))?;

        Ok(())
    }

    async fn handle_bridge_event(&self, logs: &Vec<BridgeEvent>) -> Result<(), EngineError> {
        for event in logs {
            // Turn event into Transaction
            let transaction = TransactionBuilder::default()
                .program_id(event.program_id().into())
                .from(event.user().into())
                .to(event.user().into())
                .transaction_type(TransactionType::BridgeIn)
                .value(event.amount())
                .inputs(String::new())
                .v(0)
                .r([0; 32])
                .s([0; 32])
                .build().map_err(|e| EngineError::Custom(e.to_string()))?;
            self.set_pending_transaction(transaction.clone()).await?;
        }

        Ok(())
    }

    async fn handle_call(
        &self,
        program_id: Address,
        from: Address, 
        op: String,
        inputs: String,
        sig: RecoverableSignature
    ) -> Result<(), EngineError> {
        let transaction = TransactionBuilder::default()
            .program_id(program_id.into())
            .from(from.into())
            .to([0u8; 20])
            .transaction_type(TransactionType::Call)
            .inputs(inputs)
            .value(0.into())
            .v(sig.get_v())
            .r(sig.get_r())
            .s(sig.get_s())
            .build().map_err(|e| EngineError::Custom(e.to_string()))?;
            
        self.set_pending_transaction(transaction).await?;
        Ok(())
    }

    async fn handle_send(
        &self,
        program_id: Address,
        from: Address,
        to: Address,
        value: U256,
        sig: RecoverableSignature,
    ) -> Result<(), EngineError> {
        let transaction = TransactionBuilder::default()
            .program_id(program_id.into())
            .from(from.into())
            .to(to.into())
            .transaction_type(TransactionType::Send)
            .value(value)
            .inputs(String::new())
            .v(sig.get_v())
            .r(sig.get_r())
            .s(sig.get_s())
            .build().map_err(|e| EngineError::Custom(e.to_string()))?;
        self.set_pending_transaction(transaction).await?;

        Ok(())
        
    }

    async fn handle_deploy(
        &self,
        program_id: Address,
        from: Address,
        sig: RecoverableSignature
    ) -> Result<(), EngineError> {
        let transaction = TransactionBuilder::default()
            .program_id(program_id.into())
            .from(from.into())
            .to([0u8; 20])
            .transaction_type(TransactionType::Deploy)
            .value(0.into())
            .inputs(String::new())
            .v(sig.get_v())
            .r(sig.get_r())
            .s(sig.get_s())
            .build().map_err(|e| EngineError::Custom(e.to_string()))?;
        self.set_pending_transaction(transaction).await?;

        Ok(())
    }
}

#[async_trait]
impl Actor for Engine {
    type Msg = EngineMessage;
    type State = (); 
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
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
                        self.handle_bridge_event(&log).await.unwrap_or_else(|e| {
                            log::error!("{}", e);
                        });
                    },
                    EoEvent::Settlement(log) => {
                        log::info!("Engine Received EO Settlement Event");
                    }
                }
            },
            EngineMessage::Cache { account, .. } => {
                //TODO(asmith): Use a proper LRU Cache
                self.write_to_cache(account);
            },
            EngineMessage::Call {
                program_id, from, op, inputs, sig
            } => {
                self.handle_call(program_id, from, op, inputs, sig).await;
            },
            EngineMessage::Send {
                program_id, from, to, amount, content, sig,
            } => {
                self.handle_send(program_id, from, to, amount, sig).await;
            },
            EngineMessage::Deploy {
                program_id, from, sig
            } => {
                self.handle_deploy(program_id, from, sig).await;
            }
            _ => {}
        }
        return Ok(())
    }
}
