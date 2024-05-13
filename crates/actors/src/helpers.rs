use std::error::Error as StdError;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use crate::{AccountCacheError, EngineError};
use ethereum_types::H256;
use futures::future::BoxFuture;
use futures::stream::{FuturesOrdered, FuturesUnordered};
use lasr_messages::{AccountCacheMessage, ActorType, DaClientMessage, EoMessage};
use lasr_types::{Account, Address};
use ractor::concurrency::{oneshot, OneshotReceiver};
use ractor::pg::GroupChangeMessage;
use ractor::ActorRef;
use tokio::sync::Mutex;

/// A thread-safe, non-blocking & mutatable `FuturesUnordered`.
pub type UnorderedFuturePool<F> = Arc<Mutex<FuturesUnordered<F>>>;
/// A thread-safe, non-blocking & mutatable `FuturesOrdered`.
pub type OrderedFuturePool<F> = Arc<Mutex<FuturesOrdered<F>>>;
/// Equivalent to `Pin<Box<dyn Future<Output = O> + Send + 'static>>`.
///
/// Shorthand for needing to specify the `'static` lifetime for `BoxFuture` all the time
/// for types that house `Futures` that will be forwarded to a thread to be `await`ed`.
pub type StaticFuture<O> = BoxFuture<'static, O>;

/// An extension trait for `Actor`s that aids in forwarding `Future`s from
/// `Actor::handle` to a `FuturePool` to be `await`ed at a later time,
/// unblocking the `handle` method for that `Actor`.
pub trait ActorExt: ractor::Actor {
    /// Represents the `Output` of a `Future` (`Future<Output = T>`).
    type Output;
    /// The type of `Future` to be forwarded by `Self::FuturePool`.
    type Future<O>;
    /// The type of pool that will store forwarded `Future`s.
    ///
    /// Example:
    /// ```rust,ignore
    /// use lasr_actors::helpers::{OrderedFuturePool, StaticFuture, ActorExt};
    ///
    /// impl ActorExt for MyActor {
    ///     type Output = ();
    ///     type Future<O> = StaticFuture<Self::Output>;
    ///     type FuturePool<F> = OrderedFuturePool<Self::Future<Self::Output>>;
    ///     // ...
    /// }
    /// ```
    type FuturePool<F>;
    /// The thread(s) that will `await` `Future`s passed to it from `Self::FuturePool`.
    type FutureHandler;
    /// The handle of the thread responsible for passing `Future`s to `Self::FutureHandler`.
    type JoinHandle;

    /// Returns a thread-safe, non-blocking mutatable future pool for polling
    /// futures in a future handler thread.
    ///
    /// > Note: If using `UnorderedFuturePool`, or `OrderedFuturePool` this will
    /// likely need to be an `Arc::clone`, incrementing the atomic reference
    /// count by 1.
    fn future_pool(&self) -> Self::FuturePool<Self::Future<Self::Output>>;

    /// Spawn a thread that passes futures to a thread pool to await them there.
    ///
    /// This method assumes the thread will take ownership of the thread pool, so this method
    /// may not be convenient if the thread pool needs to be shared between actors.
    fn spawn_future_handler(actor: Self, future_handler: Self::FutureHandler) -> Self::JoinHandle;
}

/// Takes any type implementing `ToString` for the actor name to search the `ractor::registry`,
/// the message type for the `ActorRef` to coerce the `ActorCell` into, and the error variant of the actor
/// and returns an `Option<ActorRef>`. This logs the default actor error in the event an actor cannot
/// be acquired from the registry.
pub fn get_actor_ref<M: Sized, E: Default + StdError + Debug>(
    actor_name: impl ToString,
) -> Option<ActorRef<M>> {
    ractor::registry::where_is(actor_name.to_string())
        .ok_or(E::default())
        .typecast()
        .log_err(|e| e)
        .and_then(|a| Some(a.into()))
}

/// Wrapper type for `std::result::Result` with emphasis on `ractor::Actor` friendliness.
/// This result type does not panic, but can be cast back into a `std::result::Result`
/// for convenience of access to methods already available for `std::result::Result`.
pub struct ActorResult<T, E: Debug>(Result<T, E>);
impl<T, E: Debug> ActorResult<T, E> {
    /// Maps a `ActorResult<T, E>` to `Option<T>` by applying a function to a
    /// contained [`Err`] value, leaving an [`Ok`] value untouched.
    ///
    /// This function can be used to pass through a successful result while handling
    /// an error via logging. It's important to note that the resulting type returned
    /// from the applied function closure must impement `std::fmt::Debug`.
    ///
    ///
    /// # Examples
    ///
    /// ```
    /// use lasr_actors::helpers::{ActorResult, Coerce};
    /// use thiserror::Error;
    ///
    /// #[derive(Error, Debug)]
    /// enum MyError {
    ///     #[error("failed because: {reason}")]
    ///     Failed { reason: String },
    /// }
    ///
    /// let x: Result<u32, std::fmt::Error> = Ok(2);
    /// assert_eq!(x.typecast().log_err(|e| MyError::Failed { reason: e.to_string() }), Some(2));
    ///
    /// // In the `Err` case the error is logged to the console, and `None` is returned:
    /// let x: Result<u32, std::fmt::Error> = Err(std::fmt::Error);
    /// assert_eq!(x.typecast().log_err(|e| MyError::Failed { reason: e.to_string() }), None);
    /// ```
    pub fn log_err<F: Debug, O: FnOnce(E) -> F>(self, op: O) -> Option<T> {
        match self.into() {
            Ok(t) => Some(t),
            Err(e) => {
                log::error!("{:?}", op(e));
                None
            }
        }
    }
}

