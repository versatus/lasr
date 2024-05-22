use async_trait::async_trait;
use eo_listener::EoServerError;
use lasr_actors::harvester_listener::HarvesterListenerActor;
use lasr_messages::ActorType::HarvesterListener;
use lasr_messages::{ActorType, HarvesterListenerMessage, SchedulerMessage};
use lasr_types::Transaction;
use ractor::concurrency::Duration;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use ractor_cluster::NodeServer;
use tokio;

pub struct FakeSchedulerActor;

#[async_trait]
impl Actor for FakeSchedulerActor {
    type Msg = SchedulerMessage;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        return match message {
            SchedulerMessage::TransactionApplied {
                transaction_hash,
                token,
            } => {
                log::info!("Transaction applied received");
                return Err("Shutting down, transaction applied received".into());
            }
            (_) => {
                // panic!("unexpected message: {:?}", message);
                Ok(())
            }
        };
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::init_with_level(log::Level::Info)
        .map_err(|e| EoServerError::Other(e.to_string()))
        .unwrap();

    let node_server = ractor_cluster::NodeServer::new(
        8197,
        "".to_string(),
        "farmer".to_string(),
        "localhost".to_string(),
        None,
        None,
    );

    let (node_actor, node_handler) = Actor::spawn(None, node_server, ())
        .await
        .expect("unable to spawn node server actor");

    log::info!("Node server actor spawned");
    let harvester_listener_actor = HarvesterListenerActor::new();
    let (harvester_listener_actor_ref, _) = Actor::spawn(
        Some(ActorType::HarvesterListener.to_string()),
        harvester_listener_actor,
        (),
    )
    .await
    .expect("unable to spawn validator actor");
    ractor::concurrency::sleep(Duration::from_millis(1000)).await;

    ractor_cluster::node::client::connect(&node_actor, "localhost:8198")
        .await
        .expect("unable to connect to node server");

    let fake_scheduler = FakeSchedulerActor;

    let (fake_scheduler_ref, fake_scheduler_handler) =
        Actor::spawn(Some(ActorType::Scheduler.to_string()), fake_scheduler, ())
            .await
            .expect("unable to spawn fake scheduler actor");

    ractor::concurrency::sleep(Duration::from_millis(2000)).await;

    let fake_scheduler_handler_result = fake_scheduler_handler.await;
    log::info!(
        "Fake scheduler actor shutdown result: {:?}",
        fake_scheduler_handler_result
    );

    ractor::concurrency::sleep(Duration::from_millis(2000)).await;

    Ok(())
}

