mod rpc_server;
mod messages;
mod types;
mod scheduler;
mod registry;
mod engine;
mod validator;
mod da_client;
mod eo_server;

pub use rpc_server::*;
pub use registry::*;
pub use scheduler::*;
pub use engine::*;
pub use validator::*;
pub use da_client::*;
pub use eo_server::*;
pub use messages::*;
pub use types::*;


use ractor::Message;
use ractor::concurrency::OneshotReceiver;

#[macro_export]
macro_rules! create_handler {
    (rpc_response, call) => {
        |resp| {
            match resp {
                RpcMessage::Response { response, .. } => {
                    return Ok(
                        response.map_err(|e| RpcError::Custom(e.to_string()))?
                    );
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
                    return Ok(
                        response.map_err(|e| RpcError::Custom(e.to_string()))?
                    );
                }
                _ => {
                    return Err(Box::new(RpcError::Custom(
                        "Received an invalid type in response to RPC `send` method".to_string()
                    )) as Box<dyn std::error::Error>);
                }
            }
        }
    };

    (rpc_deploy_success) => {
        |resp| {
            match resp {
                RpcMessage::DeploySuccess { .. } => {
                    Ok(())
                },
                _ => {
                    return Err(Box::new(RpcError::Custom(
                        "Received an invalid type in response to RPC `deploy` method".to_string()
                    )) as Box<dyn std::error::Error>);
                }
            }
        }
    };

    (get_scheduler) => {
        |resp| match resp {
            RegistryResponse::Scheduler(scheduler) => Ok(scheduler),
            _ => {
                Err(
                    Box::new(
                        SchedulerError::Custom(
                            "invalid registry response received".to_string()
                        )
                    ) as Box<dyn std::error::Error>
                )
            }
        }
    };
    
    (get_rpc) => {
        |resp| match resp {
            RegistryResponse::RpcServer(rpc_server) => Ok(rpc_server),
            _ => {
                Err(
                    Box::new(
                        RpcError::Custom(
                            "invalid registry response received".to_string()
                        )
                    ) as Box<dyn std::error::Error>
                )
            }
        }
    };

    (get_validator) => {
        |resp| match resp {
            RegistryResponse::Validator(validator) => Ok(validator),
            _ => {
                Err(
                    Box::new(
                        RpcError::Custom(
                            "invalid registry response received".to_string()
                        )
                    ) as Box<dyn std::error::Error>
                )
            }
        }
    };

    (get_engine) => {
        |resp| match resp {
            RegistryResponse::Engine(engine) => Ok(engine),
            _ => {
                Err(
                    Box::new(
                        RpcError::Custom(
                            "invalid registry response received".to_string()
                        )
                    ) as Box<dyn std::error::Error>
                )
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

    (engine_response, deploy) => {
        |resp| match resp {
        }
    };

    (get_eo) => {
        |resp| match resp {
            RegistryResponse::EoServer(eo_server) => Ok(eo_server),
            _ => {
                Err(
                    Box::new(
                        RpcError::Custom(
                            "invalid registry response received".to_string()
                        )
                    ) as Box<dyn std::error::Error>
                )
            }
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

    (get_da) => {
        |resp| match resp {
            RegistryResponse::DaClient(da_client) => Ok(da_client),
            _ => {
                Err(
                    Box::new(
                        RpcError::Custom(
                            "invalid registry response received".to_string()
                        )
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
    T: Message,
    F: FnOnce(T) -> Result<M, Box<dyn std::error::Error>>,
{
    tokio::select! {
        response = rx => {
            let resp = response.map_err(|e| channel_closed_unexpectedly(e))?;
            handler(resp)
        }
    }
}
