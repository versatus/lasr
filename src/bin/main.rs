# ![allow(unreachable_code)]
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::FromStr;

use eo_listener::EoServer as EoListener;
use eo_listener::EoServerError;
use jsonrpsee::server::ServerBuilder as RpcServerBuilder;
use lasr::actors::LasrRpcServerImpl;
use lasr::rpc::LasrRpcServer;
use lasr::AccountCacheActor;
use lasr::ActorType;
use lasr::Address;
use lasr::Batcher;
use lasr::BatcherActor;
use lasr::BlobCacheActor;
use lasr::DaClient;
use lasr::DaSupervisor;
use lasr::Engine;
use lasr::EoClient;
use lasr::EoClientActor;
use lasr::EoServer;
use lasr::EoServerWrapper;
use lasr::ExecutionEngine;
use lasr::ExecutorActor;
use lasr::LasrRpcServerActor;
use lasr::OciManager;
use lasr::OciBundlerBuilder;
use lasr::OciBundler;
use web3_pkg::web3_store::Web3Store;


use lasr::PendingTransactionActor;
use lasr::TaskScheduler;
use lasr::Validator;
use ractor::Actor;

use secp256k1::Secp256k1;
use web3::types::BlockNumber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::init_with_level(log::Level::Info)
        .map_err(|e| EoServerError::Other(e.to_string()))?;

    log::info!("Current Working Directory: {:?}", std::env::current_dir());

    dotenv::dotenv().ok();

    //TODO(asmith): Move this to be read in when and where needed and dropped
    //afterwards to minimize security vulnerabilities
    let (_, sk_string) = std::env::vars()
        .find(|(k, _)| k == "SECRET_KEY")
        .ok_or(Box::new(std::env::VarError::NotPresent) as Box<dyn std::error::Error>)?;

    let (_, block_processed_path) = std::env::vars()
        .find(|(k, _)| k == "BLOCKS_PROCESSED_PATH")
        .ok_or(Box::new(std::env::VarError::NotPresent) as Box<dyn std::error::Error>)?;

    let sk = web3::signing::SecretKey::from_str(&sk_string).map_err(|e| Box::new(e))?;

    dbg!(sk);

    let eigen_da_client = eigenda_client::EigenDaGrpcClientBuilder::default()
        .proto_path("./eigenda/api/proto/disperser/disperser.proto".to_string())
        .server_address("disperser-goerli.eigenda.xyz:443".to_string())
        .adversary_threshold(40)
        .quorum_threshold(60)
        .build()?;

    let eth_rpc_url = std::env::var("ETH_RPC_URL").expect("ETH_RPC_URL must be set");
    println!("Ethereum RPC URL: {}", eth_rpc_url);
    let http = web3::transports::Http::new(&eth_rpc_url).expect("Invalid ETH_RPC_URL");
    let web3_instance: web3::Web3<web3::transports::Http> = web3::Web3::new(http);
    let eo_client = setup_eo_client(web3_instance.clone(), sk).await?;

    #[cfg(feature = "local")]
    let bundler: OciBundler<String, String> = OciBundlerBuilder::default()
        .runtime("/usr/local/bin/runsc".to_string())
        .base_images("./base_image".to_string())
        .containers("./containers".to_string())
        .payload_path("./payload".to_string())
        .build()?;

    let store = if let Ok(addr) = std::env::var("VIPFS_ADDRESS") {
        Web3Store::from_multiaddr(&addr)?
    } else {
        Web3Store::local()?
    };
    #[cfg(feature = "local")]
    let oci_manager = OciManager::new(bundler, store);

    #[cfg(feature = "local")]
    let execution_engine = ExecutionEngine::new(oci_manager);

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
    let storage_rpc_url = std::env::var("STORAGE_RPC_URL").expect("COMPUTE_RPC_URL must be set");
    #[cfg(feature = "remote")]
    let storage_rpc_client = jsonrpsee::ws_client::WsClientBuilder::default()
        .build(storage_rpc_url)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    #[cfg(feature = "remote")]
    let execution_engine = ExecutionEngine::new(compute_rpc_client, storage_rpc_client);

    let blob_cache_actor = BlobCacheActor::new();
    let account_cache_actor = AccountCacheActor::new();
    let pending_transaction_actor = PendingTransactionActor;
    let lasr_rpc_actor = LasrRpcServerActor::new();
    let scheduler_actor = TaskScheduler::new();
    let engine_actor = Engine::new();
    let validator_actor = Validator::new();
    let eo_server_actor = EoServer::new();
    let eo_client_actor = EoClientActor;
    let da_supervisor = DaSupervisor;
    let da_client_actor = DaClient::new(eigen_da_client);
    let batcher_actor = BatcherActor;
    let executor_actor = ExecutorActor;
    let inner_eo_server =
        setup_eo_server(web3_instance.clone(), &block_processed_path).map_err(|e| Box::new(e))?;

    let (receivers_thread_tx, receivers_thread_rx) = tokio::sync::mpsc::channel(128);
    let batcher = Batcher::new(receivers_thread_tx);

    tokio::spawn(Batcher::run_receivers(receivers_thread_rx));

    let (da_supervisor, _) = Actor::spawn(Some("da_supervisor".to_string()), da_supervisor, ())
        .await
        .map_err(|e| Box::new(e))?;

    let (lasr_rpc_actor_ref, _) =
        Actor::spawn(Some(ActorType::RpcServer.to_string()), lasr_rpc_actor, ())
            .await
            .map_err(|e| Box::new(e))?;

    let (_scheduler_actor_ref, _) =
        Actor::spawn(Some(ActorType::Scheduler.to_string()), scheduler_actor, ())
            .await
            .map_err(|e| Box::new(e))?;

    let (_engine_actor_ref, _) =
        Actor::spawn(Some(ActorType::Engine.to_string()), engine_actor, ())
            .await
            .map_err(|e| Box::new(e))?;

    let (_validator_actor_ref, _) =
        Actor::spawn(Some(ActorType::Validator.to_string()), validator_actor, ())
            .await
            .map_err(|e| Box::new(e))?;

    let (_eo_server_actor_ref, _) =
        Actor::spawn(Some(ActorType::EoServer.to_string()), eo_server_actor, ())
            .await
            .map_err(|e| Box::new(e))?;

    let (_eo_client_actor_ref, _) = Actor::spawn(
        Some(ActorType::EoClient.to_string()),
        eo_client_actor,
        eo_client,
    )
    .await
    .map_err(|e| Box::new(e))?;

    let (_da_client_actor_ref, _) = Actor::spawn_linked(
        Some(ActorType::DaClient.to_string()),
        da_client_actor,
        (),
        da_supervisor.get_cell(),
    )
    .await
    .map_err(|e| Box::new(e))?;

    let (_pending_transaction_actor_ref, _) = Actor::spawn(
        Some(ActorType::PendingTransactions.to_string()),
        pending_transaction_actor,
        (),
    )
    .await
    .map_err(|e| Box::new(e))?;

    let (_batcher_actor_ref, _) =
        Actor::spawn(Some(ActorType::Batcher.to_string()), batcher_actor, batcher)
            .await
            .map_err(|e| Box::new(e))?;

    let (_executor_actor_ref, _) = Actor::spawn(
        Some(ActorType::Executor.to_string()),
        executor_actor,
        execution_engine,
    )
    .await
    .map_err(|e| Box::new(e))?;

    let (_account_cache_actor_ref, _) = Actor::spawn(
        Some(ActorType::AccountCache.to_string()),
        account_cache_actor,
        (),
    )
    .await
    .map_err(|e| Box::new(e))?;

    let (_blob_cache_actor_ref, _) =
        Actor::spawn(Some(ActorType::BlobCache.to_string()), blob_cache_actor, ())
            .await
            .map_err(|e| Box::new(e))?;

    let lasr_rpc = LasrRpcServerImpl::new(lasr_rpc_actor_ref.clone());
    let port = std::env::var("PORT").unwrap_or_else(|_| "9292".to_string());
    let server = RpcServerBuilder::default()
        .build(format!("0.0.0.0:{}", port))
        .await
        .map_err(|e| Box::new(e))?;
    let server_handle = server.start(lasr_rpc.into_rpc()).map_err(|e| Box::new(e))?;
    let eo_server_wrapper = EoServerWrapper::new(inner_eo_server);

    let (_stop_tx, stop_rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(eo_server_wrapper.run());
    tokio::spawn(server_handle.stopped());
    tokio::spawn(lasr::batch_requestor(stop_rx));

    loop {}

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
        .to_block(BlockNumber::Number(0.into()))
        .address(vec![contract_address])
        .topics(blob_settled_topic.clone(), None, None, None)
        .build();

    let bridge_filter = web3::types::FilterBuilder::default()
        .from_block(BlockNumber::Number(0.into()))
        .to_block(BlockNumber::Number(0.into()))
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
        .processed_blocks(BTreeSet::new())
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
