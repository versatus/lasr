use jsonrpsee::core::Error;
use jsonrpsee::proc_macros::rpc;
use lasr_types::Transaction;

#[rpc(client, server, namespace = "lasr")]
#[async_trait::async_trait]
pub trait LasrRpc {
    #[method(name = "call")]
    async fn call(&self, transaction: Transaction) -> Result<String, Error>;

    #[method(name = "send")]
    async fn send(&self, transaction: Transaction) -> Result<Vec<u8>, Error>;

    #[method(name = "registerProgram")]
    async fn register_program(&self, transaction: Transaction) -> Result<(), Error>;

    #[method(name = "getAccount")]
    async fn get_account(&self, address: String) -> Result<String, Error>;
}
