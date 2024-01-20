
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::core::Error;
use crate::{Token, Transaction};

#[rpc(client, server, namespace = "lasr")]
#[async_trait::async_trait]
pub trait LasrRpc {
    #[method(name = "call")]
    async fn call(
        &self,
        transaction: Transaction
    ) -> Result<Vec<Token>, Error>;
    
    #[method(name = "send")]
    async fn send(
        &self,
        transaction: Transaction
    ) -> Result<Vec<u8>, Error>;

    #[method(name = "registerProgram")]
    async fn register_program(
        &self,
        transaction: Transaction 
    ) -> Result<(), Error>;

    #[method(name = "getAccount")]
    async fn get_account(
        &self,
        address: String
    ) -> Result<Vec<u8>, Error>;
}
