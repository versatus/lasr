#![allow(unreachable_code)]
use std::{collections::BTreeSet, path::PathBuf, str::FromStr, sync::Arc};

use eo_listener::{EoServer as EoListener, EoServerError};
use futures::StreamExt;
use jsonrpsee::server::ServerBuilder as RpcServerBuilder;
use lasr_actors::{
    graph_cleaner, helpers::Coerce, AccountCacheActor, AccountCacheSupervisor, ActorExt,
    ActorManager, ActorManagerBuilder, Batcher, BatcherActor, BatcherError, BatcherSupervisor,
    BlobCacheActor, BlobCacheSupervisor, DaClient, DaClientActor, DaClientSupervisor, EngineActor,
    EngineSupervisor, EoClient, EoClientActor, EoClientSupervisor, EoServerActor,
    EoServerSupervisor, EoServerWrapper, ExecutionEngine, ExecutorActor, ExecutorSupervisor,
    LasrRpcServerActor, LasrRpcServerImpl, LasrRpcServerSupervisor, PendingTransactionActor,
    PendingTransactionSupervisor, TaskScheduler, TaskSchedulerSupervisor, ValidatorActor,
    ValidatorCore, ValidatorSupervisor,
};
use lasr_compute::{OciBundler, OciBundlerBuilder, OciManager};
use lasr_messages::{ActorName, ActorType, ToActorType};
use lasr_rpc::LasrRpcServer;
use lasr_types::Address;
use ractor::{Actor, ActorCell, ActorStatus};
use secp256k1::Secp256k1;
use tokio::sync::{
    mpsc::{Receiver, Sender},
    Mutex,
};
use web3::types::BlockNumber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::init_with_level(log::Level::Error)
        .map_err(|e| EoServerError::Other(e.to_string()))?;

    log::info!("Current Working Directory: {:?}", std::env::current_dir());

    log::warn!(
        "Version, branch and hash: {} {}",
        env!("CARGO_PKG_VERSION"),
        option_env!("GIT_REV").unwrap_or("N/A")
    );

    dotenv::dotenv().ok();

    //TODO(asmith): Move this to be read in when and where needed and dropped
    //afterwards to minimize security vulnerabilities
    let (_, sk_string) = std::env::vars()
        .find(|(k, _)| k == "SECRET_KEY")
        .expect("missing SECRET_KEY environment variable");

    let (_, block_processed_path) = std::env::vars()
        .find(|(k, _)| k == "BLOCKS_PROCESSED_PATH")
        .expect("missing BLOCKS_PROCESSED_PATH environment variable");

    let sk = web3::signing::SecretKey::from_str(&sk_string).map_err(Box::new)?;
    let eigen_da_client = eigenda_client::EigenDaGrpcClientBuilder::default()
        .proto_path("./eigenda/api/proto/disperser/disperser.proto".to_string())
        //TODO(asmith): Move the network endpoint for EigenDA to an
        //environment variable.
        .server_address("disperser-holesky.eigenda.xyz:443".to_string())
        .build()?;

    let eth_rpc_url = std::env::var("ETH_RPC_URL").expect("ETH_RPC_URL must be set");
    log::warn!("Ethereum RPC URL: {}", eth_rpc_url);
    let http = web3::transports::Http::new(&eth_rpc_url).expect("Invalid ETH_RPC_URL");
    let web3_instance: web3::Web3<web3::transports::Http> = web3::Web3::new(http);
    let eo_client = Arc::new(Mutex::new(
        setup_eo_client(web3_instance.clone(), sk).await?,
    ));
    let inner_eo_server =
        setup_eo_server(web3_instance.clone(), &block_processed_path).map_err(Box::new)?;

    #[cfg(feature = "local")]
    let bundler: OciBundler<String, String> = OciBundlerBuilder::default()
        .runtime("/usr/local/bin/runsc".to_string())
        .base_images("./base_image".to_string())
        .containers("./containers".to_string())
        .payload_path("./payload".to_string())
        .build()?;

    let store = if let Ok(addr) = std::env::var("VIPFS_ADDRESS") {
        Some(addr.clone())
    } else {
        None
    };

    #[cfg(feature = "local")]
    let oci_manager = OciManager::new(bundler, store);

    #[cfg(feature = "local")]
    let execution_engine = Arc::new(Mutex::new(ExecutionEngine::new(oci_manager)));

    #[cfg(feature = "remote")]
    log::info!("Attempting to connect compute agent");
    #[cfg(feature = "remote")]
    let compute_rpc_url = std::env::var("COMPUTE_RPC_URL").expect("COMPUTE_RPC_URL must be set");
    #[cfg(feature = "remote")]
    let compute_rpc_client = jsonrpsee::ws_client::WsClientBuilder::default()
        .build(compute_rpc_url)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    #[cfg(feature = "remote")]
    log::info!("Attempting to connect strorage agent");
    #[cfg(feature = "remote")]
    let storage_rpc_url = std::env::var("STORAGE_RPC_URL").expect("COMPUTE_RPC_URL must be set");
    #[cfg(feature = "remote")]
    let storage_rpc_client = jsonrpsee::ws_client::WsClientBuilder::default()
        .build(storage_rpc_url)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    #[cfg(feature = "remote")]
    let execution_engine = ExecutionEngine::new(compute_rpc_client, storage_rpc_client);

    let (panic_tx, mut panic_rx): (Sender<ActorCell>, Receiver<ActorCell>) =
        tokio::sync::mpsc::channel(120);

    let blob_cache_supervisor = BlobCacheSupervisor::new(panic_tx.clone());
    let account_cache_supervisor = AccountCacheSupervisor::new(panic_tx.clone());
    let pending_tx_supervisor = PendingTransactionSupervisor::new(panic_tx.clone());
    let lasr_rpc_server_supervisor = LasrRpcServerSupervisor::new(panic_tx.clone());
    let scheduler_supervisor = TaskSchedulerSupervisor::new(panic_tx.clone());
    let eo_server_supervisor = EoServerSupervisor::new(panic_tx.clone());
    let engine_supervisor = EngineSupervisor::new(panic_tx.clone());
    let validator_supervisor = ValidatorSupervisor::new(panic_tx.clone());
    let eo_client_supervisor = EoClientSupervisor::new(panic_tx.clone());
    let da_client_supervisor = DaClientSupervisor::new(panic_tx.clone());
    let batcher_supervisor = BatcherSupervisor::new(panic_tx.clone());
    let executor_supervisor = ExecutorSupervisor::new(panic_tx);

    let (blob_cache_supervisor, _blob_cache_supervisor_handle) = Actor::spawn(
        Some(blob_cache_supervisor.name()),
        blob_cache_supervisor,
        (),
    )
    .await
    .map_err(Box::new)?;
    let (account_cache_supervisor, _account_cache_supervisor_handle) = Actor::spawn(
        Some(account_cache_supervisor.name()),
        account_cache_supervisor,
        (),
    )
    .await
    .map_err(Box::new)?;
    let (pending_tx_supervisor, _pending_tx_supervisor_handle) = Actor::spawn(
        Some(pending_tx_supervisor.name()),
        pending_tx_supervisor,
        (),
    )
    .await
    .map_err(Box::new)?;
    let (lasr_rpc_server_supervisor, _lasr_rpc_server_supervisor_handle) = Actor::spawn(
        Some(lasr_rpc_server_supervisor.name()),
        lasr_rpc_server_supervisor,
        (),
    )
    .await
    .map_err(Box::new)?;
    let (scheduler_supervisor, _scheduler_supervisor_handle) =
        Actor::spawn(Some(scheduler_supervisor.name()), scheduler_supervisor, ())
            .await
            .map_err(Box::new)?;
    let (eo_server_supervisor, _eo_server_supervisor_handle) =
        Actor::spawn(Some(eo_server_supervisor.name()), eo_server_supervisor, ())
            .await
            .map_err(Box::new)?;
    let (engine_supervisor, _engine_supervisor_handle) =
        Actor::spawn(Some(engine_supervisor.name()), engine_supervisor, ())
            .await
            .map_err(Box::new)?;
    let (validator_supervisor, _validator_supervisor_handle) =
        Actor::spawn(Some(validator_supervisor.name()), validator_supervisor, ())
            .await
            .map_err(Box::new)?;
    let (eo_client_supervisor, _eo_client_supervisor_handle) =
        Actor::spawn(Some(eo_client_supervisor.name()), eo_client_supervisor, ())
            .await
            .map_err(Box::new)?;
    let (da_client_supervisor, _da_supervisor_handle) =
        Actor::spawn(Some(da_client_supervisor.name()), da_client_supervisor, ())
            .await
            .map_err(Box::new)?;
    let (batcher_supervisor, _batcher_supervisor_handle) =
        Actor::spawn(Some(batcher_supervisor.name()), batcher_supervisor, ())
            .await
            .map_err(Box::new)?;
    let (executor_supervisor, _executor_supervisor_handle) =
        Actor::spawn(Some(executor_supervisor.name()), executor_supervisor, ())
            .await
            .map_err(Box::new)?;

    let blob_cache_actor = BlobCacheActor::new();
    let account_cache_actor = AccountCacheActor::new();
    let pending_transaction_actor = PendingTransactionActor::new();
    let lasr_rpc_actor = LasrRpcServerActor::new();
    let scheduler_actor = TaskScheduler::new();
    let eo_server_actor = EoServerActor::new();
    let engine_actor = EngineActor::new();
    let validator_actor = ValidatorActor::new();
    let eo_client_actor = EoClientActor::new();
    let da_client_actor = DaClientActor::new();
    let batcher_actor = BatcherActor::new();
    let executor_actor = ExecutorActor::new();

    let (receivers_thread_tx, receivers_thread_rx) = tokio::sync::mpsc::channel(128);
    let batcher = Arc::new(Mutex::new(Batcher::new(receivers_thread_tx)));
    tokio::spawn(Batcher::run_receivers(receivers_thread_rx));

    let da_client = Arc::new(Mutex::new(DaClient::new(eigen_da_client)));
    let validator_core = Arc::new(Mutex::new(ValidatorCore::default()));

    let actor_manager_inner: ActorManager = ActorManagerBuilder::default()
        .blob_cache(blob_cache_actor.clone(), blob_cache_supervisor)
        .await?
        .account_cache(account_cache_actor.clone(), account_cache_supervisor)
        .await?
        .pending_tx(pending_transaction_actor.clone(), pending_tx_supervisor)
        .await?
        .lasr_rpc_server(lasr_rpc_actor.clone(), lasr_rpc_server_supervisor)
        .await?
        .scheduler(scheduler_actor.clone(), scheduler_supervisor)
        .await?
        .eo_server(eo_server_actor.clone(), eo_server_supervisor)
        .await?
        .engine(engine_actor.clone(), engine_supervisor)
        .await?
        .validator(
            validator_actor.clone(),
            validator_core.clone(),
            validator_supervisor,
        )
        .await?
        .eo_client(
            eo_client_actor.clone(),
            eo_client.clone(),
            eo_client_supervisor,
        )
        .await?
        .da_client(
            da_client_actor.clone(),
            da_client.clone(),
            da_client_supervisor,
        )
        .await?
        .batcher(batcher_actor.clone(), batcher.clone(), batcher_supervisor)
        .await?
        .executor(
            executor_actor.clone(),
            execution_engine.clone(),
            executor_supervisor,
        )
        .await?
        .build();

    let lasr_rpc_actor_ref = actor_manager_inner.get_lasr_rpc_actor_ref();

    let actor_manager = Arc::new(Mutex::new(actor_manager_inner));
    let engine_actor_clone = engine_actor.clone();
    let validator_actor_clone = validator_actor.clone();
    let validator_core_clone = validator_core.clone();
    let eo_client_actor_clone = eo_client_actor.clone();
    let eo_client_clone = eo_client.clone();
    let da_client_actor_clone = da_client_actor.clone();
    let da_client_clone = da_client.clone();
    let batcher_actor_clone = batcher_actor.clone();
    let batcher_clone = batcher.clone();
    let executor_actor_clone = executor_actor.clone();
    let execution_engine_clone = execution_engine.clone();

    tokio::spawn(async move {
        while let Some(actor) = panic_rx.recv().await {
            if let ActorStatus::Stopped = actor.get_status() {
                if let Some(actor_name) = actor.get_name() {
                    let manager_ptr = Arc::clone(&actor_manager);
                    match actor_name.to_actor_type() {
                        ActorType::BlobCache => {
                            ActorManager::respawn_blob_cache(
                                manager_ptr,
                                actor_name,
                                blob_cache_actor.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::AccountCache => {
                            ActorManager::respawn_account_cache(
                                manager_ptr,
                                actor_name,
                                account_cache_actor.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::PendingTransactions => {
                            ActorManager::respawn_pending_tx(
                                manager_ptr,
                                actor_name,
                                pending_transaction_actor.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::RpcServer => {
                            ActorManager::respawn_lasr_rpc_server(
                                manager_ptr,
                                actor_name,
                                lasr_rpc_actor.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::Scheduler => {
                            ActorManager::respawn_scheduler(
                                manager_ptr,
                                actor_name,
                                scheduler_actor.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::EoServer => {
                            ActorManager::respawn_eo_server(
                                manager_ptr,
                                actor_name,
                                eo_server_actor.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::Engine => {
                            ActorManager::respawn_engine(
                                manager_ptr,
                                actor_name,
                                engine_actor_clone.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::Validator => {
                            ActorManager::respawn_validator(
                                manager_ptr,
                                actor_name,
                                validator_actor_clone.clone(),
                                validator_core_clone.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::EoClient => {
                            ActorManager::respawn_eo_client(
                                manager_ptr,
                                actor_name,
                                eo_client_actor_clone.clone(),
                                eo_client_clone.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::DaClient => {
                            ActorManager::respawn_da_client(
                                manager_ptr,
                                actor_name,
                                da_client_actor_clone.clone(),
                                da_client_clone.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::Batcher => {
                            ActorManager::respawn_batcher(
                                manager_ptr,
                                actor_name,
                                batcher_actor_clone.clone(),
                                batcher_clone.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        ActorType::Executor => {
                            ActorManager::respawn_executor(
                                manager_ptr,
                                actor_name,
                                executor_actor_clone.clone(),
                                execution_engine_clone.clone(),
                            )
                            .await
                            .typecast()
                            .log_err(|e| e);
                        }
                        _ => unreachable!("Actor is not a child of a supervisor"),
                    }
                }
            }
        }
    });

    let lasr_rpc = LasrRpcServerImpl::new(lasr_rpc_actor_ref);
    let port = std::env::var("PORT").unwrap_or_else(|_| "9292".to_string());
    let server = RpcServerBuilder::default()
        .build(format!("0.0.0.0:{}", port))
        .await
        .map_err(Box::new)?;
    let server_handle = server.start(lasr_rpc.into_rpc()).map_err(Box::new)?;
    let eo_server_wrapper = EoServerWrapper::new(inner_eo_server);

    let (_stop_tx, stop_rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(graph_cleaner());
    tokio::spawn(eo_server_wrapper.run());
    tokio::spawn(server_handle.stopped());
    tokio::spawn(lasr_actors::batch_requestor(stop_rx));

    let future_thread_pool = tokio_rayon::rayon::ThreadPoolBuilder::new()
        .num_threads(num_cpus::get())
        .build()?;
    tokio::spawn(async move {
        loop {
            {
                let futures = batcher_actor.future_pool();
                let mut guard = futures.lock().await;
                future_thread_pool
                    .install(|| async move {
                        if let Some(Err(err)) = guard.next().await {
                            log::error!("{err:?}");
                            if let BatcherError::FailedTransaction { msg, txn } = err {
                                Batcher::handle_transaction_error(msg, *txn)
                            }
                        }
                    })
                    .await;
            }
            {
                let futures = executor_actor.future_pool();
                let mut guard = futures.lock().await;
                future_thread_pool
                    .install(|| async move { guard.next().await })
                    .await;
            }
            {
                let futures = validator_actor.future_pool();
                let mut guard = futures.lock().await;
                future_thread_pool
                    .install(|| async move {
                        if let Some(Err(err)) = guard.next().await {
                            log::error!("{err:?}");
                        }
                    })
                    .await;
            }
            {
                let futures = da_client_actor.future_pool();
                let mut guard = futures.lock().await;
                future_thread_pool
                    .install(|| async move { guard.next().await })
                    .await;
            }
            {
                let futures = engine_actor.future_pool();
                let mut guard = futures.lock().await;
                future_thread_pool
                    .install(|| async move {
                        if let Some(Err(err)) = guard.next().await {
                            log::error!("{err:?}");
                        }
                    })
                    .await;
            }
            {
                let futures = eo_client_actor.future_pool();
                let mut guard = futures.lock().await;
                future_thread_pool
                    .install(|| async move { guard.next().await })
                    .await;
            }
        }
    });

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
    }

    _stop_tx.send(1).await?;

    Ok(())
}

fn setup_eo_server(
    web3_instance: web3::Web3<web3::transports::Http>,
    path: &str,
) -> Result<EoListener, EoServerError> {
    // Initialize the ExecutableOracle Address
    //0x5FbDB2315678afecb367f032d93F642f64180aa3
    let eo_address_str = std::env::var("EO_CONTRACT_ADDRESS").expect("EO_CONTRACT_ADDRESS environment variable is not set. Please set the EO_CONTRACT_ADDRESS environment variable with the Executable Oracle contract address.");
    let eo_address = eo_listener::EoAddress::new(&eo_address_str);
    let contract_address = eo_address
        .parse()
        .map_err(|err| EoServerError::Other(err.to_string()))?;
    let contract_abi = eo_listener::get_abi()?;
    let address = web3::types::Address::from(contract_address);
    let contract = web3::contract::Contract::new(web3_instance.eth(), address, contract_abi);

    let blob_settled_topic = eo_listener::get_blob_index_settled_topic();
    let bridge_topic = eo_listener::get_bridge_event_topic();

    let blob_settled_filter = web3::types::FilterBuilder::default()
        .from_block(BlockNumber::Number(0.into()))
        .to_block(BlockNumber::Latest)
        .address(vec![contract_address])
        .topics(blob_settled_topic.clone(), None, None, None)
        .build();

    let bridge_filter = web3::types::FilterBuilder::default()
        .from_block(BlockNumber::Number(0.into()))
        .to_block(BlockNumber::Latest)
        .address(vec![contract_address])
        .topics(bridge_topic.clone(), None, None, None)
        .build();

    let blob_settled_event = contract
        .abi()
        .event("BlobIndexSettled")
        .map_err(|e| EoServerError::Other(e.to_string()))?
        .clone();

    let bridge_event = contract
        .abi()
        .event("Bridge")
        .map_err(|e| EoServerError::Other(e.to_string()))?
        .clone();

    // Logging out all the variables being sent to the EoServer
    log::info!("web3_instance: {:?}", web3_instance);
    log::info!("eo_address: {:?}", eo_address);
    log::info!("contract_address: {:?}", contract_address);
    log::info!("address: {:?}", address);
    log::info!("contract: {:?}", contract);
    log::info!("blob_settled_topic: {:?}", blob_settled_topic);
    log::info!("bridge_topic: {:?}", bridge_topic);
    log::info!("blob_settled_filter: {:?}", blob_settled_filter);
    log::info!("bridge_filter: {:?}", bridge_filter);
    log::info!("blob_settled_event: {:?}", blob_settled_event);
    log::info!("bridge_event: {:?}", bridge_event);
    log::info!("path: {:?}", path);

    let eo_server = eo_listener::EoServerBuilder::default()
        .web3(web3_instance)
        .eo_address(eo_address)
        .block_time(std::time::Duration::from_millis(2500))
        .bridge_processed_blocks(BTreeSet::new())
        .settled_processed_blocks(BTreeSet::new())
        .contract(contract)
        .bridge_topic(bridge_topic)
        .blob_settled_topic(blob_settled_topic)
        .bridge_filter(bridge_filter)
        .current_bridge_filter_block(0.into())
        .current_blob_settlement_filter_block(0.into())
        .blob_settled_filter(blob_settled_filter)
        .blob_settled_event(blob_settled_event)
        .bridge_event(bridge_event)
        .path(PathBuf::from_str(path).map_err(|e| EoServerError::Other(e.to_string()))?)
        .build()?;

    Ok(eo_server)
}

async fn setup_eo_client(
    web3_instance: web3::Web3<web3::transports::Http>,
    sk: web3::signing::SecretKey,
) -> Result<EoClient, Box<dyn std::error::Error>> {
    // Initialize the ExecutableOracle Address
    //0x5FbDB2315678afecb367f032d93F642f64180aa3
    //0x5FbDB2315678afecb367f032d93F642f64180aa3

    let eo_address_str = std::env::var("EO_CONTRACT_ADDRESS").expect("EO_CONTRACT_ADDRESS environment variable is not set. Please set the EO_CONTRACT_ADDRESS environment variable with the Executable Oracle contract address.");
    let eo_address = eo_listener::EoAddress::new(&eo_address_str);
    // Initialize the web3 instance
    let contract_address = eo_address
        .parse()
        .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)?;
    let contract_abi =
        eo_listener::get_abi().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    let address = web3::types::Address::from(contract_address);
    let contract = web3::contract::Contract::new(web3_instance.eth(), address, contract_abi);

    let secp = Secp256k1::new();

    let (_secret_key, public_key) = secp.generate_keypair(&mut secp256k1::rand::rngs::OsRng);

    let user_address: Address = public_key.into();

    // Logging out all the variables being sent to the EoClient
    log::info!("web3_instance: {:?}", web3_instance);
    log::info!("contract: {:?}", contract);
    log::info!("user_address: {:?}", user_address);
    log::info!("sk: {:?}", sk);

    EoClient::new(web3_instance, contract, user_address, sk)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}
