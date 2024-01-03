#![allow(unused)]
use ethereum_types::U256;
use secp256k1::{SecretKey, Secp256k1, Message};
use crate::{
    LasrRpcClient,
    PayloadBuilder, 
    Address, 
    TransactionType, 
    Account, 
    RecoverableSignature, 
    Transaction, Token
};

pub type WalletError = Box<dyn std::error::Error + Send>;
pub type WalletResult<T> = Result<T, WalletError>;

#[derive(Builder, Clone)]
pub struct Wallet<L> 
where 
    L: LasrRpcClient
{
    client: L,
    sk: SecretKey,
    builder: PayloadBuilder,
    address: Address,
    account: Account
}

impl<L: LasrRpcClient + Send + Sync> Wallet<L> {

    pub async fn send(
        &mut self,
        to: &Address,
        program_id: &Address,
        value: U256,
    ) -> WalletResult<Token> {
        let account = self.account();
        let address = self.address();

        account.validate_balance(program_id, value)?;

        let payload = self.builder
            .transaction_type(TransactionType::Send(account.nonce()))
            .from(address.into())
            .to(to.into())
            .program_id(program_id.into())
            .inputs(String::new())
            .op(String::new())
            .value(value)
            .build().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let msg = Message::from_digest_slice(&payload.hash()).map_err(|e| {
            Box::new(e) as Box<dyn std::error::Error + Send>
        })?;

        let context = Secp256k1::new();

        let sig: RecoverableSignature = context.sign_ecdsa_recoverable(&msg, &self.sk).into();

        let transaction: Transaction = (payload, sig.clone()).into();

        let token: Token = bincode::deserialize(
            &self.client.send(
                transaction.clone()
            ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?
        ).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.get_account(&self.address()).await?;

        Ok(token)
    }

    pub async fn call(
        &mut self,
        program_id: Address,
        to: Address,
        value: U256,
        op: String,
        inputs: String,
    ) -> WalletResult<()> {

        let account = self.account();
        let address = self.address();

        account.validate_balance(&program_id, value)?;

        let payload = self.builder 
            .transaction_type(TransactionType::Send(account.nonce()))
            .from(address.into())
            .to(to.into())
            .program_id(program_id.into())
            .inputs(inputs)
            .op(op)
            .value(value)
            .build().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let msg = Message::from_digest_slice(&payload.hash()).map_err(|e| {
            Box::new(e) as Box<dyn std::error::Error + Send>
        })?;

        let context = Secp256k1::new();

        let sig: RecoverableSignature = context.sign_ecdsa_recoverable(&msg, &self.sk).into();

        let transaction: Transaction = (payload, sig.clone()).into();

        let _token = self.client.call(
            transaction.clone()
        ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.get_account(&self.address()).await?;

        Ok(())
    }

    pub async fn deploy(&mut self, inputs: String) -> WalletResult<()> {
        let account = self.account();
        let address = self.address();

        let payload = self.builder 
            .transaction_type(TransactionType::Deploy(account.nonce()))
            .from(address.into())
            .to([0; 20])
            .program_id([0; 20])
            .inputs(inputs)
            .op(String::new())
            .value(0.into())
            .build().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let msg = Message::from_digest_slice(&payload.hash()).map_err(|e| {
            Box::new(e) as Box<dyn std::error::Error + Send>
        })?;

        let context = Secp256k1::new();

        let sig: RecoverableSignature = context.sign_ecdsa_recoverable(&msg, &self.sk).into();

        let transaction: Transaction = (payload, sig.clone()).into();

        //TODO: return `payment token` with approval set to Address(0), i.e. network
        //should be able to pull fees from the contract deployer/owner account
        let _ = self.client.deploy(
            transaction.clone()
        ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.get_account(&self.address()).await?;

        Ok(())
    }

    pub async fn get_account(&mut self, address: &Address) -> WalletResult<()> {
        let account: Account = bincode::deserialize(
            &self.client.get_account(format!("{:x}", address)).await.map_err(|e| {
                Box::new(e) as Box<dyn std::error::Error + Send>
            })?
        ).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.account = account;

        println!("\n");
        println!("****************** Wallet Balances ********************");
        println!("*       Token           |           Balance           *");
        println!("* ----------------------|---------------------------- *");
        for (id, token) in self.account().programs() {
            println!("*    {:<23}      | {:>23}     *", id, token.balance());
        }
        println!("*******************************************************");
        println!("\n");

        Ok(())
    }

    pub fn address(&self) -> Address {
        self.address
    }

    fn account_mut(&mut self) -> &mut Account {
        &mut self.account
    }

    pub(crate) fn account(&self) -> Account {
        self.account.clone()
    }
}
