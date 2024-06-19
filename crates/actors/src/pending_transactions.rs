use lasr_messages::{
    ActorName, ActorType, ExecutorMessage, PendingTransactionMessage, SchedulerMessage,
    SupervisorType, ValidatorMessage,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::sync::mpsc::Sender;

use async_trait::async_trait;
use chrono::prelude::*;
use lasr_types::{Address, AddressOrNamespace, Outputs, Transaction, TransactionType};
use ractor::{Actor, ActorCell, ActorProcessingErr, ActorRef, SupervisionEvent};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    get_actor_ref, helpers::Coerce, process_group_changed, SchedulerError, ValidatorError,
};

pub const PENDING_TIMEOUT: u64 = 15000;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Vertex {
    transaction: Transaction,
    timestamp: u64,
    outputs: Option<Outputs>,
    accounts_touched: HashSet<Address>,
    dependent_transactions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PreCallVertex {
    transaction: Transaction,
    program_account: Address,
    dependencies: Vec<String>,
}

impl Vertex {
    pub fn new(transaction: Transaction, outputs: Option<Outputs>) -> Vertex {
        let accounts_touched = Vertex::extract_accounts_touched(&transaction, &outputs);

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
        outputs: &Option<Outputs>,
    ) -> HashSet<Address> {
        let mut accounts_involved = HashSet::new();
        accounts_involved.extend(transaction.get_accounts_involved());
        if let Some(o) = outputs {
            let involved: Vec<Address> = o
                .instructions()
                .iter()
                .flat_map(|inst| {
                    let nested: Vec<Address> = inst
                        .get_accounts_involved()
                        .iter()
                        .filter_map(|addr| match addr {
                            AddressOrNamespace::This => Some(transaction.to()),
                            AddressOrNamespace::Address(address) => Some(*address),
                            AddressOrNamespace::Namespace(_namespace) => None,
                        })
                        .collect();
                    nested
                })
                .collect();
            accounts_involved.extend(involved);
        }

        accounts_involved
    }
}

impl PreCallVertex {
    pub fn new(transaction: Transaction) -> PreCallVertex {
        let program_account = transaction.to();
        PreCallVertex {
            transaction,
            program_account,
            dependencies: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PreCallGraph {
    vertices: HashMap<String, Arc<RwLock<PreCallVertex>>>,
    program_index: HashMap<Address, Vec<String>>,
}

impl PreCallGraph {
    pub fn new() -> PreCallGraph {
        PreCallGraph {
            vertices: HashMap::new(),
            program_index: HashMap::new(),
        }
    }

    pub fn add_call(&mut self, transaction: Transaction) {
        tracing::info!(
            "adding call: {} to pre-call graph for program: {}",
            &transaction.hash_string(),
            &transaction.to()
        );
        let transaction_id = transaction.hash_string();

        // New transaction, new vertex, who dis
        let vertex = Arc::new(RwLock::new(PreCallVertex::new(transaction.clone())));

        // set has_dependencies flag to false
        let mut has_dependencies = false;

        // check if we can acquire the read guard
        if let Ok(guard) = vertex.read() {
            // guard acquired, get the transaction its related to and
            // pull the to address, this is the address to the program
            // being called
            let vertex_program_id = guard.transaction.to();

            // Insert the vertex into the graph vertices map, with transaction id/hash as key
            self.vertices.insert(transaction_id.clone(), vertex.clone());

            // Check if an entry for the program id exists in the program index.
            // if so this means it has dependencies
            let dependencies = self.program_index.entry(vertex_program_id).or_default();

            for dependency_id in dependencies.iter() {
                // For each dependency it has, get the relevant transaction vertex
                if let Some(dep_vertex) = self.vertices.get(dependency_id) {
                    tracing::warn!(
                        "found pre-call dependencies: {} for transaction: {}",
                        &dependency_id,
                        &transaction_id
                    );
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
            tracing::warn!("no dependencies found, executing");
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
                        return Ok(());
                    }
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "unable to acquire write guard on vertex for executed transaction",
                    ));
                }
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "unable to find vertex associated with transaction {}",
                        transaction_hash
                    ),
                ));
            }
        };

        if let Some(next_vtx) = self.vertices.get(&next) {
            if let Ok(guard) = next_vtx.read() {
                let transaction = guard.transaction.clone();
                let _ = self.send_to_executor(transaction);
            } else {
                //return error
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("unable to find vertex associated with transaction {}", next),
                ));
            }
        };

        Ok(())
    }

    fn send_to_executor(&self, transaction: Transaction) -> std::io::Result<()> {
        let executor: ActorRef<ExecutorMessage> =
            ractor::registry::where_is(ActorType::Executor.to_string())
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "unable to acquire executor actor",
                ))?
                .into();

        let message = ExecutorMessage::Exec { transaction };

        let _ = executor.cast(message);

        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct PendingGraph {
    vertices: HashMap<String, Arc<RwLock<Vertex>>>,
    account_index: HashMap<Address, VecDeque<String>>,
}

