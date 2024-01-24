use ethereum_types::U256 as EthU256;
use crate::{Instruction, AddressOrNamespace, U256};

// May want to change the name of this trait to `PayableContract`
pub trait PayableContract {
    const NAMESPACE: &'static str;
    fn receive_payment(from: AddressOrNamespace, amount: Option<U256>, token_ids: Vec<U256>) -> Vec<Instruction>;
}
