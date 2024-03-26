use std::time::Duration;

use crate::{AccountCacheError, EngineError};
use ethereum_types::H256;
use lasr_messages::{AccountCacheMessage, ActorType, DaClientMessage, EoMessage};
use lasr_types::{Account, Address};
use ractor::concurrency::{oneshot, OneshotReceiver};
use ractor::ActorRef;
use tokio::time::timeout;

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
            let resp = response.map_err(|e| channel_closed_unexpectedly(e))?;
            handler(resp)
        }
    }
}

pub async fn check_account_cache(address: Address) -> Option<Account> {
    let actor: ActorRef<AccountCacheMessage> =
        ractor::registry::where_is(ActorType::AccountCache.to_string())?.into();

    let (tx, rx) = oneshot();
    let message = AccountCacheMessage::Read { address, tx };

    let _ = actor.cast(message).ok()?;

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
        attempt_get_account_from_da(da_actor, address, Duration::from_secs(3), blob_index),
    )
    .await
    {
        Ok(Some(account)) => return Some(account),
        Ok(None) => return None,
        Err(e) => {
            log::error!("Error attempting to get account from DA: {e}");
            return None;
        }
    }
}

pub async fn get_account(address: Address) -> Option<Account> {
    log::info!(
        "checking account cache for account: {} using `get_account` method in mod.rs",
        address.to_full_string()
    );
    let mut account = check_account_cache(address).await;
    if let None = &mut account {
        account = check_da_for_account(address).await;
    }
    return account;
}

pub async fn get_blob_index(
    eo_actor: ActorRef<EoMessage>,
    message: EoMessage,
    rx: OneshotReceiver<EoMessage>,
) -> Option<(Address, H256, u128)> {
    let _ = eo_actor.cast(message).ok()?;
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
    max_duration: Duration,
    blob_index: (Address, H256, u128),
) -> Option<Account> {
    let start_time = std::time::Instant::now();
    loop {
        let (tx, rx) = oneshot();
        let message = DaClientMessage::RetrieveAccount {
            address,
            batch_header_hash: blob_index.1.into(),
            blob_index: blob_index.2,
            tx,
        };

        if let Some(account) = get_account_from_da(da_actor.clone(), message, rx).await {
            return Some(account);
        }

        if start_time.elapsed() >= max_duration {
            return None;
        }
    }
}
