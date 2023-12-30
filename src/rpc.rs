
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::core::Error;
use crate::{Token, TokenDelta, Transaction, Account};

#[rpc(client, server, namespace = "lasr")]
#[async_trait::async_trait]
pub trait LasrRpc {
    #[method(name = "call")]
    async fn call(
        &self,
        transaction: Transaction
    ) -> Result<Vec<TokenDelta>, Error>;
    
    #[method(name = "send")]
    async fn send(
        &self,
        transaction: Transaction
    ) -> Result<Token, Error>;

    #[method(name = "deploy")]
    async fn deploy(
        &self,
        transaction: Transaction
    ) -> Result<(), Error>;

    #[method(name = "getAccount")]
    async fn get_account(
        &self,
        address: String
    ) -> Result<Account, Error>;
}
