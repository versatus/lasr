use std::{collections::{HashMap, HashSet}, sync::{Arc, RwLock}};
use crate::{
    Address,
    PendingTransactionMessage,
    Transaction,
    ActorType,
    ValidatorMessage,
    BatcherMessage,
    Outputs,
    AddressOrNamespace, TransactionType
};
use async_trait::async_trait;
use ractor::{Actor, ActorRef, ActorProcessingErr};
use schemars::JsonSchema;
use serde::{Serialize, Deserialize};
use std::fmt::Display;
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Vertex {
    transaction: Transaction,
    outputs: Option<Outputs>,
    accounts_touched: HashSet<Address>,
    dependent_transactions: HashSet<String>
}

impl Vertex {
    pub fn new(
        transaction: Transaction,
        outputs: Option<Outputs>,
    ) -> Vertex {
        let accounts_touched = Vertex::extract_accounts_touched(
            &transaction,
            &outputs
        );
        Vertex { 
            transaction,
            outputs, 
            accounts_touched, 
            dependent_transactions: HashSet::new(),
        }
    }

    pub fn accounts_touched(&self) -> &HashSet<Address> {
        &self.accounts_touched
    }

    fn extract_accounts_touched(
        transaction: &Transaction,
        outputs: &Option<Outputs>
    ) -> HashSet<Address> {
        let mut accounts_involved = HashSet::new();
        accounts_involved.extend(transaction.get_accounts_involved());
        if let Some(o) = outputs {
            let involved: Vec<Address> = o.instructions().iter().map(|inst| {
                let nested: Vec<Address> = inst.get_accounts_involved()
                    .iter()
                    .filter_map(|addr| {
                        match addr {
                            AddressOrNamespace::This => Some(transaction.to()),
                            AddressOrNamespace::Address(address) => Some(address.clone()),
                            AddressOrNamespace::Namespace(_namespace) => None
                        }
                    }).collect();
                nested
            }).flatten().collect();
            accounts_involved.extend(involved);
        } 

        return accounts_involved
    }
}

#[derive(Clone, Debug)]
pub struct PendingGraph {
    vertices: HashMap<String, Arc<RwLock<Vertex>>>,
    account_index: HashMap<Address, HashSet<String>>
}

impl PendingGraph {
    pub fn new() -> PendingGraph {
        PendingGraph { 
            vertices: HashMap::new(),
            account_index: HashMap::new() 
        }
    }

    pub fn add_transaction(
        &mut self, 
        transaction: Transaction,
        outputs: Option<Outputs>,
    ) {
        log::info!("adding transaction: {} to dependency graph", &transaction.hash_string());
        let transaction_id = transaction.hash_string(); 

        let vertex = Arc::new(
            RwLock::new(
                Vertex::new(
                    transaction.clone(), 
                    outputs.clone()
                )
            )
        );

        let mut has_dependencies = false;

        if let Ok(guard) = vertex.read() {
            let vertex_accounts = guard.accounts_touched.clone(); 

            self.vertices.insert(
                transaction_id.clone(), 
                vertex.clone()
            );

            for account in vertex_accounts {
                let dependencies = self.account_index.entry(
                    account.clone()
                ).or_insert_with(HashSet::new);

                for dependency_id in dependencies.iter() {
                    if let Some(dep_vertex) = self.vertices.get(dependency_id) {
                        log::info!("found dependencies: {} for transaction: {}", &dependency_id, &transaction_id);
                        if let Ok(mut guard) = dep_vertex.write() {
                            guard.dependent_transactions.insert(transaction_id.clone());
                            has_dependencies = true;
                        }
                    }
                }
                dependencies.insert(transaction_id.clone());
            }
        };

        if !has_dependencies {
            log::info!("no dependencies found, scheduling with validator");
            self.schedule_with_validator(transaction, outputs);
        }
    }

