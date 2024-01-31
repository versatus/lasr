use crate::Instruction;
use lasr_types::{AddressOrNamespace, U256};
pub trait PayableContract {
    fn receive_payment(
        from: AddressOrNamespace,
        amount: Option<U256>,
        token_ids: Vec<U256>,
    ) -> Vec<Instruction>;
}
