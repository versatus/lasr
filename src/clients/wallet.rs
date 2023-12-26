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

        let payload = PayloadBuilder::default()
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

        let result = self.client.send(
            program_id,
            address,
            to,
            value,
            sig,
            account.nonce()
        ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.account_mut().apply_send_transaction(transaction)?;

        Ok(())
    }

    fn call(&self) -> WalletResult<()> {
        todo!()
    }

    fn address(&self) -> Address {
        todo!()
    }

    fn account_mut(&mut self) -> &mut Account {
        &mut self.account
    }

    fn account(&self) -> Account {
        todo!()
    }
}
