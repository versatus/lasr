use ethereum_types::U256;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::core::Error;
use crate::{account::Address, certificate::RecoverableSignature, Token, TokenDelta};

#[rpc(client, server, namespace = "lasr")]
#[async_trait::async_trait]
pub trait LasrRpc {
    #[method(name = "call")]
    async fn call(
        &self,
        program_id: Address,
        from: Address,
        to: Address,
        value: U256,
        op: String,
        inputs: String,
        sig: RecoverableSignature,
        nonce: U256, 
    ) -> Result<Vec<TokenDelta>, Error>;
    
    #[method(name = "send")]
    async fn send(
        &self,
        program_id: Address,
        from: Address,
        to: Address,
        amount: U256,
        sig: RecoverableSignature,
        nonce: U256, 
    ) -> Result<Token, Error>;

    #[method(name = "deploy")]
    async fn deploy(
        &self,
        program_id: Address,
        sig: RecoverableSignature,
        nonce: U256
    ) -> Result<(), Error>;
}
