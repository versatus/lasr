use std::{collections::{HashMap, HashSet}, sync::{Arc, RwLock}};
use lasr_messages::{
    PendingTransactionMessage,
    ActorType,
    ValidatorMessage, SchedulerMessage, ExecutorMessage,
};

use lasr_types::{
    Address,
    Outputs,
    AddressOrNamespace, 
    TransactionType,
    Transaction,
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PreCallVertex {
    transaction: Transaction,
    program_account: Address,
    dependencies: Vec<String>
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

impl PreCallVertex {
    pub fn new(
        transaction: Transaction
    ) -> PreCallVertex {
        let program_account = transaction.to();
        PreCallVertex { 
            transaction,
            program_account,
            dependencies: Vec::new() 
        }
    }
}

#[derive(Clone, Debug)]
pub struct PreCallGraph {
    vertices: HashMap<String, Arc<RwLock<PreCallVertex>>>,
    program_index: HashMap<Address, Vec<String>>
}

impl PreCallGraph {

    pub fn new() -> PreCallGraph {
        PreCallGraph { 
            vertices: HashMap::new(),
            program_index: HashMap::new() 
        }
    }

    pub fn add_call(
        &mut self,
        transaction: Transaction
    ) {
        log::info!("adding call: {} to pre-call graph for program: {}", &transaction.hash_string(), &transaction.to());
        let transaction_id = transaction.hash_string();

        // New transaction, new vertex, who dis
        let vertex = Arc::new(
            RwLock::new(
                PreCallVertex::new(
                    transaction.clone()
                )
            )
        );
        
        // set has_dependencies flag to false
        let mut has_dependencies = false;

        // check if we can acquire the read guard
        if let Ok(guard) = vertex.read() {
            // guard acquired, get the transaction its related to and 
            // pull the to address, this is the address to the program
            // being called
            let vertex_program_id = guard.transaction.to();

            // Insert the vertex into the graph vertices map, with transaction id/hash as key
            self.vertices.insert(
                transaction_id.clone(),
                vertex.clone()
            );
            
            // Check if an entry for the program id exists in the program index.
            // if so this means it has dependencies
            let dependencies = self.program_index.entry(
                vertex_program_id.clone()
            ).or_insert_with(Vec::new);
            
            for dependency_id in dependencies.iter() {
                // For each dependency it has, get the relevant transaction vertex 
                if let Some(dep_vertex) = self.vertices.get(dependency_id) {
                    log::warn!("found pre-call dependencies: {} for transaction: {}", &dependency_id, &transaction_id);
                    // For each dependent transaction vertex acquire a write guard
                    if let Ok(mut dep_guard) = dep_vertex.write() {
                        // check if the dependencies include the current transaction
                        if dep_guard.dependencies.contains(&transaction_id.clone()) {
                            // If not, add it.
                            dep_guard.dependencies.push(transaction_id.clone());
                        }
                        //If so don't add it, but set the has dependencies flag to true 
                        //in either case, as a dependency exists
                        has_dependencies = true;
                    }
                }
            }
        }

        // If the transaction has dependencies, it can't be executed yet 
        // if it does not, forward it to executor for execution
        if !has_dependencies {
            log::warn!("no dependencies found, executing");
            let _ = self.send_to_executor(transaction);
        }
    }

    fn handle_completed_exec(&mut self, transaction_hash: &str) -> std::io::Result<()> {
        // When a transaction has completed execution, get the next transaction 
        // dependency
        let next = {
            if let Some(exec_vertex) = self.vertices.remove(transaction_hash) {
                if let Ok(mut guard) = exec_vertex.write() {

                    if !guard.dependencies.is_empty() {
                        guard.dependencies.remove(0)
                    } else {
                        return Ok(())
                    }
                } else {
                    return Err(
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "unable to acquire write guard on vertex for executed transaction"
                        )
                    )
                }
            } else {
                return Err(
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("unable to find vertex associated with transaction {}", transaction_hash)
                    )
                )
            }
        };

        if let Some(next_vtx) = self.vertices.get(&next) {
            if let Ok(guard) = next_vtx.read() {
                let transaction = guard.transaction.clone();
                self.send_to_executor(transaction);
            } else {
                //return error
                return Err(
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("unable to find vertex associated with transaction {}", next)
                    )
                )
            }
        };

        Ok(())
    }

    fn send_to_executor(&self, transaction: Transaction) -> std::io::Result<()> {
        let executor: ActorRef<ExecutorMessage> = ractor::registry::where_is(ActorType::Executor.to_string()).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "unable to acquire executor actor")
        )?.into();

        let message = ExecutorMessage::Exec { transaction };

        let _ = executor.cast(message);

        Ok(())
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
                        if let Ok(mut dep_guard) = dep_vertex.write() {
                            dep_guard.dependent_transactions.insert(transaction_id.clone());
                            has_dependencies = true;
                        }
                    }
                }
                dependencies.insert(transaction_id.clone());
            }
        }

        if !has_dependencies {
            log::info!("no dependencies found, scheduling with validator");
            let _ = self.schedule_with_validator(transaction, outputs);
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

    fn handle_invalid(&mut self, invalid_transaction_hash: &str, e: Box<dyn std::error::Error + Send>) -> Result<Vec<String>,PendingTransactionError> {
        log::info!("handling invalid transaction");
        let mut transactions_ready_for_validation = Vec::new();
        if let Some(invalid_vertex) = self.vertices.remove(invalid_transaction_hash) {
            for (id, vertex) in self.vertices.iter() {
                log::info!("checking for dependenty transactions");
                if let Ok(mut guard) = vertex.write() {
                    if guard.dependent_transactions.remove(
                        invalid_transaction_hash
                    ) && guard.dependent_transactions.is_empty() {
                        log::info!("marking dependent transaction: {} ready for validation", &id);
                        transactions_ready_for_validation.push(id.clone());
                    }
                }
            }

            if let Ok(guard) = invalid_vertex.read() {
                for account in guard.accounts_touched.iter() {
                    if let Some(transactions) = self.account_index.get_mut(account) {
                        log::info!(
                            "removing invalid transaction {:?} from depdendency graph for account: {}",
                            &invalid_transaction_hash, &account
                        );
                        transactions.remove(invalid_transaction_hash);
                    }
                }
            }
        }

        let scheduler: ActorRef<SchedulerMessage> = ractor::registry::where_is(
            ActorType::Scheduler.to_string()
        ).ok_or(
            PendingTransactionError
        )?.into();

        let message = SchedulerMessage::SendTransactionFailure { 
            transaction_hash: invalid_transaction_hash.to_string(), 
            error: e 
        };

        let _ = scheduler.cast(message);
        Ok(transactions_ready_for_validation)
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
}

pub struct DependencyGraphs {
    pub pending: PendingGraph,
    pub pre_call: PreCallGraph,
}

impl DependencyGraphs {
    pub fn new() -> Self {
        Self {
            pending: PendingGraph::new(),
            pre_call: PreCallGraph::new()
        }
    }

    pub fn add_transaction(&mut self, transaction: Transaction, outputs: Option<Outputs>) {
        self.pending.add_transaction(transaction, outputs);
    }

    pub fn add_call(&mut self, transaction: Transaction) {
        self.pre_call.add_call(transaction);
    }

    pub fn handle_completed_exec(&mut self, transaction_hash: &str) {
        self.pre_call.handle_completed_exec(transaction_hash);
    }

    pub fn get_transactions(&self, transaction_ids: Vec<String>) -> Vec<(Transaction, Option<Outputs>)> {
        self.pending.get_transactions(transaction_ids)
    }

    pub fn schedule_with_validator(&mut self, transaction: Transaction, outputs: Option<Outputs>) -> Result<(), Box<dyn std::error::Error>> {
        self.pending.schedule_with_validator(transaction, outputs)
    }

    pub fn handle_valid(&mut self, transaction_hash: &str) -> Vec<String> {
        self.pending.handle_valid(transaction_hash)
    }

    pub fn handle_invalid(&mut self, transaction_hash: &str, e: Box<dyn std::error::Error + Send>) -> Result<Vec<String>, PendingTransactionError> {
        self.pending.handle_invalid(transaction_hash, e)
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
    type State = DependencyGraphs; 
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(DependencyGraphs::new()) 
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            PendingTransactionMessage::New { transaction, outputs }  => {
                log::info!("received new transction {}", transaction.hash_string());
                state.add_transaction(transaction.clone(), outputs);
                log::info!("added transaction: {} to dependency graph", transaction.hash_string());
            }
            PendingTransactionMessage::NewCall { transaction } => {
                log::warn!("received new call transaction {}", transaction.hash_string());
                state.add_call(transaction.clone());
                log::warn!("added call transaction: {} to pre-call dependency graph", transaction.hash_string());
            }
            PendingTransactionMessage::ExecSuccess { transaction } => {
                log::warn!("received notice that transaction {} successfully executed", &transaction.hash_string());
                state.handle_completed_exec(&transaction.hash_string());
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
            PendingTransactionMessage::Invalid { transaction, e } => {
                log::error!("transaction: {} is invalid: {e}", transaction.hash_string());
                let transactions_ready_for_validation = match state.handle_invalid(&transaction.hash_string(), e) {
                    Ok(get_transactions) => {
                        state.get_transactions(
                            get_transactions
                        )
                    }
                    Err(e) => {
                        log::error!("Error handling invalid transaction {e}");
                        vec![]
                    }
                };

                for (transaction, outputs) in transactions_ready_for_validation {
                    let _ = state.schedule_with_validator(transaction, outputs);
                }
            }
            PendingTransactionMessage::GetPendingTransaction { 
                transaction_hash: _, 
                sender: _ 
            } => {
                log::info!("Pending transaction requested");
            }
            PendingTransactionMessage::ValidCall { transaction, .. } => {
                let get_transactions = state.handle_valid(&transaction.hash_string());
                log::warn!("received valid transactions in pending transaction in graph for transaction: {}", transaction.hash_string());
                let transactions_ready_for_validation = state.get_transactions(
                    get_transactions
                );

                for (transaction, outputs) in transactions_ready_for_validation {
                    log::warn!("scheduling: {} with validator", transaction.hash_string());
                    let _ = state.schedule_with_validator(transaction, outputs);
                }
            }
            PendingTransactionMessage::Confirmed { .. }=> {
                todo!()
            }
        }
        Ok(())
    }
}