impl PendingGraph {
    pub fn new() -> PendingGraph {
        PendingGraph {
            vertices: HashMap::new(),
            account_index: HashMap::new(),
        }
    }

    pub fn clean_graph(&mut self) {
        // Look at all vertices, see if any have timed out,
        // if any have timed out, check if they have dependent transactions
        // queue dependent transactions up for execution.
        // check every 15 seconds.
        for (hash, vtx) in self.vertices.clone() {
            tracing::warn!("checking if {} has timed out", &hash);
            // Get this vertex
            let mut timed_out = false;
            // Check if we can get read guard
            if let Ok(guard) = vtx.read() {
                // Check if it is timed out
                let elapsed = Utc::now().timestamp_millis() as u64 - guard.timestamp;
                if elapsed >= PENDING_TIMEOUT {
                    // Set timedout flag
                    tracing::warn!("{} has indeed timed out", &hash);
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
                            tracing::warn!("collecting dependent transaction hashes");
                            deps.push(dep.clone());
                        }
                    }
                }
            }

            tracing::warn!("collecting transactions ready for validation");
            let ready_for_validation: Vec<String> = deps.iter().filter_map(|dep| {
                match self.vertices.get_mut(dep) {
                    Some(vtx) => {
                        match vtx.write() {
                            Ok(mut guard) => {
                                guard.accounts_touched_mut().iter().for_each(|account| {
                                    let account_dependencies = self.account_index.get_mut(account);
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
                                        act_deps.front().cloned()
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
                                tracing::error!("Error attempting to acquire write guard for vertex for transaction: {}: {}", &dep, e);
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
                            tracing::warn!("scheduling: {} with validator", transaction_hash);
                            let _ = self.schedule_with_validator(transaction, outputs);
                        }
                        Err(e) => tracing::error!(
                            "Unable to acquire read guard on vertex: {}: {}",
                            transaction_hash,
                            e
                        ),
                    }
                }
            }
        }
    }

    pub fn add_transaction(&mut self, transaction: Transaction, outputs: Option<Outputs>) {
        tracing::info!(
            "adding transaction: {} to dependency graph",
            &transaction.hash_string()
        );
        let transaction_id = transaction.hash_string();

        // Create a new vertex
        let vertex = Arc::new(RwLock::new(Vertex::new(
            transaction.clone(),
            outputs.clone(),
        )));

        // set dependency flag to false
        let mut has_dependencies = false;

        // get vertex read guard
        if let Ok(guard) = vertex.read() {
            // get accounts involved
            let vertex_accounts = guard.accounts_touched.clone();

            // insert the vertex into the vertices map
            self.vertices.insert(transaction_id.clone(), vertex.clone());

            for account in vertex_accounts {
                // for each account involved in the transaction
                // check if the account has an entry in the account index
                let dependencies = self
                    .account_index
                    .entry(
                        account, // If not insert a new VecDeque
                    )
                    .or_default();

                for dependency_id in dependencies.iter() {
                    // for each dependency in the account dependencies entry
                    if let Some(dep_vertex) = self.vertices.get(dependency_id) {
                        // get the vertex for the dependency
                        tracing::warn!(
                            "found dependencies: {} for transaction: {}",
                            &dependency_id,
                            &transaction_id
                        );
                        if let Ok(mut dep_guard) = dep_vertex.write() {
                            // add the current transaction to the back of its dependent
                            // transactionsvector if it does not already contain the
                            // dependent transaction
                            if !dep_guard
                                .dependent_transactions
                                .clone()
                                .contains(&transaction_id)
                            {
                                dep_guard
                                    .dependent_transactions
                                    .push(transaction_id.clone());
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
            tracing::warn!("no dependencies found, scheduling with validator");
            let _ = self.schedule_with_validator(transaction, outputs);
        }
    }

    fn handle_valid(&mut self, validated_transaction_hash: &str) -> Vec<String> {
        tracing::info!("handling validated transaction");
        let mut transactions_ready_for_validation = Vec::new();
        if let Some(validated_vertex) = self.vertices.remove(validated_transaction_hash) {
            for (id, vertex) in self.vertices.iter() {
                tracing::info!("checking for dependent transactions");
                if let Ok(mut guard) = vertex.write() {
                    if let Some(index) = guard
                        .dependent_transactions
                        .iter()
                        .position(|hash| hash == validated_transaction_hash)
                    {
                        let _ = guard.dependent_transactions.remove(index);
                        if guard.dependent_transactions.is_empty() {
                            tracing::info!(
                                "marking dependent transaction: {} ready for validation",
                                &id
                            );
                            transactions_ready_for_validation.push(id.clone());
                        }
                    }
                }
            }

            if let Ok(guard) = validated_vertex.read() {
                for account in guard.accounts_touched.iter() {
                    if let Some(transactions) = self.account_index.get_mut(account) {
                        tracing::info!(
                            "removing validated transaction {:?} from dependency graph for account: {}", 
                            &validated_transaction_hash, &account
                        );
                        if let Some(index) = &transactions
                            .iter()
                            .position(|hash| hash == validated_transaction_hash)
                        {
                            transactions.remove(*index);
                        }
                    }
                }
            }
        }

        transactions_ready_for_validation
    }

    fn handle_invalid(
        &mut self,
        invalid_transaction_hash: &str,
        e: Box<dyn std::error::Error + Send>,
    ) -> Result<Vec<String>, PendingTransactionError> {
        tracing::info!("handling invalid transaction");
        let mut transactions_ready_for_validation = Vec::new();
        if let Some(invalid_vertex) = self.vertices.remove(invalid_transaction_hash) {
            for (id, vertex) in self.vertices.iter() {
                tracing::info!("checking for dependenty transactions");
                if let Ok(mut guard) = vertex.write() {
                    if let Some(index) = guard
                        .dependent_transactions
                        .iter()
                        .position(|hash| hash == invalid_transaction_hash)
                    {
                        let _ = guard.dependent_transactions.remove(index);
                        if guard.dependent_transactions.is_empty() {
                            tracing::info!(
                                "marking dependent transaction: {} ready for validation",
                                &id
                            );
                            transactions_ready_for_validation.push(id.clone());
                        }
                    }
                }
            }

            if let Ok(guard) = invalid_vertex.read() {
                for account in guard.accounts_touched.iter() {
                    if let Some(transactions) = self.account_index.get_mut(account) {
                        tracing::info!(
                            "removing invalid transaction {:?} from depdendency graph for account: {}",
                            &invalid_transaction_hash, &account
                        );

                        if let Some(index) = &transactions
                            .iter()
                            .position(|hash| hash == invalid_transaction_hash)
                        {
                            transactions.remove(*index);
                        }
                    }
                }
            }
        }

        if let Some(scheduler) =
            get_actor_ref::<SchedulerMessage, SchedulerError>(ActorType::Scheduler)
        {
            let message = SchedulerMessage::SendTransactionFailure {
                transaction_hash: invalid_transaction_hash.to_string(),
                error: e,
            };

            scheduler.cast(message).typecast().log_err(|e| {
                SchedulerError::Custom(format!(
                    "failed to cast SendTransactionFailure to scheduler: {e:?}"
                ))
            });
        }

        Ok(transactions_ready_for_validation)
    }

    fn get_transactions(
        &self,
        transaction_ids: Vec<String>,
    ) -> Vec<(Transaction, Option<Outputs>)> {
        tracing::info!("acquiring transactions from the dependency graph");
        transaction_ids
            .into_iter()
            .filter_map(|id| {
                if let Some(vertex) = self.vertices.get(&id) {
                    if let Ok(guard) = vertex.read() {
                        Some((guard.transaction.clone(), guard.outputs.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    fn schedule_with_validator(
        &mut self,
        transaction: Transaction,
        outputs: Option<Outputs>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(validator) =
            get_actor_ref::<ValidatorMessage, ValidatorError>(ActorType::Validator)
        {
            tracing::warn!(
                "casting message to validator to validate transaction: {}",
                &transaction.hash_string()
            );
            let transaction_type = transaction.transaction_type();
            let message = match &transaction_type {
                TransactionType::Send(_) => ValidatorMessage::PendingTransaction { transaction },
                TransactionType::Call(_) => ValidatorMessage::PendingCall {
                    outputs,
                    transaction,
                },
                TransactionType::BridgeIn(_) => {
                    ValidatorMessage::PendingTransaction { transaction }
                }
                _ => {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "have not implemented validation for this transaction type",
                    )) as Box<dyn std::error::Error>)
                }
            };
            validator.cast(message).typecast().log_err(|e| {
                ValidatorError::Custom(format!(
                    "failed to cast {:?} message to ValidatorActor: {e:?}",
                    transaction_type
                ))
            });
        }

        Ok(())
    }
}

#[derive(Default)]
pub struct DependencyGraphs {
    pub pending: PendingGraph,
    pub pre_call: PreCallGraph,
}

impl DependencyGraphs {
    pub fn new() -> Self {
        Self {
            pending: PendingGraph::new(),
            pre_call: PreCallGraph::new(),
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
            tracing::error!("Error in handle_completed_exec: {e}");
        }
    }

    pub fn get_transactions(
        &self,
        transaction_ids: Vec<String>,
    ) -> Vec<(Transaction, Option<Outputs>)> {
        self.pending.get_transactions(transaction_ids)
    }

    pub fn schedule_with_validator(
        &mut self,
        transaction: Transaction,
        outputs: Option<Outputs>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.pending.schedule_with_validator(transaction, outputs)
    }

    pub fn handle_valid(&mut self, transaction_hash: &str) -> Vec<String> {
        self.pending.handle_valid(transaction_hash)
    }

    pub fn handle_invalid(
        &mut self,
        transaction_hash: &str,
        e: Box<dyn std::error::Error + Send>,
    ) -> Result<Vec<String>, PendingTransactionError> {
        self.pending.handle_invalid(transaction_hash, e)
    }

    pub fn clean_pending_graph(&mut self) {
        self.pending.clean_graph();
    }

    pub fn clean_pre_call_graph(&mut self) {
        todo!()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PendingTransactionActor {
    bridge_in_transactions: std::sync::Arc<tokio::sync::Mutex<Vec<Transaction>>>,
}
impl PendingTransactionActor {
    pub fn new() -> Self {
        Self {
            bridge_in_transactions: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}
impl ActorName for PendingTransactionActor {
    fn name(&self) -> ractor::ActorName {
        ActorType::PendingTransactions.to_string()
    }
}

#[derive(Debug, Clone, Error, Default)]
pub enum PendingTransactionError {
    #[default]
    #[error("failed to acquire PendingTransactionActor from registry")]
    RactorRegistryError,

    #[error("{0}")]
    Custom(String),
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
            PendingTransactionMessage::New {
                transaction,
                outputs,
            } => {
                tracing::warn!("received new transction {}", transaction.hash_string());
                if transaction.transaction_type().is_bridge_in() {
                    let mut bridge_in_transactions = self.bridge_in_transactions.lock().await;
                    match bridge_in_transactions.contains(&transaction) {
                        true => {
                            tracing::warn!("found duplicate bridge in transaction, skipping..");
                            return Ok(());
                        }
                        false => bridge_in_transactions.push(transaction.clone()),
                    }
                }
                state.add_transaction(transaction.clone(), outputs);
                tracing::warn!(
                    "added transaction: {} to dependency graph",
                    transaction.hash_string()
                );
            }
            PendingTransactionMessage::NewCall { transaction } => {
                tracing::warn!(
                    "received new call transaction {}",
                    transaction.hash_string()
                );
                state.add_call(transaction.clone());
                tracing::warn!(
                    "added call transaction: {} to pre-call dependency graph",
                    transaction.hash_string()
                );
            }
            PendingTransactionMessage::ExecSuccess { transaction } => {
                tracing::warn!(
                    "received notice that transaction {} successfully executed",
                    &transaction.hash_string()
                );
                state.handle_completed_exec(&transaction.hash_string());
            }
            PendingTransactionMessage::Valid { transaction, .. } => {
                tracing::info!(
                    "received notice transaction is valid: {}",
                    transaction.hash_string()
                );
                let get_transactions = state.handle_valid(&transaction.hash_string());
                let transactions_ready_for_validation = state.get_transactions(get_transactions);

                for (transaction, outputs) in transactions_ready_for_validation {
                    let _ = state.schedule_with_validator(transaction, outputs);
                }
            }
            PendingTransactionMessage::Invalid { transaction, e } => {
                tracing::error!("transaction: {} is invalid: {e}", transaction.hash_string());
                let transactions_ready_for_validation =
                    match state.handle_invalid(&transaction.hash_string(), e) {
                        Ok(get_transactions) => state.get_transactions(get_transactions),
                        Err(e) => {
                            tracing::error!("Error handling invalid transaction {e}");
                            vec![]
                        }
                    };

                for (transaction, outputs) in transactions_ready_for_validation {
                    let _ = state.schedule_with_validator(transaction, outputs);
                }
            }
            PendingTransactionMessage::GetPendingTransaction {
                transaction_hash: _,
                sender: _,
            } => {
                tracing::info!("Pending transaction requested");
            }
            PendingTransactionMessage::ValidCall { transaction, .. } => {
                let get_transactions = state.handle_valid(&transaction.hash_string());
                tracing::warn!("received valid transactions in pending transaction in graph for transaction: {}", transaction.hash_string());
                let transactions_ready_for_validation = state.get_transactions(get_transactions);

                for (transaction, outputs) in transactions_ready_for_validation {
                    tracing::warn!("scheduling: {} with validator", transaction.hash_string());
                    let _ = state.schedule_with_validator(transaction, outputs);
                }
            }
            PendingTransactionMessage::CleanGraph => {
                tracing::warn!("Attempting to clean pending graph");
                state.clean_pending_graph();
            }
            PendingTransactionMessage::Confirmed { .. } => {
                todo!()
            }
        }
        Ok(())
    }
}

pub async fn graph_cleaner() -> std::io::Result<()> {
    let pt_actor: ActorRef<PendingTransactionMessage> =
        ractor::registry::where_is(ActorType::PendingTransactions.to_string())
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::Other,
                "unable to acquire PendingTransactions Actor",
            ))?
            .into();

    loop {
        tokio::time::sleep(Duration::from_millis(PENDING_TIMEOUT)).await;
        let message = PendingTransactionMessage::CleanGraph;
        let _ = pt_actor.clone().cast(message);
    }
}

pub struct PendingTransactionSupervisor {
    panic_tx: Sender<ActorCell>,
}
impl PendingTransactionSupervisor {
    pub fn new(panic_tx: Sender<ActorCell>) -> Self {
        Self { panic_tx }
    }
}
impl ActorName for PendingTransactionSupervisor {
    fn name(&self) -> ractor::ActorName {
        SupervisorType::PendingTransaction.to_string()
    }
}
#[derive(Debug, Error, Default)]
pub enum PendingTransactionSupervisorError {
    #[default]
    #[error("failed to acquire PendingTransactionSupervisor from registry")]
    RactorRegistryError,
}

#[async_trait]
impl Actor for PendingTransactionSupervisor {
    type Msg = PendingTransactionMessage;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: SupervisionEvent,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        tracing::warn!("Received a supervision event: {:?}", message);
        match message {
            SupervisionEvent::ActorStarted(actor) => {
                tracing::info!(
                    "actor started: {:?}, status: {:?}",
                    actor.get_name(),
                    actor.get_status()
                );
            }
            SupervisionEvent::ActorPanicked(who, reason) => {
                tracing::error!("actor panicked: {:?}, err: {:?}", who.get_name(), reason);
                self.panic_tx.send(who).await.typecast().log_err(|e| e);
            }
            SupervisionEvent::ActorTerminated(who, _, reason) => {
                tracing::error!("actor terminated: {:?}, err: {:?}", who.get_name(), reason);
            }
            SupervisionEvent::PidLifecycleEvent(event) => {
                tracing::info!("pid lifecycle event: {:?}", event);
            }
            SupervisionEvent::ProcessGroupChanged(m) => {
                process_group_changed(m);
            }
        }
        Ok(())
    }
}
