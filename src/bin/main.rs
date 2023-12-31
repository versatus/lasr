use std::collections::BTreeSet;

use eo_listener::EoServerError;
use lasr::AccountCacheActor;
use lasr::ActorType;
use lasr::Address;
use lasr::BlobCacheActor;
use lasr::EoClient;
use lasr::EoClientActor;
use lasr::EoServerWrapper;
use lasr::LasrRpcServerActor;
use lasr::PendingTransactionActor;
use lasr::TaskScheduler;
use lasr::Engine;
use lasr::Validator;
use lasr::EoServer;
use eo_listener::EoServer as EoListener;
use lasr::DaClient;
use lasr::rpc::LasrRpcServer;
use lasr::actors::LasrRpcServerImpl;
use jsonrpsee::server::ServerBuilder as RpcServerBuilder;
use ractor::Actor;

use secp256k1::Secp256k1;
use web3::types::BlockNumber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::init_with_level(
        log::Level::Info
    ).map_err(|e| EoServerError::Other(e.to_string()))?;

    let eigen_da_client = eigenda_client::EigenDaGrpcClientBuilder::default() 
        .proto_path("./eigenda/api/proto/disperser/disperser.proto".to_string())
        .server_address("disperser-goerli.eigenda.xyz:443".to_string())
        .adversary_threshold(40)
        .quorum_threshold(60)
        .build()?;

    let http: web3::transports::Http = web3::transports::Http::new("http://127.0.0.1:8545").map_err(|err| {
        EoServerError::Other(err.to_string())
    })?;

    let web3_instance: web3::Web3<web3::transports::Http> = web3::Web3::new(http);

    let eo_client = setup_eo_client(web3_instance.clone()).await?;

    let blob_cache_actor = BlobCacheActor::new(); 
    let account_cache_actor = AccountCacheActor::new();
    let pending_transaction_actor = PendingTransactionActor;
    let lasr_rpc_actor = LasrRpcServerActor::new();
    let scheduler_actor = TaskScheduler::new();
    let engine_actor = Engine::new();
    let validator_actor = Validator::new();
    let eo_server_actor = EoServer::new();
    let eo_client_actor = EoClientActor;
    let da_client_actor = DaClient::new(eigen_da_client);
    let inner_eo_server = setup_eo_server(web3_instance.clone()).map_err(|e| {
        Box::new(e)
    })?;

    let (lasr_rpc_actor_ref, _) = Actor::spawn(
        Some(ActorType::RpcServer.to_string()), 
        lasr_rpc_actor, 
        ()
    ).await.map_err(|e| Box::new(e))?;

    let (_scheduler_actor_ref, _) = Actor::spawn(
        Some(ActorType::Scheduler.to_string()), 
        scheduler_actor, 
        ()
    ).await.map_err(|e| Box::new(e))?;

    let (_engine_actor_ref, _) = Actor::spawn(
        Some(ActorType::Engine.to_string()), 
        engine_actor, 
        ()
    ).await.map_err(|e| Box::new(e))?;

    let (_validator_actor_ref, _) = Actor::spawn(
        Some(ActorType::Validator.to_string()),
        validator_actor, 
        ()
    ).await.map_err(|e| Box::new(e))?;

    let (_eo_server_actor_ref, _) = Actor::spawn(
        Some(ActorType::EoServer.to_string()), 
        eo_server_actor, 
        ()
    ).await.map_err(|e| Box::new(e))?;

    let (_eo_client_actor_ref, _) = Actor::spawn(
        Some(ActorType::EoClient.to_string()),
        eo_client_actor,
        eo_client
    ).await.map_err(|e| Box::new(e))?;

    let (_da_client_actor_ref, _) = Actor::spawn(
        Some(ActorType::DaClient.to_string()), 
        da_client_actor, 
        ()
    ).await.map_err(|e| Box::new(e))?;

    let (_pending_transaction_actor_ref, _) = Actor::spawn(
        Some(ActorType::PendingTransactions.to_string()),
        pending_transaction_actor,
        ()
    ).await.map_err(|e| Box::new(e))?;

    let (_account_cache_actor_ref, _) = Actor::spawn(
        Some(ActorType::AccountCache.to_string()),
        account_cache_actor,
        ()
    ).await.map_err(|e| Box::new(e))?;

    let (_blob_cache_actor_ref, _) = Actor::spawn(
        Some(ActorType::BlobCache.to_string()),
        blob_cache_actor,
        ()
    ).await.map_err(|e| Box::new(e))?;

    let lasr_rpc = LasrRpcServerImpl::new(lasr_rpc_actor_ref.clone());
    let server = RpcServerBuilder::default().build("127.0.0.1:9292").await.map_err(|e| {
        Box::new(e)
    })?;
    let server_handle = server.start(lasr_rpc.into_rpc()).map_err(|e| {
        Box::new(e)
    })?;
    let eo_server_wrapper = EoServerWrapper::new(inner_eo_server);
    
    tokio::spawn(eo_server_wrapper.run());
    tokio::spawn(server_handle.stopped());

    loop {}

    #[allow(unreachable_code)]
    Ok(())
}


fn setup_eo_server(web3_instance: web3::Web3<web3::transports::Http>) -> Result<EoListener, EoServerError> {

    // Initialize the ExecutableOracle Address
    //0x5FbDB2315678afecb367f032d93F642f64180aa3
    let eo_address = eo_listener::EoAddress::new("0x5FbDB2315678afecb367f032d93F642f64180aa3");
    let contract_address = eo_address.parse().map_err(|err| {
        EoServerError::Other(err.to_string())
    })?;
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

    let blob_settled_event = contract.abi().event("BlobIndexSettled").map_err(|e| {
       EoServerError::Other(e.to_string()) 
    })?.clone();

    let bridge_event = contract.abi().event("Bridge").map_err(|e| {
        EoServerError::Other(e.to_string())
    })?.clone();

    
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
        .build()?;
    
    Ok(eo_server)
}

async fn setup_eo_client(web3_instance: web3::Web3<web3::transports::Http>) -> Result<EoClient, Box<dyn std::error::Error>> {
    // Initialize the ExecutableOracle Address
    //0x5FbDB2315678afecb367f032d93F642f64180aa3
    //0x5FbDB2315678afecb367f032d93F642f64180aa3
    let eo_address = eo_listener::EoAddress::new("0x5FbDB2315678afecb367f032d93F642f64180aa3");
    // Initialize the web3 instance
    let contract_address = eo_address.parse().map_err(|err| {
        Box::new(
            err
        ) as Box<dyn std::error::Error>
    })?;
    let contract_abi = eo_listener::get_abi().map_err(|e| {
        Box::new(e) as Box<dyn std::error::Error>
    })?;
    let address = web3::types::Address::from(contract_address);
    let contract = web3::contract::Contract::new(web3_instance.eth(), address, contract_abi);

    let secp = Secp256k1::new();

    let (_secret_key, public_key) = secp.generate_keypair(&mut secp256k1::rand::rngs::OsRng);

    let user_address: Address = public_key.into();
    EoClient::new(web3_instance, contract, user_address).await.map_err(|e| {
        Box::new(e) as Box<dyn std::error::Error>
    })
}
