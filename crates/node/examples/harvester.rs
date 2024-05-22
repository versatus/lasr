use eo_listener::EoServerError;
use lasr_actors::PendingTransactionActor;
use lasr_messages::ActorType::PendingTransactions;
use lasr_messages::{ActorType, HarvesterListenerMessage};
use lasr_types::{Token, Transaction};
use ractor::concurrency::Duration;
use ractor::Actor;
use ractor::BytesConvertable;

#[tokio::main]
pub async fn main() {
    simple_logger::init_with_level(log::Level::Info)
        .map_err(|e| EoServerError::Other(e.to_string()))
        .unwrap();

    let node_server = ractor_cluster::NodeServer::new(
        8198,
        "".to_string(),
        "harvester".to_string(),
        "localhost".to_string(),
        None,
        None,
    );

    let (node_actor, node_handler) = Actor::spawn(None, node_server, ())
        .await
        .expect("unable to spawn node server actor");

    //look for a HarvesterListenerActor
    log::info!("Looking for HarvesterListenerActor");

    let harvester_listener_search_string = ActorType::HarvesterListener.to_string();

    log::info!(
        "harvester_listener_search_string: {:?}",
        harvester_listener_search_string
    );

    loop {
        let remote_actors = ractor::pg::get_members(&harvester_listener_search_string.clone());

        log::info!("Group: \n{:?}", remote_actors);

        let remote_actor = if !remote_actors.is_empty() {
            log::info!("HarvesterListenerActor found");
            remote_actors[0].clone()
        } else {
            log::info!("{:?}", ractor::registry::registered());
            log::info!("HarvesterListenerActor not found");
            ractor::concurrency::sleep(Duration::from_millis(1000)).await;
            continue;
        };

        ractor::concurrency::sleep(Duration::from_millis(1000)).await;

        let transaction = Transaction::default();

        let token: Token = transaction.into();

        remote_actor
            .send_message(HarvesterListenerMessage::TransactionApplied(
                "".to_string(),
                token,
            ))
            .unwrap();

        ractor::concurrency::sleep(Duration::from_millis(1000)).await;

        let remote_actors = ractor::pg::get_members(&harvester_listener_search_string.clone());

        assert_eq!(remote_actors.len(), 1);

        log::info!("Harvester Listener Exists");

        ractor::concurrency::sleep(Duration::from_millis(2000)).await;

        let remote_actors = ractor::pg::get_members(&harvester_listener_search_string.clone());

        assert_eq!(remote_actors.len(), 0);

        log::info!("Farmer removed");

        break;
    }
}
