use ethereum_types::U256;
use secp256k1::{SecretKey, Secp256k1, Message};
use crate::{
    LasrRpcClient,
    PayloadBuilder, 
    Address, 
    TransactionType, 
    Account, 
    RecoverableSignature, 
    Transaction
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
        to: Address,
        program_id: Address,
        value: U256,
    ) -> WalletResult<()> {
        let account = self.account();
        let address = self.address();

        account.validate_balance(&program_id, value)?;

        let payload = self.builder
            .transaction_type(TransactionType::Send(account.nonce()))
            .from(address.into())
            .to(to.into())
            .program_id(program_id.into())
            .inputs(String::new())
            .value(value)
            .build().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let msg = Message::from_digest_slice(&payload.hash()).map_err(|e| {
            Box::new(e) as Box<dyn std::error::Error + Send>
        })?;

        let context = Secp256k1::new();

        let sig: RecoverableSignature = context.sign_ecdsa_recoverable(&msg, &self.sk).into();

        let transaction: Transaction = (payload, sig.clone()).into();

        let token = self.client.send(
            transaction.clone()
        ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.account_mut().apply_send_transaction(transaction, token)?;

        Ok(())
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

        let token_deltas = self.client.call(
            transaction.clone()
        ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.account_mut().apply_call_transaction(transaction, token_deltas)?;

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

        Ok(())
    }

    fn address(&self) -> Address {
        self.address
    }

    fn account_mut(&mut self) -> &mut Account {
        &mut self.account
    }

    fn account(&self) -> Account {
        self.account.clone()
    }
}