/// A convenience trait for coercing one type into another via type hint.
///
/// This trait works synergistically with `From` and `Into` implementations,
/// reducing cases of `let` bindings purely for type ascriptions.
pub trait Coerce {
    type Type;
    fn typecast(self) -> Self::Type;
}
impl<T, E: Debug> Coerce for ActorResult<T, E> {
    type Type = Result<T, E>;
    fn typecast(self) -> Self::Type {
        self.into()
    }
}
impl<T, E: Debug> Coerce for Result<T, E> {
    type Type = ActorResult<T, E>;
    fn typecast(self) -> Self::Type {
        self.into()
    }
}
impl<T, E: Debug> From<Result<T, E>> for ActorResult<T, E> {
    fn from(value: Result<T, E>) -> Self {
        Self(value)
    }
}
impl<T, E: Debug> From<ActorResult<T, E>> for Result<T, E> {
    fn from(value: ActorResult<T, E>) -> Result<T, E> {
        value.0
    }
}

pub fn process_group_changed(group_change_message: GroupChangeMessage) {
    match group_change_message {
        GroupChangeMessage::Join(scope, group, actors) => {
            log::warn!("actor(s) {actors:?} have joined group {group:?} with scope {scope:?}")
        }
        GroupChangeMessage::Leave(scope, group, actors) => {
            log::warn!("actor(s) {actors:?} have left group {group:?} with scope {scope:?}")
        }
    }
}

#[macro_export]
macro_rules! create_handler {
    (rpc_response, call) => {
        |resp| match resp {
            RpcMessage::Response { response, .. } => match response {
                Ok(resp) => return Ok(resp),
                _ => {
                    return Err(Box::new(RpcError::Custom(
                        "Received an invalid type in response to RPC `call` method".to_string(),
                    )) as Box<dyn std::error::Error>);
                }
            },
            _ => {
                return Err(Box::new(RpcError::Custom(
                    "Received an invalid type in response to RPC `call` method".to_string(),
                )) as Box<dyn std::error::Error>);
            }
        }
    };

    (rpc_response, send) => {
        |resp| match resp {
            RpcMessage::Response { response, .. } => match response {
                Ok(resp) => {
                    return Ok(resp);
                }
                _ => {
                    return Err(Box::new(RpcError::Custom(
                        "Received an invalid type in response to RPC `send` method".to_string(),
                    )) as Box<dyn std::error::Error>);
                }
            },
            _ => {
                return Err(Box::new(RpcError::Custom(
                    "Received an invalid type in response to RPC `send` method".to_string(),
                )) as Box<dyn std::error::Error>);
            }
        }
    };

    (rpc_response, registerProgram) => {
        |resp| match resp {
            RpcMessage::Response { response, .. } => match response {
                Ok(resp) => return Ok(resp),

                _ => {
                    return Err(Box::new(RpcError::Custom(
                        "Received an invalid type in response to RPC `registerProgram` method"
                            .to_string(),
                    )) as Box<dyn std::error::Error>);
                }
            },
            _ => {
                return Err(Box::new(RpcError::Custom(
                    "Received an invalid type in response to RPC `registerProgram` method"
                        .to_string(),
                )) as Box<dyn std::error::Error>);
            }
        }
    };

    (rpc_response, getAccount) => {
        |resp| match resp {
            RpcMessage::Response { response, .. } => match response {
                Ok(TransactionResponse::GetAccountResponse(account)) => {
                    return Ok(TransactionResponse::GetAccountResponse(account))
                }
                _ => {
                    return Err(Box::new(RpcError::Custom(
                        "received an invalid type in response to RPC `getAccount` method"
                            .to_string(),
                    )) as Box<dyn std::error::Error>);
                }
            },
            _ => {
                return Err(Box::new(RpcError::Custom(
                    "Received an invalid type in response to RPC `getAccount` method".to_string(),
                )) as Box<dyn std::error::Error>)
            }
        }
    };

    (engine_response, call) => {
        |resp| match resp {}
    };

    (engine_response, send) => {
        |resp| match resp {}
    };

    (engine_response, registerProgram) => {
        |resp| match resp {}
    };

    (retrieve_blob) => {
        |resp| match resp {
            Some(account) => Ok(Some(account)),
            _ => Ok(None),
        }
    };

    (retrieve_blob_index) => {
        |resp| match resp {
            EoMessage::AccountBlobIndexAcquired {
                address,
                batch_header_hash,
                blob_index,
            } => Ok((address, batch_header_hash, blob_index)),
            EoMessage::ContractBlobIndexAcquired {
                program_id,
                batch_header_hash,
                blob_index,
            } => Ok((program_id, batch_header_hash, blob_index)),
            EoMessage::AccountBlobIndexNotFound { address } => Err(Box::new(EngineError::Custom(
                format!("unable to acquire blob index for account: {:?}", address),
            ))
                as Box<dyn std::error::Error>),
            EoMessage::ContractBlobIndexNotFound { program_id } => {
                Err(Box::new(EngineError::Custom(format!(
                    "unable to acquire blob index for program_id: {:?}",
                    program_id
                ))) as Box<dyn std::error::Error>)
            }
            _ => Err(
                Box::new(EngineError::Custom("invalid response received".to_string()))
                    as Box<dyn std::error::Error>,
            ),
        }
    };

    (account_cache_response) => {
        |resp| match resp {
            Some(account) => Ok(account),
            None => Err(Box::new(AccountCacheError::Custom(
                "no account found in account cache response from handler".to_string(),
            )) as Box<dyn std::error::Error>),
        }
    };
}

