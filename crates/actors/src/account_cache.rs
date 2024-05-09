use crate::{Coerce, MAX_BATCH_SIZE};
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use lasr_messages::{
    AccountCacheMessage, ActorName, ActorType, RpcMessage, RpcResponseError, SupervisorType,
    TransactionResponse,
};
use lasr_types::{Account, AccountType, Address};
use ractor::{
    concurrency::OneshotReceiver, Actor, ActorCell, ActorProcessingErr, ActorRef, SupervisionEvent,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone, Default)]
pub struct AccountCacheActor;

impl ActorName for AccountCacheActor {
    fn name(&self) -> ractor::ActorName {
        ActorType::AccountCache.to_string()
    }
}

#[derive(Debug, Clone, Error)]
pub enum AccountCacheError {
    #[error("failed to acquire AccountCacheActor from registry")]
    RactorRegistryError,

    #[error("failed to acquire account data from cache for address {}", addr.to_full_string())]
    FailedAccountAcquisition { addr: Address },

    #[error("{0}")]
    Custom(String),
}

impl Default for AccountCacheError {
    fn default() -> Self {
        AccountCacheError::RactorRegistryError
    }
}

#[allow(unused)]
#[derive(Debug, Default)]
pub struct AccountCache {
    cache: HashMap<Address, Account>,
    receivers: FuturesUnordered<OneshotReceiver<Address>>,
    batch_interval: Duration,
    last_batch: Option<Instant>,
}

impl AccountCache {
    pub fn new() -> Self {
        let batch_interval_secs = std::env::var("BATCH_INTERVAL")
            .unwrap_or_else(|_| "180".to_string())
            .parse::<u64>()
            .unwrap_or(180);
        Self {
            cache: HashMap::new(),
            receivers: FuturesUnordered::new(),
            batch_interval: Duration::from_secs(batch_interval_secs),
            last_batch: None,
        }
    }

    pub(crate) fn get(&self, address: &Address) -> Option<&Account> {
        if let Some(account) = self.cache.get(address) {
            return Some(account);
        }
        None
    }

    pub(crate) fn remove(
        &mut self,
        address: &Address,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        self.cache.remove(address);
        Ok(())
    }

    pub(crate) fn update(
        &mut self,
        account: Account,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        let addr = account.owner_address();
        if let Some(a) = self.cache.get_mut(&addr) {
            *a = account;
            return Ok(());
        }

        Err(
            Box::new(AccountCacheError::FailedAccountAcquisition { addr })
                as Box<dyn std::error::Error + Send>,
        )
    }

    pub(crate) fn handle_cache_write(
        &mut self,
        account: Account,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        match account.account_type() {
            AccountType::User => {
                let address = account.owner_address();
                if let Some(entry) = self.cache.get_mut(&address) {
                    log::info!("Found account: 0x{:x} in cache, updating...", &address);
                    *entry = account;
                } else {
                    log::info!(
                        "Did not find account: 0x{:x} in cache, inserting...",
                        &address
                    );
                    self.cache.insert(address, account);
                    log::info!(
                        "Inserted account: 0x{:x} in cache, cache.len(): {}",
                        &address,
                        self.cache.len()
                    );
                }
            }
            AccountType::Program(program_address) => {
                if let Some(entry) = self.cache.get_mut(&program_address) {
                    log::info!(
                        "Found program_account: 0x{:x} in cache, updating...",
                        &program_address
                    );
                    *entry = account;
                } else {
                    log::info!(
                        "Did not find account: 0x{:x} in cache, inserting...",
                        &program_address
                    );
                    self.cache.insert(program_address, account);
                    log::info!(
                        "Inserted account: 0x{:x} in cache, cache.len(): {}",
                        &program_address,
                        self.cache.len()
                    );
                }
            }
        }

        self.check_build_batch()?;

        Ok(())
    }

    fn check_build_batch(&mut self) -> Result<(), Box<dyn std::error::Error + Send>> {
        let bytes = serde_json::to_vec(&self.cache)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
        if base64::encode(bytes).len() >= MAX_BATCH_SIZE {
            self.build_batch()?;
        } else if let Some(instant) = self.last_batch {
            if Instant::now().duration_since(instant) > self.batch_interval {
                self.build_batch()?;
            }
        } else if self.last_batch.is_none() {
            self.last_batch = Some(Instant::now());
        }

        Ok(())
    }

    fn build_batch(&self) -> Result<(), Box<dyn std::error::Error + Send>> {
        log::info!("Time to build a batch and settle it");
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
                log::info!(
                    "Received account cache write request: 0x{:x}",
                    &account.owner_address()
                );
                let _ = state.handle_cache_write(account.clone());
                log::info!("Account: {:?}", &account);
            }
            AccountCacheMessage::Read { address, tx } => {
                log::info!(
                    "Recieved account cache read request for account: {:?}",
                    &address
                );
                let account = state.get(&address);
                log::info!("acquired account option: {:?}", &account);
                let _ = tx.send(account.cloned());
            }
            AccountCacheMessage::Remove { address } => {
                let _ = state.remove(&address);
            }
            AccountCacheMessage::Update { account } => {
                if let Err(_e) = state.update(account.clone()) {
                    let _ = state.handle_cache_write(account.clone());
                }
            }
            AccountCacheMessage::TryGetAccount { address, reply } => {
                if let Some(account) = state.get(&address) {
                    let _ = reply.send(RpcMessage::Response {
                        response: Ok(TransactionResponse::GetAccountResponse(account.clone())),
                        reply: None,
                    });
                } else {
                    // Pass along to EO
                    // EO passes along to DA if Account Blob discovered
                    let response = Err(RpcResponseError {
                        description: "Unable to acquire account from DA or Protocol Cache"
                            .to_string(),
                    });
                    let _ = reply.send(RpcMessage::Response {
                        response,
                        reply: None,
                    });
                }
            }
        }
        Ok(())
    }
}

pub struct AccountCacheSupervisor {
    panic_tx: Sender<ActorCell>,
}
impl AccountCacheSupervisor {
    pub fn new(panic_tx: Sender<ActorCell>) -> Self {
        Self { panic_tx }
    }
}
impl ActorName for AccountCacheSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::AccountCache.to_string()
    }
}
#[derive(Debug, Error, Default)]
pub enum AccountCacheSupervisorError {
    #[default]
    #[error("failed to acquire AccountCacheSupervisor from registry")]
    RactorRegistryError,
}

#[async_trait]
impl Actor for AccountCacheSupervisor {
    type Msg = AccountCacheMessage;
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
