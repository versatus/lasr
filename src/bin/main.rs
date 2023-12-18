use eo_listener::EoServerError;
use lasr::EoServerWrapper;
use lasr::LasrRpcServerActor;
use lasr::TaskScheduler;
use lasr::Engine;
use lasr::Validator;
use lasr::EoServer;
use eo_listener::EoServer as EoListener;
use lasr::DaClient;
use lasr::rpc::LasrRpcServer;
use lasr::actors::LasrRpcServerImpl;
use lasr::actors::ActorRegistry;
use jsonrpsee::server::ServerBuilder as RpcServerBuilder;
use ractor::Actor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::init_with_level(log::Level::Info).map_err(|e| EoServerError::Other(e.to_string()))?;
    let registry = ActorRegistry;
    let (registry_ref, _) = Actor::spawn(
        Some("registry".to_string()), registry, ()
    ).await.map_err(|e| Box::new(e))?;
    
    let lasr_rpc_actor = LasrRpcServerActor::new(registry_ref.clone());
    let scheduler_actor = TaskScheduler::new(registry_ref.clone());
    let engine_actor = Engine::new(registry_ref.clone());
    let validator_actor = Validator::new(registry_ref.clone());
    let eo_server_actor = EoServer::new(registry_ref.clone());
    let da_client_actor = DaClient::new(registry_ref.clone());
    let inner_eo_server = setup_eo_server().map_err(|e| {
        Box::new(e)
    })?;

    let (lasr_rpc_actor_ref, _) = Actor::spawn(
        None, lasr_rpc_actor.clone(), ()
    ).await.map_err(|e| Box::new(e))?;
    lasr_rpc_actor.register_self(lasr_rpc_actor_ref.clone())?;

    let (scheduler_actor_ref, _) = Actor::spawn(None, scheduler_actor.clone(), ()).await.map_err(|e| Box::new(e))?;
    scheduler_actor.register_self(scheduler_actor_ref.clone())?;

    let (engine_actor_ref, _) = Actor::spawn(None, engine_actor.clone(), ()).await.map_err(|e| Box::new(e))?;
    engine_actor.register_self(engine_actor_ref.clone())?;

    let (validator_actor_ref, _) = Actor::spawn(None, validator_actor.clone(), ()).await.map_err(|e| Box::new(e))?;
    validator_actor.register_self(validator_actor_ref.clone())?;

    let (eo_server_actor_ref, _) = Actor::spawn(None, eo_server_actor.clone(), ()).await.map_err(|e| Box::new(e))?;
    eo_server_actor.register_self(eo_server_actor_ref.clone())?;

    let (da_client_actor_ref, _) = Actor::spawn(None, da_client_actor.clone(), ()).await.map_err(|e| Box::new(e))?;
    da_client_actor.register_self(da_client_actor_ref.clone())?;

    let lasr_rpc = LasrRpcServerImpl::new(lasr_rpc_actor_ref.clone());
    let server = RpcServerBuilder::default().build("127.0.0.1:9292").await.map_err(|e| {
        Box::new(e)
    })?;
    let server_handle = server.start(lasr_rpc.into_rpc()).map_err(|e| {
        Box::new(e)
    })?;
    let eo_server_wrapper = EoServerWrapper::new(
        eo_server_actor_ref.clone(),
        inner_eo_server,
    );

    tokio::spawn(eo_server_wrapper.run());
    tokio::spawn(server_handle.stopped());

    loop {
    }

    Ok(())
}


fn setup_eo_server() -> Result<EoListener, EoServerError> {

    let http: web3::transports::Http = web3::transports::Http::new("http://127.0.0.1:8545").map_err(|err| {
        EoServerError::Other(err.to_string())
    })?;

    // Initialize the ExecutableOracle Address
    let eo_address = eo_listener::EoAddress::new("0x610178dA211FEF7D417bC0e6FeD39F05609AD788");
    // Initialize the web3 instance
    let web3: web3::Web3<web3::transports::Http> = web3::Web3::new(http);

    let contract_address = eo_address.parse().map_err(|err| {
        EoServerError::Other(err.to_string())
    })?;
    let contract_abi = eo_listener::get_abi()?;
    let address = web3::types::Address::from(contract_address);
    let contract = web3::contract::Contract::new(web3.eth(), address, contract_abi);
    
    let blob_settled_topic = eo_listener::get_blob_index_settled_topic();
    let bridge_topic = eo_listener::get_bridge_event_topic();

    let blob_settled_filter = web3::types::FilterBuilder::default()
        .address(vec![contract_address])
        .topics(blob_settled_topic, None, None, None)
        .build();

    let bridge_filter = web3::types::FilterBuilder::default()
        .address(vec![contract_address])
        .topics(bridge_topic, None, None, None)
        .build();

    let blob_settled_event = contract.abi().event("BlobIndexSettled").map_err(|e| {
       EoServerError::Other(e.to_string()) 
    })?.clone();

    let bridge_event = contract.abi().event("Bridge").map_err(|e| {
        EoServerError::Other(e.to_string())
    })?.clone();

    
    let eo_server = eo_listener::EoServerBuilder::default()
        .web3(web3)
        .eo_address(eo_address)
        .last_processed_block(web3::types::BlockNumber::Number(web3::types::U64([4])))
        .contract(contract)
        .bridge_filter(bridge_filter)
        .blob_settled_filter(blob_settled_filter)
        .blob_settled_event(blob_settled_event)
        .bridge_event(bridge_event)
        .build()?;
    
    println!("Returning eo server");
    Ok(eo_server)
}