//
// #[tokio::test]
// async fn handle_pending_transaction_remote() {
//     //enable info level log
//
//     simple_logger::init_with_level(log::Level::Info)
//         .map_err(|e| EoServerError::Other(e.to_string()))
//         .unwrap();
//
//     // pub struct MockAccountCacheActor;
//     //
//     // #[async_trait]
//     // impl Actor for MockAccountCacheActor {
//     //     type Msg = AccountCacheMessage;
//     //     type State = ();
//     //     type Arguments = ();
//     //
//     //     async fn pre_start(
//     //         &self,
//     //         _myself: ActorRef<Self::Msg>,
//     //         _: (),
//     //     ) -> Result<Self::State, ActorProcessingErr> {
//     //         Ok(())
//     //     }
//     //
//     //     async fn handle(
//     //         &self,
//     //         myself: ActorRef<Self::Msg>,
//     //         message: Self::Msg,
//     //         state: &mut Self::State,
//     //     ) -> Result<(), ActorProcessingErr> {
//     //         Ok(())
//     //     }
//     // }
//     //
//     // let mock_account_cache_actor = MockAccountCacheActor {};
//
//     // let mock_account_cache_ref = Actor::spawn(Some(ActorType::AccountCache.to_string()), mock_account_cache_actor, ())
//     //     .await
//     //     .expect("unable to spawn account cache actor");
//
//     // tokio::spawn(async move {
//     //     // let node_server = ractor_cluster::NodeServer::new(
//     //     //     8198,
//     //     //     "".to_string(),
//     //     //     "harvester".to_string(),
//     //     //     "localhost".to_string(),
//     //     //     None,
//     //     //     None,
//     //     // );
//     //     //
//     //     // let (node_server_ref, _) = Actor::spawn(None, node_server, ())
//     //     //     .await
//     //     //     .expect("unable to spawn node server actor");
//     //
//     //     let validator_actor = ValidatorActor::new();
//     //     let
//     //     let (validator_actor_ref, _) =
//     //         Actor::spawn(Some(ActorType::Validator.to_string()), validator_actor, ())
//     //             .await
//     //             .expect("unable to spawn validator actor");
//     //
//     //     // let account_cache_actor = FakeAccountCacheActor;
//     //     //
//     //     // let account_cache_ref = Actor::spawn(
//     //     //     Some(ActorType::AccountCache.to_string()),
//     //     //     account_cache_actor,
//     //     //     (),
//     //     // )
//     //     // .await
//     //     // .expect("unable to spawn account cache actor");
//     //
//     //     info!("Node server actor spawned");
//     //
//     //     ractor::concurrency::sleep(Duration::from_millis(10000)).await;
//     // });
//
//     tokio::spawn(async move {
//         // let node_server = ractor_cluster::NodeServer::new(
//         //     8197,
//         //     "".to_string(),
//         //     "farmer-1".to_string(),
//         //     "localhost".to_string(),
//         //     None,
//         //     None,
//         // );
//
//         // let (node_actor, node_handler) = Actor::spawn(None, node_server, ())
//         //     .await
//         //     .expect("unable to spawn node server actor");
//
//         let account_cache_actor = FakeAccountCacheActor;
//
//         let account_cache_ref = Actor::spawn(
//             Some(ActorType::AccountCache.to_string()),
//             account_cache_actor,
//             (),
//         )
//         .await
//         .expect("unable to spawn account cache actor");
//
//         info!("Node server actor spawned");
//         ractor::concurrency::sleep(Duration::from_millis(1000)).await;
//
//         // ractor_cluster::node::client::connect(&node_actor, "localhost:8198")
//         //     .await
//         //     .expect("unable to connect to node server");
//
//         ractor::concurrency::sleep(Duration::from_millis(10000)).await;
//     });
//
//     // let node_server = ractor_cluster::NodeServer::new(
//     //     8199,
//     //     "".to_string(),
//     //     "farmer".to_string(),
//     //     "localhost".to_string(),
//     //     None,
//     //     None,
//     // );
//     //
//     // let (node_actor, node_handler) = Actor::spawn(None, node_server, ())
//     //     .await
//     //     .expect("unable to spawn node server actor");
//
//     let pending_transactions = PendingTransactionActor;
//
//     let pending_transactions_ref = Actor::spawn(
//         Some(PendingTransactions.to_string()),
//         pending_transactions,
//         (),
//     )
//     .await
//     .expect("unable to spawn pending transactions actor");
//
//     let account_cache_actor = FakeAccountCacheActor;
//
//     let account_cache_ref = Actor::spawn(
//         Some(ActorType::AccountCache.to_string()),
//         account_cache_actor,
//         (),
//     )
//     .await
//     .expect("unable to spawn account cache actor");
//
//     ractor::concurrency::sleep(Duration::from_millis(1000)).await;
//
//     // ractor_cluster::node::client::connect(&node_actor, "localhost:8198")
//     //     .await
//     //     .expect("unable to connect to node server");
//
//     ractor::concurrency::sleep(Duration::from_millis(1000)).await;
//
//     // let remote_actor = ractor::registry::where_is(ActorType::Validator.to_string())
//     //     .expect("unable to acquire node server actor");
//     //
//     // ractor::concurrency::sleep(Duration::from_millis(1000)).await;
//
//     // remote_actor
//     //     .send_message(ValidatorMessage::PendingTransaction {
//     //         transaction: Transaction::default(),
//     //     })
//     //     .unwrap();
//
//     ractor::concurrency::sleep(Duration::from_millis(5000)).await;
//
//     // Create the Validator actor and its state
//     // let validator_actor = Validator::new();
//     // let (validator_actor_ref, _) =
//     //     Actor::spawn(Some(ActorType::Validator.to_string()), validator_actor, ())
//     //         .await
//     //         .expect("unable to spawn validator actor");
//     //
//     // // Register the Validator actor with the NodeServer
//     //
//     // // Create the message to be handled
//     // let transaction = Transaction::default();
//     // let message = ValidatorMessage::PendingTransaction(transaction.clone());
//     //
//     // let message = message.serialize()?;
//     //
//     // validator_actor_ref
//     //     .handle_serialized(message, &mut validator_core)
//     //     .await?;
//     //
//     // // Send the message to the NodeServer
//     // node_server_ref
//     //     .tell(message, Some(validator_actor_ref.clone()))
//     //     .await
//     //     .expect("unable to send message to node server");
// }
