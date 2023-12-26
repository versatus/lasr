use ethereum_types::U256;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::core::Error;
use crate::{account::Address, certificate::RecoverableSignature, Token, TokenDelta, Transaction};

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
}
