use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use ractor::{concurrency::{OneshotReceiver, OneshotSender}, Actor, ActorRef, ActorProcessingErr};
use thiserror::Error;
use std::{collections::HashMap, fmt::Display};
use crate::{Address, Account, AccountCacheMessage, TokenDelta};

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

#[allow(unused)]
#[derive(Debug)]
pub struct AccountCache {
    cache: HashMap<Address, Account>,
    receivers: FuturesUnordered<OneshotReceiver<Address>>,
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
        if let Some(entry) = self.cache.get_mut(address) {
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

    #[allow(unused)]
    pub(crate) fn handle_cache_removal(
        &mut self,
        address: &Address
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.cache.remove(address);
        Ok(())
    }

    #[allow(unused)]
    pub(crate) fn handle_cache_check(
        &self,
        address: &Address,
        response: OneshotSender<Option<Account>>
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(account) = self.cache.get(address) {
            let _ = response.send(Some(account.clone()));
        } else {
            let _ = response.send(None);
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
                let _ = state.handle_cache_write(account);
            }
            AccountCacheMessage::Read { address, tx } => {
                log::info!("Recieved account cache read request"); 
                let account = state.get(&address);
                log::info!("Account: {:?}", &account);
                let _ = tx.send(account.cloned());
            }
            AccountCacheMessage::Remove { address } => {
                let _ = state.remove(&address);
            }
            AccountCacheMessage::Update { address, delta } => {
                let _ = state.update(&address, &delta);
            }
        }
        Ok(())
    }
}
