use ractor::ActorRef;
use ractor::concurrency::{OneshotReceiver, oneshot};
use lasr_types::{Address, Account};
use lasr_messages::{
    DaClientMessage, 
    ActorType, 
    EoMessage, 
    AccountCacheMessage
};
use crate::{
    EngineError, 
    AccountCacheError,
};

#[macro_export]
macro_rules! create_handler {
    (rpc_response, call) => {
        |resp| {
            match resp {
                RpcMessage::Response { response, .. } => {
                    match response {
                        Ok(resp) => return Ok(resp),
                        _ => {
                            return Err(Box::new(RpcError::Custom(
                                "Received an invalid type in response to RPC `call` method".to_string()
                            )) as Box<dyn std::error::Error>);
                        }
                    }
                }
                _ => {
                    return Err(Box::new(RpcError::Custom(
                        "Received an invalid type in response to RPC `call` method".to_string()
                    )) as Box<dyn std::error::Error>);
                }
            }
        }
    };

    (rpc_response, send) => {
        |resp| {
            match resp {
                RpcMessage::Response { response, .. } => {
                    match response {
                        Ok(resp) => {
                            return Ok(resp);
                        }
                        _ => {
                            return Err(Box::new(RpcError::Custom(
                                "Received an invalid type in response to RPC `send` method".to_string()
                            )) as Box<dyn std::error::Error>);
                        }
                    }
                }
                _ => {
                    return Err(Box::new(RpcError::Custom(
                        "Received an invalid type in response to RPC `send` method".to_string()
                    )) as Box<dyn std::error::Error>);
                }
            }
        }
    };

    (rpc_response, registerProgram) => {
        |resp| {
            match resp {
                RpcMessage::Response { response, .. } => {
                    match response {
                        Ok(resp) => {
                            return Ok(resp)
                        },

                        _ => {
                            return Err(Box::new(RpcError::Custom(
                                "Received an invalid type in response to RPC `registerProgram` method".to_string()
                            )) as Box<dyn std::error::Error>);
                        }
                    }
                }
                _ => {
                    return Err(Box::new(RpcError::Custom(
                        "Received an invalid type in response to RPC `registerProgram` method".to_string()
                    )) as Box<dyn std::error::Error>);
                }
            }
        }
    };

    (rpc_response, getAccount) => {
        |resp| {
            match resp {
                RpcMessage::Response { response, .. } => {
                    match response {
                        Ok(TransactionResponse::GetAccountResponse(account)) => {
                            return Ok(TransactionResponse::GetAccountResponse(account))
                        }
                        _ => {
                            return Err(Box::new(RpcError::Custom(
                                "received an invalid type in response to RPC `getAccount` method".to_string()
                            )) as Box<dyn std::error::Error>);
                        }
                    }
                }
                _ => return Err(Box::new(RpcError::Custom(
                        "Received an invalid type in response to RPC `getAccount` method".to_string()
                    )) as Box<dyn std::error::Error>)
            }
        }
    };

    (engine_response, call) => {
        |resp| match resp {
        }
    };

    (engine_response, send) => {
        |resp| match resp {
        }
    };

    (engine_response, registerProgram) => {
        |resp| match resp {
        }
    };

    (retrieve_blob) => {
        |resp| match resp {
            Some(account) => Ok(Some(account)),
            _ => Ok(None) 
        }
    };

    (retrieve_blob_index) => {
        |resp| match resp {
            EoMessage::AccountBlobIndexAcquired {
                address, batch_header_hash, blob_index
            } => {
                Ok((address, batch_header_hash, blob_index))
            },
            EoMessage::ContractBlobIndexAcquired {
                program_id, batch_header_hash, blob_index
            } => {
                Ok((program_id, batch_header_hash, blob_index))
            },
            EoMessage::AccountBlobIndexNotFound {
                address
            } => {
                Err(
                    Box::new(
                        EngineError::Custom(
                            format!(
                                "unable to acquire blob index for account: {:?}", 
                                address
                            )
                        )
                    ) as Box<dyn std::error::Error>
                )
            },
            EoMessage::ContractBlobIndexNotFound {
                program_id
            } => {
                Err(
                    Box::new(
                        EngineError::Custom(
                            format!(
                                "unable to acquire blob index for program_id: {:?}",
                                program_id
                            )
                        )
                    ) as Box<dyn std::error::Error>
                )
            }
            _ => {
                Err(
                    Box::new(
                        EngineError::Custom(
                            "invalid response received".to_string()
                        )
                    ) as Box<dyn std::error::Error>
                )
            }

        }
    };

    (account_cache_response) => {
        |resp| match resp {
            Some(account) => Ok(account),
            None => {
                Err(
                    Box::new(
                        AccountCacheError
                    ) as Box<dyn std::error::Error>
                )
            }
        }
    };
}

fn channel_closed_unexpectedly<E: std::error::Error + 'static>(e: E) -> Box<dyn std::error::Error + 'static> {
    Box::new(e)
}

pub async fn handle_actor_response<T, F, M>(
    rx: OneshotReceiver<T>,
    handler: F
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
    let actor: ActorRef<AccountCacheMessage> = ractor::registry::where_is(
        ActorType::AccountCache.to_string()
    )?.into();

    let (tx, rx) = oneshot();
    let message = AccountCacheMessage::Read { address, tx };

    let _ = actor.cast(message).ok()?; 

    let handler = create_handler!(account_cache_response);
    let account = handle_actor_response(rx, handler).await.ok()?;

    Some(account)
}

pub async fn check_da_for_account(address: Address) -> Option<Account> {
    let eo_actor: ActorRef<EoMessage> = ractor::registry::where_is(
        ActorType::EoClient.to_string()
    )?.into();

    let (tx, rx) = oneshot();
    let message = EoMessage::GetAccountBlobIndex { address, sender: tx};

    let _ = eo_actor.cast(message).ok()?;
    let eo_handler = create_handler!(retrieve_blob_index);
    let blob_index = handle_actor_response(rx, eo_handler).await.ok()?;

    let (tx, rx) = oneshot();
    let da_actor: ActorRef<DaClientMessage> = ractor::registry::where_is(
        ActorType::DaClient.to_string()
    )?.into();

    let message = DaClientMessage::RetrieveAccount { 
        address,
        batch_header_hash: blob_index.1.into(), 
        blob_index: blob_index.2, 
        tx 
    };

    da_actor.cast(message).ok()?;
    let da_handler = create_handler!(retrieve_blob);

    handle_actor_response(rx, da_handler).await.ok()?
}

pub async fn get_account(address: Address) -> Option<Account> {
    log::info!("checking account cache for account: {} using `get_account` method in mod.rs", address.to_full_string());
    let mut account = check_account_cache(address).await;
    if let None = &mut account {
        account = check_da_for_account(address).await;
    }
    return account
}
