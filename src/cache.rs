#![allow(unused)]
use std::{collections::HashMap, fmt::Display};

use async_trait::async_trait;
use eigenda_client::proof::BlobVerificationProof;
use futures::stream::{FuturesUnordered, StreamExt};
use ractor::{
    ActorRef, 
    concurrency::{
        oneshot, 
        OneshotReceiver, 
        OneshotSender
    }, Actor, 
    ActorProcessingErr
};
use thiserror::Error;
use tokio::sync::mpsc::{Receiver, Sender};

use eigenda_client::response::BlobResponse;
use crate::{
    DaClientMessage, 
    Address, 
    EoMessage, 
    Account, 
    EngineMessage, 
    ValidatorMessage, 
    Token, 
    SchedulerMessage, 
    ActorType, 
    AccountCacheMessage, 
    TokenDelta
};

#[derive(Debug, Clone)]
pub struct AccountCacheActor;

#[derive(Debug, Clone, Error)]
pub struct AccountCacheError;

impl Display for AccountCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Default for AccountCacheError {
    fn default() -> Self {
        AccountCacheError
    }
}

#[derive(Debug)]
pub struct AccountCache {
    cache: HashMap<Address, Account>,
    receivers: FuturesUnordered<OneshotReceiver<Address>>,
    // engine_actor: ActorRef<EngineMessage>,
    // validator_actor: ActorRef<ValidatorMessage>,
    // eo_actor: ActorRef<EoMessage>,
    // writer: Receiver<Account>,
    // checker: Receiver<(Address, OneshotSender<Option<Account>>)>,
}

#[derive(Debug)]
pub struct PendingTokens {
    map: HashMap<Address, Vec<OneshotSender<(Address, Address)>>>
}

impl PendingTokens {
    pub fn new(token: Token, sender: OneshotSender<(Address, Address)>) -> Self {
        let mut map = HashMap::new();
        map.insert(token.program_id(), vec![sender]);
        Self {
            map
        }
    }

    pub(crate) fn insert(
        &mut self,
        token: Token,
        sender: OneshotSender<(Address, Address)>
    ) -> Result<(), Box<dyn std::error::Error>> {
        let address = token.program_id();
        if let Some(mut entry) = self.map.get_mut(&address) {
            entry.push(sender);
            return Ok(())
        } 
        self.map.insert(address, vec![sender]);
        Ok(())
    }

    pub(crate) fn remove(
        &mut self,
        token: &Token
    ) -> Option<Vec<OneshotSender<(Address, Address)>>> {
        let program_id = token.program_id();
        self.map.remove(&program_id)
    }

    pub(crate) fn get(
        &mut self,
        token: &Token
    ) -> Option<&mut Vec<OneshotSender<(Address, Address)>>> {
        let program_id = token.program_id();
        self.map.get_mut(&program_id)
    }
}

#[derive(Debug)]
pub struct PendingTransactions {
    pending: HashMap<Address, PendingTokens>,
    receivers: FuturesUnordered<OneshotReceiver<(Address, Token)>>,
    scheduler_actor: ActorRef<SchedulerMessage>,
    engine_actor: ActorRef<EngineMessage>,
    eo_actor: ActorRef<EngineMessage>,
    writer: Receiver<(Address, Token, OneshotSender<(Address, Address)>)>
}

#[derive(Debug)]
pub struct PendingBlobCache {
    //TODO(asmith) create an ergonimical RequestId struct for EigenDa 
    //Blob responses
    queue: HashMap<Address, BlobResponse>,
    receivers: FuturesUnordered<OneshotReceiver<(Address, BlobVerificationProof)>>,
    da_actor: ActorRef<DaClientMessage>,
    eo_actor: ActorRef<EoMessage>,
    writer: Receiver<(Address, BlobResponse)>,
}

impl PendingBlobCache {
    pub fn new(
        da_actor: ActorRef<DaClientMessage>,
        eo_actor: ActorRef<EoMessage>,
        writer: Receiver<(Address, BlobResponse)>
    ) -> Self {
        let queue = HashMap::new();
        let receivers = FuturesUnordered::new();
        Self { queue, receivers, da_actor, eo_actor, writer }
    }

    fn handle_queue_removal(
        &mut self, 
        address: Address, 
        proof: BlobVerificationProof
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.queue.remove(&address);
        let batch_header_hash = proof.batch_metadata().batch_header_hash();
        let blob_index = proof.blob_index();
        let res = self.eo_actor.cast(
            EoMessage::Settle { 
                address, 
                batch_header_hash: batch_header_hash.to_string(), 
                blob_index 
            }
        );

        if let Err(e) = res {
            log::error!("Encountered error trying to inform EO Blob is ready for settlement: {:?}", e);
        }

        let res = self.da_actor.cast(
            DaClientMessage::RetrieveBlob { 
                batch_header_hash: batch_header_hash.to_string(), 
                blob_index
            }
        );

        if let Err(e) = res {
            log::error!("Encountered error trying to inform DaClient Message to retrieve blob: {:?}", e);
        }

        Ok(())
    }

