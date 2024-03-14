use std::{collections::{HashMap, HashSet, VecDeque}, sync::{Arc, RwLock}, time::Duration};
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
use chrono::prelude::*;

pub const PENDING_TIMEOUT: u64 = 15000;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Vertex {
    transaction: Transaction,
    timestamp: u64,
    outputs: Option<Outputs>,
    accounts_touched: HashSet<Address>,
    dependent_transactions: Vec<String>
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

        let timestamp = Utc::now().timestamp_millis() as u64;
        Vertex { 
            transaction,
            timestamp,
            outputs, 
            accounts_touched, 
            dependent_transactions: Vec::new(),
        }
    }

    pub fn accounts_touched(&self) -> &HashSet<Address> {
        &self.accounts_touched
    }
    
    pub fn accounts_touched_mut(&mut self) -> &mut HashSet<Address> {
        &mut self.accounts_touched
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
                let _ = self.send_to_executor(transaction);
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
    account_index: HashMap<Address, VecDeque<String>>
}

impl PendingGraph {
    pub fn new() -> PendingGraph {
        PendingGraph { 
            vertices: HashMap::new(),
            account_index: HashMap::new() 
        }
    }

    pub fn clean_graph(&mut self) {
        // Look at all vertices, see if any have timed out,
        // if any have timed out, check if they have dependent transactions
        // queue dependent transactions up for execution.
        // check every 15 seconds.
        for (hash, vtx) in self.vertices.clone() {
            log::warn!("checking if {} has timed out", &hash);
            // Get this vertex
            let mut timed_out = false;
            // Check if we can get read guard
            if let Ok(guard) = vtx.read() {
                // Check if it is timed out
                let elapsed = guard.timestamp - Utc::now().timestamp_millis() as u64;
                if elapsed >= PENDING_TIMEOUT {
                    // Set timedout flag
                    log::warn!("{} has indeed timed out", &hash);
                    timed_out = true;
                }
            }

            let mut deps = Vec::new();
            if timed_out {
                // If timed out
                if let Some(vtx) = self.vertices.remove(&hash) {
                    // get the vertex
                    if let Ok(guard) = vtx.read() {
                        // get the dependent transaction hashes and push them into 
                        // a vector
                        for dep in &guard.dependent_transactions {
                            log::warn!("collecting dependent transaction hashes");
                            deps.push(dep.clone());
                        }
                    }
                }
            }

            log::warn!("collecting transactions ready for validation");
            let ready_for_validation: Vec<String> = deps.iter().filter_map(|dep| {
                match self.vertices.get_mut(dep) {
                    Some(vtx) => {
                        match vtx.write() {
                            Ok(mut guard) => {
                                guard.accounts_touched_mut().iter().for_each(|account| {
                                    let account_dependencies = self.account_index.get_mut(&account);
                                    if let Some(act_deps) = account_dependencies {
                                        if let Some(index) = &act_deps.iter().position(|h| h.clone() == hash) {
                                            act_deps.remove(*index);
                                        }
                                    }
                                });

                                let accounts_involved = guard.accounts_touched().clone();
                                let act_next: VecDeque<String> = accounts_involved.iter().filter_map(|acct| {
                                    let account_dependencies = self.account_index.get(acct);
                                    if let Some(act_deps) = account_dependencies {
                                        if let Some(first) = act_deps.get(0) {
                                            Some(first.clone())
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                }).collect();

                                if act_next.iter().all(|h| h == dep) {
                                    Some(dep.clone())
                                } else {
                                    None
                                }
                            }
                            Err(e) => {
                                log::error!("Error attempting to acquire write guard for vertex for transaction: {}: {}", &dep, e);
                                None
                            }
                        }
                    }
                    None => None
                }
            }).collect();

            for transaction_hash in ready_for_validation {
                if let Some(vtx) = self.vertices.clone().get(&transaction_hash) {
                    match vtx.read() {
                        Ok(guard) => {
                            let transaction = guard.transaction.clone();
                            let outputs = guard.outputs.clone();
                            log::warn!("scheduling: {} with validator", transaction_hash);
                            let _ = self.schedule_with_validator(transaction, outputs);
                        } 
                        Err(e) => log::error!("Unable to acquire read guard on vertex: {}: {}", transaction_hash, e),
                    }
                }
            }
        }
    }

    pub fn add_transaction(
        &mut self, 
        transaction: Transaction,
        outputs: Option<Outputs>,
    ) {
        log::info!("adding transaction: {} to dependency graph", &transaction.hash_string());
        let transaction_id = transaction.hash_string(); 

        // Create a new vertex
        let vertex = Arc::new(
            RwLock::new(
                Vertex::new(
                    transaction.clone(), 
                    outputs.clone()
                )
            )
        );

        // set dependency flag to false
        let mut has_dependencies = false;

        // get vertex read guard
        if let Ok(guard) = vertex.read() {
            // get accounts involved
            let vertex_accounts = guard.accounts_touched.clone(); 

            // insert the vertex into the vertices map
            self.vertices.insert(
                transaction_id.clone(), 
                vertex.clone()
            );

            for account in vertex_accounts {
                // for each account involved in the transaction
                // check if the account has an entry in the account index
                let dependencies = self.account_index.entry(
                    account.clone()
                    // If not insert a new VecDeque
                ).or_insert_with(VecDeque::new);

                for dependency_id in dependencies.iter() {
                    // for each dependency in the account dependencies entry
                    if let Some(dep_vertex) = self.vertices.get(dependency_id) {
                        // get the vertex for the dependency
                        log::warn!("found dependencies: {} for transaction: {}", &dependency_id, &transaction_id);
                        if let Ok(mut dep_guard) = dep_vertex.write() {
                            // add the current transaction to the back of its dependent
                            // transactionsvector if it does not already contain the 
                            // dependent transaction
                            if !dep_guard.dependent_transactions.clone().contains(&transaction_id) {
                                dep_guard.dependent_transactions.push(transaction_id.clone());
                            }
                            // set the dependency flag to true
                            has_dependencies = true;
                        }
                    }
                }
                if has_dependencies {
                    // If it does have dependencies, push this transaction
                    // to the back of the account dependencies queue
                    dependencies.push_back(transaction_id.clone());
                }
            }
        }

        if !has_dependencies {
            // If there are no dependencies, then go ahead and schedule the 
            // transaction with the validator
            log::warn!("no dependencies found, scheduling with validator");
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
                    if let Some(index) = guard.dependent_transactions.iter().position(|hash| hash == validated_transaction_hash) {
                        let _ = guard.dependent_transactions.remove(index);
                        if guard.dependent_transactions.is_empty() {
                            log::info!("marking dependent transaction: {} ready for validation", &id);
                            transactions_ready_for_validation.push(id.clone());
                        }
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
                        if let Some(index) = &transactions.iter().position(|hash| hash == validated_transaction_hash) {
                            transactions.remove(*index);
                        }
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
                    if let Some(index) = guard.dependent_transactions.iter().position(|hash| hash == invalid_transaction_hash) {
                        let _ = guard.dependent_transactions.remove(index);
                        if guard.dependent_transactions.is_empty() {
                            log::info!("marking dependent transaction: {} ready for validation", &id);
                            transactions_ready_for_validation.push(id.clone());
                        }
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

                        if let Some(index) = &transactions.iter().position(|hash| hash == invalid_transaction_hash) {
                            transactions.remove(*index);
                        }
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
        log::warn!("casting message to validator to validate transaction: {}", &transaction.hash_string());
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
        if let Err(e) = self.pre_call.handle_completed_exec(transaction_hash) { 
            log::error!("Error in handle_completed_exec: {e}");
        }
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
    
    pub fn clean_pending_graph(&mut self) {
        self.pending.clean_graph();
    }

    pub fn clean_pre_call_graph(&mut self) {
        todo!()
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
                log::warn!("received new transction {}", transaction.hash_string());
                state.add_transaction(transaction.clone(), outputs);
                log::warn!("added transaction: {} to dependency graph", transaction.hash_string());
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
            PendingTransactionMessage::CleanGraph => {
                log::warn!("Attempting to clean pending graph");
                state.clean_pending_graph();
            }
            PendingTransactionMessage::Confirmed { .. }=> {
                todo!()
            }
        }
        Ok(())
    }
}


pub async fn graph_cleaner() -> std::io::Result<()> {
    let pt_actor: ActorRef<PendingTransactionMessage> = ractor::registry::where_is(
        ActorType::PendingTransactions.to_string()
    ).ok_or(
        std::io::Error::new(
            std::io::ErrorKind::Other,
            "unable to acquire PendingTransactions Actor"
        )
    )?.into();

    loop {
        tokio::time::sleep(Duration::from_millis(PENDING_TIMEOUT)).await;
        let message = PendingTransactionMessage::CleanGraph;
        let _ = pt_actor.clone().cast(message);
    }
}
