use ethereum_types::U256;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::core::Error;
use crate::{account::Address, certificate::RecoverableSignature, Token};

#[rpc(client, server, namespace = "lasr")]
#[async_trait::async_trait]
pub trait LasrRpc {
    #[method(name = "call")]
    async fn call(
        &self,
        program_id: Address,
        from: Address,
        op: String,
        inputs: String,
        sig: RecoverableSignature 
    ) -> Result<Token, Error>;
    
    #[method(name = "send")]
    async fn send(
        &self,
        program_id: Address,
        from: Address,
        to: Address,
        amount: U256,
        sig: RecoverableSignature
    ) -> Result<Token, Error>;

    #[method(name = "deploy")]
    async fn deploy(
        &self,
        program_id: Address,
        sig: RecoverableSignature,
    ) -> Result<(), Error>;
}
