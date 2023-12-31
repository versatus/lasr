#![allow(unused)]
use std::{collections::{HashMap, VecDeque, BTreeMap}, fmt::Display};

use async_trait::async_trait;
use base64::{Engine as _, engine::{self, general_purpose}, alphabet};
use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Serialize, Deserialize};
use thiserror::Error;

use crate::{Transaction, Account, BatcherMessage, get_account, AccountBuilder};

const CUSTOM_ENGINE: engine::GeneralPurpose = engine::GeneralPurpose::new(&alphabet::URL_SAFE, general_purpose::PAD);

#[derive(Clone, Debug, Error)]
pub enum BatcherError {
    Custom(String)
}

impl Display for BatcherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Debug)]
pub struct BatcherActor;

#[derive(Builder, Clone, Debug, Serialize, Deserialize)]
pub struct Batch {
    transactions: HashMap<[u8; 32], Transaction>,
    accounts: HashMap<[u8; 20], Account>
}

impl Batch {
    pub fn new() -> Self {
        Self {
            transactions: HashMap::new(),
            accounts: HashMap::new(),
        }
    }

    pub(super) fn check_transaction_size(&self) -> Result<usize, BatcherError> {
        let encoded = CUSTOM_ENGINE.encode(bincode::serialize(&self.transactions).map_err(|e| {
            BatcherError::Custom(e.to_string())
        })?);

        Ok(encoded.as_bytes().len())
    }

    pub(super) fn check_account_size(&self) -> Result<usize, BatcherError> {
        let encoded = CUSTOM_ENGINE.encode(bincode::serialize(&self.accounts).map_err(|e| {
            BatcherError::Custom(e.to_string())
        })?);

        Ok(encoded.as_bytes().len())
    }

    pub(super) fn transaction_would_exceed_capacity(
        &self,
        transaction: Transaction
    ) -> Result<bool, BatcherError> {
        let mut test_batch = self.clone();
        test_batch.insert_transaction(transaction)?;
        test_batch.transactions_at_capacity()
    }

    pub(super) fn account_would_exceed_capacity(
        &self,
        account: Account
    ) -> Result<bool, BatcherError> {
        let mut test_batch = self.clone();
        test_batch.insert_account(account)?;
        test_batch.accounts_at_capacity()
    }

    pub(super) fn transactions_at_capacity(&self) -> Result<bool, BatcherError> {
        Ok(self.check_transaction_size()? >= 512 * 1024)
    }

    pub(super) fn accounts_at_capacity(&self) -> Result<bool, BatcherError> {
        Ok(self.check_account_size()? >= 512 * 1024)
    }

    pub fn insert_transaction(&mut self, transaction: Transaction) -> Result<(), BatcherError> {
        if !self.transaction_would_exceed_capacity(transaction.clone())? {
            let mut id: [u8; 32] = [0; 32];
            id.copy_from_slice(&transaction.hash());
            self.transactions.insert(id, transaction.clone());
            return Ok(())
        }

        Err(
            BatcherError::Custom(
                "transactions at capacity".to_string()
            )
        )
    }

    pub fn insert_account(&mut self, account: Account) -> Result<(), BatcherError> {
        if !self.account_would_exceed_capacity(account.clone())? {
            let mut id: [u8; 20] = account.address().into();
            self.accounts.insert(id, account.clone());
            return Ok(())
        }

        Err(
            BatcherError::Custom(
                "accounts at capacity".to_string()
            )
        )
    }

    pub fn transactions(&self) -> HashMap<[u8; 32], Transaction> {
        self.transactions.clone()
    }

    pub fn accounts(&self) -> HashMap<[u8; 20], Account> {
        self.accounts.clone()
    }
}

#[derive(Builder, Clone, Debug)]
pub struct Batcher {
    parent: Batch,
    children: VecDeque<Batch>
}

impl Batcher {
    pub fn new() -> Self {
       Self {
           parent: Batch::new(),
           children: VecDeque::new()
       }
    } 

    pub(super) async fn cache_account(&self, account: Account) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    pub(super) async fn add_transaction_to_batch(&mut self, transaction: Transaction) -> Result<(), Box<dyn std::error::Error>> {
        let mut new_batch = Batch::new();
        let mut res = self.parent.insert_transaction(transaction.clone());
        let mut iter = self.children.iter_mut();
        while let Err(_) = res {
            if let Some(mut child) = iter.next() {
                res = child.insert_transaction(transaction.clone());
            } else {
                new_batch.insert_transaction(transaction.clone());
            }
        }

        if new_batch.transactions().len() > 0 {
            self.children.push_back(new_batch)
        }

        Ok(())
    }

    pub(super) async fn add_account_to_batch(&mut self, account: Account) -> Result<(), Box<dyn std::error::Error>> {
        self.cache_account(account.clone()).await?;
        let mut new_batch = Batch::new();
        let mut res = self.parent.insert_account(account.clone());
        let mut iter = self.children.iter_mut();
        while let Err(_) = res {
            if let Some(mut child) = iter.next() {
                res = child.insert_account(account.clone())
            } else {
                new_batch.insert_account(account.clone());
            }
        }
        if new_batch.accounts().len() > 0 {
            self.children.push_back(new_batch);
        }

        Ok(())
    }

    pub(super) async fn add_transaction_to_account(&mut self, transaction: Transaction) -> Result<(), Box<dyn std::error::Error>> {
        let mut from_account = get_account(transaction.from()).await;
        let from_account = if let Some(mut account) = from_account {
            let _ = account.apply_transaction(transaction.clone());
            account
        } else {
            if !transaction.transaction_type().is_bridge_in() {
                return Err(Box::new(BatcherError::Custom("sender account does not exist".to_string())))
            }

            let mut account = AccountBuilder::default()
                .address(transaction.from())
                .programs(BTreeMap::new())
                .nonce(0.into())
                .build()?;

            let _ = account.apply_transaction(transaction.clone());
            account
        };

        let mut to_account = get_account(transaction.to()).await;
        let to_account = if let Some(mut account) = to_account {
            let mut account = AccountBuilder::default()
                .address(transaction.to())
                .programs(BTreeMap::new())
                .nonce(0.into())
                .build()?;

            let _ = account.apply_transaction(transaction.clone());
            account
        } else {
            let mut account = AccountBuilder::default()
                .address(transaction.from())
                .programs(BTreeMap::new())
                .nonce(0.into())
                .build()?;

            let _ = account.apply_transaction(transaction.clone());
            account
        };

        self.add_account_to_batch(from_account).await?;
        self.add_account_to_batch(to_account).await?;
        self.add_transaction_to_batch(transaction).await?;

        Ok(())
    }
}

impl BatcherActor {
    pub fn new() -> Self {
        BatcherActor
    }
}


#[async_trait]
impl Actor for BatcherActor {
    type Msg = BatcherMessage;
    type State = Batcher; 
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(Batcher::new()) 
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        _message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        Ok(())
    }
}