    fn handle_valid(&mut self, validated_transaction_hash: &str) -> Vec<String> {
        log::info!("handling validated transaction");
        let mut transactions_ready_for_validation = Vec::new();
        if let Some(validated_vertex) = self.vertices.remove(validated_transaction_hash) {
            for (id, vertex) in self.vertices.iter() {
                log::info!("checking for dependent transactions");
                if let Ok(mut guard) = vertex.write() {
                    if guard.dependent_transactions.remove(
                        validated_transaction_hash
                    ) && guard.dependent_transactions.is_empty() {
                        log::info!("marking dependent transaction: {} ready for validation", &id);
                        transactions_ready_for_validation.push(id.clone());
                    }
                }
            }

            if let Ok(guard) = validated_vertex.read() {
                for account in guard.accounts_touched.iter() {
                    if let Some(transactions) = self.account_index.get_mut(account) {
                        log::info!(
                            "removing validated transaction {:?} from dependency graph for account: {}", 
                            &validated_transaction_hash, &account
                        );
                        transactions.remove(validated_transaction_hash);
                    }
                }
            }
        }

        transactions_ready_for_validation
    }

    fn get_transactions(
        &self,
        transaction_ids: Vec<String>
    ) -> Vec<(Transaction, Option<Outputs>)> {
        log::info!("acquiring transactions from the dependency graph");
        transaction_ids.into_iter().filter_map(|id| {
            if let Some(vertex) = self.vertices.get(&id) {
                if let Ok(guard) = vertex.read() {
                    Some((guard.transaction.clone(), guard.outputs.clone()))
                } else {
                    None
                }
            } else {
                None
            }
        }).collect()
    }

    fn schedule_with_validator(
        &mut self,
        transaction: Transaction,
        outputs: Option<Outputs>
    ) -> Result<(), Box<dyn std::error::Error>> {
        let validator: ActorRef<ValidatorMessage> = ractor::registry::where_is(
            ActorType::Validator.to_string()
        ).ok_or(
            PendingTransactionError
        ).map_err(|e| Box::new(e))?.into();
        log::info!("casting message to validator to validate transaction: {}", &transaction.hash_string());
        let message = match transaction.transaction_type() {
            TransactionType::Send(_) => { 
                ValidatorMessage::PendingTransaction { transaction }
            },
            TransactionType::Call(_) => {
                ValidatorMessage::PendingCall { outputs, transaction }
            },
            TransactionType::BridgeIn(_) => {
                ValidatorMessage::PendingTransaction { transaction }
            }
            _ => {
                return Err(
                    Box::new(
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "have not implemented validation for this transaction type"
                        )
                    ) as Box<dyn std::error::Error>
                )
            }
        };
        validator.cast(message)?;

        Ok(())
    }

    fn send_to_batcher(
        &self, 
        transaction: Transaction
    ) -> Result<(), Box<dyn std::error::Error>> {
        let batcher: ActorRef<BatcherMessage> = ractor::registry::where_is(
            ActorType::Batcher.to_string()
        ).ok_or(
            Box::new(PendingTransactionError)
        )?.into();

        let message = BatcherMessage::AppendTransaction { transaction, outputs: None };

        batcher.cast(message)?;

        Ok(())
    }

}

#[derive(Debug, Clone)]
pub struct PendingTransactionActor;

#[derive(Debug, Clone, Error)]
pub struct  PendingTransactionError;

impl Display for PendingTransactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", &self)
    }
}

#[async_trait]
impl Actor for PendingTransactionActor {
    type Msg = PendingTransactionMessage;
    type State = PendingGraph; 
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(PendingGraph::new()) 
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            PendingTransactionMessage::New { transaction, outputs }  => {
                state.add_transaction(transaction.clone(), outputs);
                log::info!("added transaction: {} to dependency graph", transaction.hash_string());
            }
            PendingTransactionMessage::Valid { transaction, .. } => {
                log::info!("received notice transaction is valid: {}", transaction.hash_string());
                let get_transactions = state.handle_valid(&transaction.hash_string());
                let transactions_ready_for_validation = state.get_transactions(
                    get_transactions
                );

                for (transaction, outputs) in transactions_ready_for_validation {
                    let _ = state.schedule_with_validator(transaction, outputs);
                }
            }
            PendingTransactionMessage::Invalid { transaction } => {
                log::error!("transaction: {} is invalid", transaction.hash_string());
            }
            PendingTransactionMessage::GetPendingTransaction { 
                transaction_hash, 
                sender 
            } => {
                log::info!("Pending transaction requested");
            }
            PendingTransactionMessage::ValidCall { .. } => {
                todo!()
            }
            PendingTransactionMessage::Confirmed { .. }=> {
                todo!()
            }
        }
        Ok(())
    }
}
