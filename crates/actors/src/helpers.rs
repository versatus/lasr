use std::sync::Arc;
use std::time::Duration;

use crate::{AccountCacheError, EngineError};
use ethereum_types::H256;
use futures::future::BoxFuture;
use futures::stream::{FuturesOrdered, FuturesUnordered};
use lasr_messages::{AccountCacheMessage, ActorType, DaClientMessage, EoMessage};
use lasr_types::{Account, Address};
use ractor::concurrency::{oneshot, OneshotReceiver};
use ractor::ActorRef;
use tokio::{sync::Mutex, time::timeout};

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
    fn spawn_future_handler(actor: Self, future_handler: Self::FutureHandler) -> Self::JoinHandle;
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
            None => Err(Box::new(AccountCacheError) as Box<dyn std::error::Error>),
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

    let blob_index = {
        match timeout(
            Duration::from_secs(5),
            attempt_get_blob_index(eo_actor, address, Duration::from_secs(3)),
        )
        .await
        {
            Ok(Some(blob_index)) => blob_index,
            Ok(None) => return None,
            Err(e) => {
                log::error!("Error in attempting to get blob_index: {e}");
                return None;
            }
        }
    };

    match timeout(
        Duration::from_secs(5),
        attempt_get_account_from_da(da_actor, address, blob_index),
    )
    .await
    {
        Ok(Some(account)) => Some(account),
        Ok(None) => None,
        Err(e) => {
            log::error!("Error attempting to get account from DA: {e}");
            None
        }
    }
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
    max_duration: Duration,
) -> Option<(Address, H256, u128)> {
    let start_time = std::time::Instant::now();
    loop {
        let (tx, rx) = oneshot();
        let message = EoMessage::GetAccountBlobIndex {
            address,
            sender: tx,
        };
        if let Some(blob_index) = get_blob_index(eo_actor.clone(), message, rx).await {
            return Some(blob_index);
        }

        if start_time.elapsed() >= max_duration {
            return None;
        }
    }
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

    return None;
}
