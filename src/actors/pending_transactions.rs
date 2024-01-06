use std::collections::{HashMap, VecDeque};
use crate::{Address, PendingTransactionMessage, Transaction, ActorType, ValidatorMessage, BatcherMessage};
use async_trait::async_trait;
use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::fmt::Display;
use thiserror::Error;

#[derive(Debug)]
pub struct DependencyGraph {
    parent: Transaction,
    children: VecDeque<Transaction>,
}

impl DependencyGraph {
    pub fn new(
        parent: Transaction, 
    ) -> Self {
        Self { parent, children: VecDeque::new() }
    }

    pub(crate) async fn insert(
        &mut self,
        transaction: Transaction 
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.parent.hash() == transaction.hash() {
            log::warn!("transaction already in dependency graph, ignoring");
            return Ok(())
        }

        if self.children.contains(&transaction) {
            log::warn!("transaction already in dependency graph, ignoring");
            return Ok(())
        }
        self.children.push_back(transaction);
        Ok(())
    }

    pub(crate) async fn next(
        &mut self,
    ) -> Option<Transaction> {
        let new_parent = self.children.pop_front();
        if let Some(parent) = new_parent {
            self.parent = parent.clone();
            return Some(parent)
        }

        None
    }

    pub fn get(
        &self,
    ) -> &Transaction {
        &self.parent
    }

    pub fn get_mut(
        &mut self
    ) -> &mut Transaction {
        &mut self.parent
    }
}

#[derive(Debug)]
pub struct PendingTransactions {
    // User address -> ProgramId -> DependencyGraph 
    pending: HashMap<Address, HashMap<Address, DependencyGraph>>,
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

impl PendingTransactions {
    pub fn new() -> Self {
        let pending = HashMap::new();
        PendingTransactions { 
            pending
        }
    }

    async fn schedule_with_validator(&mut self, transaction: Transaction) -> Result<(), Box<dyn std::error::Error>> {
        let validator: ActorRef<ValidatorMessage> = ractor::registry::where_is(
            ActorType::Validator.to_string()
        ).ok_or(
            PendingTransactionError
        ).map_err(|e| Box::new(e))?.into();
        let message = ValidatorMessage::PendingTransaction { transaction };
        validator.cast(message)?;

        Ok(())
    }

    async fn handle_new_pending(
        &mut self,
        transaction: Transaction,
    ) -> Result<(), Box<dyn std::error::Error>> {
        log::info!("received new transaction: {}", transaction.hash_string());
        log::info!("transaction type: {:?}", transaction.transaction_type());
        if let Some(entry) = self.pending.get_mut(&transaction.from()) {
            log::info!("account exists in pending transactions");
            if let Some(graph) = entry.get_mut(&transaction.program_id()) {
                log::info!("program id exists in pending transactions");
                let _ = graph.insert(transaction).await?;
                return Ok(())
            } 

            let _ = entry.insert(
                transaction.program_id(),
                DependencyGraph::new(transaction.clone())
            );
            self.schedule_with_validator(transaction).await?;
            return Ok(())
        }

        log::info!("account doesn't exist in pending transactions yet, entering it");
        let mut graph = HashMap::new();
        graph.insert(transaction.program_id(), DependencyGraph::new(transaction.clone()));
        self.pending.insert(transaction.from(), graph);
        self.schedule_with_validator(transaction).await?;
        Ok(())
    }

    async fn handle_confirmed(
        &mut self,
        transaction: Transaction
    ) -> Result<(), Box<dyn std::error::Error>> {
        log::info!("handling confirmed transaction");
        let mut remove_graph: bool = false;
        let mut remove_user: bool = false;
        let mut new_parent: Option<Transaction> = None;
        if let Some(programs) = self.pending.get_mut(&transaction.from()) {
            log::info!("found program");
            if let Some(entry) = programs.get_mut(&transaction.program_id()) {
                log::info!("found transaction");
                if let Some(next) = entry.next().await {
                    log::info!("discovered child transaction, setting to parent");
                    new_parent = Some(next);
                } else {
                    remove_graph = true;
                }
            }

            if remove_graph {
                log::info!("no more transactions for user/program removing graph");
                programs.remove(&transaction.program_id());
            }

            if programs.len() == 0 {
                remove_user = true;
            }
        }

        if let Some(transaction) = new_parent {
            log::info!("found new parent transaction, sending to validator");
            let _ = self.schedule_with_validator(transaction).await?;
        }

        if remove_user {
            log::info!("no more transactions for user removing address");
            self.pending.remove(&transaction.from());
        }

        self.send_to_batcher(transaction).await?;

        Ok(())
    }

    async fn send_to_batcher(&self, transaction: Transaction) -> Result<(), Box<dyn std::error::Error>> {
        let batcher: ActorRef<BatcherMessage> = ractor::registry::where_is(ActorType::Batcher.to_string()).ok_or(
            Box::new(PendingTransactionError)
        )?.into();

        let message = BatcherMessage::AppendTransaction(transaction);

        batcher.cast(message)?;

        Ok(())
    }
}

#[async_trait]
impl Actor for PendingTransactionActor {
    type Msg = PendingTransactionMessage;
    type State = PendingTransactions; 
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(PendingTransactions::new()) 
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            PendingTransactionMessage::New { transaction }  => {
                let res = state.handle_new_pending(transaction).await;
                if let Err(e) = res {
                    log::error!("Encountered error adding transaction to pending pool {}", e);
                }
            }
            PendingTransactionMessage::Valid { transaction, .. } => {
                log::info!("received notice transaction is valid: {}", transaction.hash_string());
                let _ = state.handle_confirmed(transaction.clone()).await;
                // Send to batcher
                // batcher certifies transaction
                // batcher consolidates transactions from account
                // and applies updated account to cache until settlement
                // when account, transaction batch exceeds a certain size it is committed to DA
            }
            PendingTransactionMessage::Invalid { .. } => {}
            PendingTransactionMessage::Confirmed { map, .. }=> {
                for (_, tx) in map.into_iter() {
                    let _ = state.handle_confirmed(tx).await;
                }
            }
        }
        Ok(())
    }
}
