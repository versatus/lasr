use std::time::Duration;
use std::time::Instant;
use lasr::Instruction;
use lasr::PayableContract;
use ethereum_types::U256;
use serde::Deserialize;
use serde::Serialize;

pub trait Escrow: PayableContract + Serialize + Deserialize<'static> {
    type Conditions: Serialize + Deserialize<'static>;
    /// Takes an address from a depositor, and address to a redeemer,
    /// the deposit token address, the deposit token amount/ID, as well as an 
    /// optional map of conditions which when all are met should equal the 
    /// full deposit amount +/- a fee, anything left over would be auto assumed
    /// to be a fee for the "owner" of this `Escrow` contract.
    ///
    /// Method produces instructions, should produce a minimum of 2 Instructions:
    /// `Transfer`, to tell the protocol to take `deposit` in the `Token` represented 
    /// by `deposit_token_address` from `depositor` account and add it to 
    /// the balance of a `program_address`, i.e. an address generated by the
    /// `generate_program_address` method. 
    ///
    /// Should also generate an `Update` instruction to tell the protocol 
    /// to `Update` the program address account with the Conditions and Maturity
    /// in this scenario, the `Token` `arbitrary_data` field will be used to 
    /// store the conditions and maturity in.
    ///
    /// A third Instruction to `Update` the depositors token arbirary data field 
    /// with a deposit_id which can be a 32 byte array representing a unique id
    /// which can be returned to the depositor so the depositor can inform the redeemer 
    /// of the deposit ID that will need to be used to inform the contract which 
    /// condition is attempting to be met.
    fn deposit(
        depositor: [u8; 20],
        redeemer: [u8; 20],
        deposit_token_address: [u8; 20], 
        deposit: U256, 
        conditions: Option<Self::Conditions>,
        // Escrow can also be timed, probably in production would make sense 
        // to have 2 separate methods for timed deposits vs condition based deposits
        // and another for both
        maturity: Option<Duration>,
    ) -> Vec<Instruction>; 
    /// Redeemer calls providing the deposit_id, the item_address and the item amount/id 
    /// the protcol `Read`s the program account as a pre-condition (as well as the 
    /// redeemer account to verify the balance/existence of the item). When this 
    /// call is made, the logic implemented here should deserialize the conditions
    /// should check if the condition(s) are met, and will then return 4 `Instruction`s
    /// 1. Transfer the item to the depositor
    /// 2. Transfer the the amount/item for the condition in the deposit_token to the redeemer
    /// from the escrow program account
    /// 3. Update the depositor token to remove the deposit ID 
    /// 4. Update the contract account to "mark" conditions as met, or remove the 
    /// pending escrow altogether so it cannot be triggered again in the future 
    /// to drain the program account from that token
    /// 
    /// An alternative to 4 is that for each escrow a new program account address
    /// can be generated and therefore once the condition(s) are fully met the 
    /// account is zeroed out, or only maintains the fee to the contract owner
    fn redeem(
        escrow_address: [u8; 32],
        redeemer: [u8; 20],
        deposit_token_address: [u8; 20],
        deposit_id: Option<[u8; 32]>,
        condition_proof: impl AsRef<[u8]>, 
        item_address: [u8; 20],
        item: U256
    ) -> Vec<Instruction>;
    /// If the condition is not met in time, the depositor can revoke the escrow
    /// and receive their tokens/item back.
    fn revoke(
        escrow_address: [u8; 32],
        deposit_id: Option<[u8; 32]>,
        depositor_address: [u8; 32], 
        timestamp: Instant, 
        maturity: Duration
    ) -> Vec<Instruction>;
    /// Generates a program address for the contract, all contracts that
    /// plan on storing value must have this method implemented.
    fn generate_program_address(
        &self,
        program_id: [u8; 32],
        seed: &[u8]
    ) -> Result<[u8;32], String> {
        PayableContract::generate_program_address(self, program_id, seed)
    }
}

fn main() {
}