use crate::{helpers::Coerce, process_group_changed, AccountValue, MAX_BATCH_SIZE};
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use lasr_messages::{
    AccountCacheMessage, ActorName, ActorType, RpcMessage, RpcResponseError, SupervisorType,
    TransactionResponse,
};
#[cfg(feature = "mock_storage")]
use lasr_types::MockPersistenceStore;
use lasr_types::{Account, AccountType, Address, PersistenceStore};
use ractor::{
    concurrency::OneshotReceiver, Actor, ActorCell, ActorProcessingErr, ActorRef, SupervisionEvent,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use thiserror::Error;
#[cfg(not(feature = "mock_storage"))]
use tikv_client::RawClient as TikvClient;
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone, Default)]
pub struct AccountCacheActor;

pub type StorageRef = <AccountCacheActor as Actor>::Arguments;

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

pub struct AccountCache<S: PersistenceStore> {
    inner: AccountCacheInner,
    storage: S,
}
impl<S: PersistenceStore> AccountCache<S> {
    pub fn new(storage: S) -> Self {
        Self {
            inner: AccountCacheInner::new(),
            storage,
        }
    }
}

#[allow(unused)]
#[derive(Debug, Default)]
pub struct AccountCacheInner {
    cache: HashMap<Address, Account>,
    receivers: FuturesUnordered<OneshotReceiver<Address>>,
    batch_interval: Duration,
    last_batch: Option<Instant>,
}

impl AccountCacheInner {
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
                    tracing::info!("Found account: 0x{:x} in cache, updating...", &address);
                    *entry = account;
                } else {
                    tracing::info!(
                        "Did not find account: 0x{:x} in cache, inserting...",
                        &address
                    );
                    self.cache.insert(address, account);
                    tracing::info!(
                        "Inserted account: 0x{:x} in cache, cache.len(): {}",
                        &address,
                        self.cache.len()
                    );
                }
            }
            AccountType::Program(program_address) => {
                if let Some(entry) = self.cache.get_mut(&program_address) {
                    tracing::info!(
                        "Found program_account: 0x{:x} in cache, updating...",
                        &program_address
                    );
                    *entry = account;
                } else {
                    tracing::info!(
                        "Did not find account: 0x{:x} in cache, inserting...",
                        &program_address
                    );
                    self.cache.insert(program_address, account);
                    tracing::info!(
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
        tracing::info!("Time to build a batch and settle it");
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
    type State = AccountCache<Self::Arguments>;
    #[cfg(not(feature = "mock_storage"))]
    type Arguments = TikvClient;
    #[cfg(feature = "mock_storage")]
    type Arguments = MockPersistenceStore<String, Vec<u8>>;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(AccountCache::new(args))
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            AccountCacheMessage::Write {
                account,
                who,
                location,
            } => {
                let owner = &account.owner_address().to_full_string();
                tracing::warn!(
                    "Received account cache write request from {} for address {}: WHERE: {}",
                    who.to_string(),
                    owner,
                    location
                );
                let _ = state.inner.handle_cache_write(account.clone());
                tracing::info!("Account written to for address {owner}: {:?}", &account);
            }
            AccountCacheMessage::Read { address, tx, who } => {
                let hex_address = &address.to_full_string();
                tracing::warn!(
                    "Recieved account cache read request from {} for address: {}",
                    who.to_string(),
                    hex_address
                );
                let account = if let Some(account) = state.inner.get(&address) {
                    tracing::warn!("retrieved account from account cache for address {hex_address}: {account:?}");
                    Some(account.clone())
                } else {
                    // Pass to persistence store
                    tracing::warn!(
                        "Account not found in AccountCache for address {hex_address}, connecting to persistence store."
                    );
                    let acc_key = address.to_full_string();

                    // Pull `Account` data from persistence store
                    PersistenceStore::get(
                        &state.storage,
                        acc_key.to_owned().into()
                    )
                    .await
                    .typecast()
                    .log_err(|e| AccountCacheError::Custom(format!("failed to find Account with address: {hex_address} in persistence store: {e:?}")))
                    .flatten()
                    .and_then(|returned_data| {
                        bincode::deserialize(&returned_data)
                            .typecast()
                            .log_err(|e| e)
                            .and_then(|AccountValue { account }| {
                                tracing::debug!("retrieved account from persistence store for address {hex_address}: {account:?}");
                                Some(account)
                            })
                    })
                };
                let _ = tx.send(account);
            }
            AccountCacheMessage::Remove { address } => {
                let _ = state.inner.remove(&address);
            }
            AccountCacheMessage::Update { account } => {
                if let Err(_e) = state.inner.update(account.clone()) {
                    let _ = state.inner.handle_cache_write(account.clone());
                }
            }
            AccountCacheMessage::TryGetAccount { address, reply } => {
                if let Some(account) = state.inner.get(&address) {
                    let _ = reply.send(RpcMessage::Response {
                        response: Ok(TransactionResponse::GetAccountResponse(account.clone())),
                        reply: None,
                    });
                } else {
                    let response = Err(RpcResponseError {
                        description:
                            "Unable to acquire account from Persistence Store or Protocol Cache"
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
        tracing::warn!("Received a supervision event: {:?}", message);
        match message {
            SupervisionEvent::ActorStarted(actor) => {
                tracing::info!(
                    "actor started: {:?}, status: {:?}",
                    actor.get_name(),
                    actor.get_status()
                );
            }
            SupervisionEvent::ActorPanicked(who, reason) => {
                tracing::error!("actor panicked: {:?}, err: {:?}", who.get_name(), reason);
                self.panic_tx.send(who).await.typecast().log_err(|e| e);
            }
            SupervisionEvent::ActorTerminated(who, _, reason) => {
                tracing::error!("actor terminated: {:?}, err: {:?}", who.get_name(), reason);
            }
            SupervisionEvent::PidLifecycleEvent(event) => {
                tracing::info!("pid lifecycle event: {:?}", event);
            }
            SupervisionEvent::ProcessGroupChanged(m) => {
                process_group_changed(m);
            }
        }
        Ok(())
    }
}