    fn handle_queue_write(
        &mut self,
        address: Address, 
        response: BlobResponse
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(entry) = self.queue.get_mut(&address) {
            *entry = response.clone();
        } else {
            self.queue.insert(address.clone(), response.clone());
        }
        let (tx, rx) = oneshot();
        self.receivers.push(rx);
        let res = self.da_actor.cast(
            DaClientMessage::ValidateBlob { 
                request_id: response.request_id(),
                address,
                tx
            }
        );
        if let Err(e) = res {        
            log::error!("Encountered error attempting to ask DA Client to validated Blob: {}", e);
        }
        Ok(())
    }

    pub async fn run(
        mut self, 
        mut stop: OneshotReceiver<u8>
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        while let Err(_) = stop.try_recv() {
            tokio::select! {
                res = self.receivers.next() => {
                    match res {
                        Some(Ok((address, resp))) => {
                            self.handle_queue_removal(address, resp);
                        }
                        _ => {}
                    }
                },
                write = self.writer.recv() => {
                    match write {
                        Some((address, blob_response)) => {
                            self.handle_queue_write(address, blob_response);
                        }
                        _ => {}
                    }
                },
            }

        }
        Ok(())
    }
}

impl AccountCache {
    pub fn new(
    ) -> Self {
        Self {
            cache: HashMap::new(),
            receivers: FuturesUnordered::new(),
        }
    }

    pub(crate) fn get(
        &self,
        address: &Address
    ) -> Option<&Account> {
        if let Some(account) = self.cache.get(address) {
            return Some(account)
        }
        return None
    }

    pub(crate) fn remove(
        &mut self,
        address: &Address
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.cache.remove(address);
        Ok(())
    }

    pub(crate) fn update(
        &mut self,
        address: &Address,
        delta: &TokenDelta
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(mut entry) = self.cache.get_mut(address) {
            entry.update_programs(address, delta);
        } 

        Ok(())
    }

    pub(crate) fn handle_cache_write(
        &mut self,
        account: Account
    ) -> Result<(), Box<dyn std::error::Error>> {
        let address = account.address(); 
        // if let Some(mut entry) = self.cache.get_mut(&address) {
        //    log::info!("Found account: 0x{:x} in cache, updating...", &account.address());
        //    *entry = account;
        //} else {
        self.cache.insert(address, account);
        //}
        
        Ok(())
    }

    pub(crate) fn handle_cache_removal(
        &mut self,
        address: &Address
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.cache.remove(address);
        Ok(())
    }

    pub(crate) fn handle_cache_check(
        &self,
        address: &Address,
        response: OneshotSender<Option<Account>>
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(account) = self.cache.get(address) {
            response.send(Some(account.clone()));
        } else {
            response.send(None);
        }
        Ok(())
    }
}

impl AccountCacheActor {
    pub fn new() -> Self {
        Self 
    }
}

#[async_trait]
impl Actor for AccountCacheActor {
    type Msg = AccountCacheMessage;
    type State = AccountCache; 
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(AccountCache::new()) 
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            AccountCacheMessage::Write { account } => {
                log::info!("Received account cache write request");
                state.handle_cache_write(account);
            }
            AccountCacheMessage::Read { address, tx } => {
                log::info!("Recieved account cache read request"); 
                let account = state.get(&address);
                log::info!("Account: {:?}", &account);
                tx.send(account.cloned());
            }
            AccountCacheMessage::Remove { address } => {
                state.remove(&address);
            }
            AccountCacheMessage::Update { address, delta } => {
                state.update(&address, &delta);
            }
        }
        Ok(())
    }
}

impl PendingTransactions {
    fn handle_new_pending(
        &mut self,
        address: Address,
        token: Token,
        tx: OneshotSender<(Address, Address)>
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(mut entry) = self.pending.get_mut(&address) {
            entry.insert(token, tx);
            return Ok(())
        }
        let pending_token = PendingTokens::new(token, tx); 
        Ok(())
    }

    fn handle_confirmed(
        &mut self, 
        address: Address,
        token: Token
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut remove: bool = false;
        if let Some(pending) = self.pending.get_mut(&address) {
            if let Some(senders) = pending.get(&token) {
                let sender = senders.remove(0);
                sender.send((address, token.program_id()));
                if senders.len() == 0 {
                    remove = true;
                }
            }

            if remove {
                pending.remove(&token);
            }
        }

        Ok(())
    }

    pub async fn run(
        mut self,
        mut stop: OneshotReceiver<u8>
    ) -> Result<(), Box<dyn std::error::Error>> {
        log::info!("Starting Pending Transaction Cache");
        while let Err(_) = stop.try_recv() {
            tokio::select! {
                res = self.receivers.next() => {
                    match res {
                        Some(Ok((address, token))) => {
                            self.handle_confirmed(address, token);
                        },
                        _ => {}
                    }
                }

                write = self.writer.recv() => {
                    match write {
                        Some((address, token, tx)) => {
                            self.handle_new_pending(address, token, tx);
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }
}