fn channel_closed_unexpectedly<E: std::error::Error + 'static>(
    e: E,
) -> Box<dyn std::error::Error + 'static> {
    Box::new(e)
}

pub async fn handle_actor_response<T, F, M>(
    rx: OneshotReceiver<T>,
    handler: F,
) -> Result<M, Box<dyn std::error::Error>>
where
    F: FnOnce(T) -> Result<M, Box<dyn std::error::Error>>,
{
    tokio::select! {
        response = rx => {
            let resp = response.map_err(channel_closed_unexpectedly)?;
            handler(resp)
        }
        _ = tokio::time::sleep(Duration::from_secs(15)) => {
            Err(
                Box::new(
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "rpc request timed out, this does not mean your request failed"
                    )
                )
            )
        }
    }
}

pub async fn check_account_cache(address: Address) -> Option<Account> {
    let actor: ActorRef<AccountCacheMessage> =
        ractor::registry::where_is(ActorType::AccountCache.to_string())?.into();

    let (tx, rx) = oneshot();
    let message = AccountCacheMessage::Read { address, tx };

    actor.cast(message).ok()?;

    let handler = create_handler!(account_cache_response);
    let account = handle_actor_response(rx, handler).await.ok()?;

    Some(account)
}

pub async fn check_da_for_account(address: Address) -> Option<Account> {
    log::warn!("checking DA for account");
    let eo_actor: ActorRef<EoMessage> =
        ractor::registry::where_is(ActorType::EoClient.to_string())?.into();

    let da_actor: ActorRef<DaClientMessage> =
        ractor::registry::where_is(ActorType::DaClient.to_string())?.into();

    let blob_index = match attempt_get_blob_index(eo_actor, address).await {
        Some(blob_index) => blob_index,
        None => {
            log::error!(
                "failed to acquire blob index from EO for address: {}",
                address.to_full_string()
            );
            return None;
        }
    };

    attempt_get_account_from_da(da_actor, address, blob_index).await
}

pub async fn get_account(address: Address) -> Option<Account> {
    log::info!(
        "checking account cache for account: {} using `get_account` method in mod.rs",
        address.to_full_string()
    );
    let mut account = check_account_cache(address).await;
    if account.is_none() {
        account = check_da_for_account(address).await;
    }
    account
}

pub async fn get_blob_index(
    eo_actor: ActorRef<EoMessage>,
    message: EoMessage,
    rx: OneshotReceiver<EoMessage>,
) -> Option<(Address, H256, u128)> {
    eo_actor.cast(message).ok()?;
    let eo_handler = create_handler!(retrieve_blob_index);
    handle_actor_response(rx, eo_handler).await.ok()
}

pub async fn attempt_get_blob_index(
    eo_actor: ActorRef<EoMessage>,
    address: Address,
) -> Option<(Address, H256, u128)> {
    let (tx, rx) = oneshot();
    let message = EoMessage::GetAccountBlobIndex {
        address,
        sender: tx,
    };
    get_blob_index(eo_actor.clone(), message, rx).await
}

pub async fn get_account_from_da(
    da_actor: ActorRef<DaClientMessage>,
    message: DaClientMessage,
    rx: OneshotReceiver<Option<Account>>,
) -> Option<Account> {
    da_actor.cast(message).ok()?;
    let da_handler = create_handler!(retrieve_blob);
    handle_actor_response(rx, da_handler).await.ok()?
}

pub async fn attempt_get_account_from_da(
    da_actor: ActorRef<DaClientMessage>,
    address: Address,
    blob_index: (Address, H256, u128),
) -> Option<Account> {
    let (tx, rx) = oneshot();
    let message = DaClientMessage::RetrieveAccount {
        address,
        batch_header_hash: blob_index.1,
        blob_index: blob_index.2,
        tx,
    };

    if let Some(account) = get_account_from_da(da_actor.clone(), message, rx).await {
        return Some(account);
    }

    None
}
