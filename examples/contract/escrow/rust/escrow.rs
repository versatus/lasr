use std::io::{Read, Write};
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use ethereum_types::U256 as EthU256;
use lasr::{Outputs, U256};
use serde::Deserialize;
use serde::Serialize;

use lasr::{ 
    Address, AddressOrNamespace, ArbitraryData, DataValue, Inputs, Instruction,
    Namespace, PayableContract, ProgramField, ProgramFieldValue, ProgramUpdate,
    ProgramUpdateField, TokenOrProgramUpdate, TransferInstruction, UpdateInstruction
}; 

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
        payment_token: [u8; 20], 
        payment_token_amount: U256, 
        redeemer: [u8; 20],
        contracted_item_address: [u8; 20],
        contracted_item: U256,
        // Escrow can also be timed, probably in production would make sense 
        // to have 2 separate methods for timed deposits vs condition based deposits
        // and another for both
        maturity: Option<u128>,
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
        caller: [u8; 20],
        deposit_id: [u8; 32],
        conditions: Self::Conditions,
        item_address: [u8; 20],
        item: U256
    ) -> Vec<Instruction>;
    /// If the condition is not met in time, the depositor can revoke the escrow
    /// and receive their tokens/item back.
    fn revoke(
        deposit_id: [u8; 32],
        depositor_address: [u8; 20], 
        conditions: Self::Conditions,
    ) -> Vec<Instruction> {
        Vec::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscrowContract;

impl PayableContract for EscrowContract {
    const NAMESPACE: &'static str = "HelloEscrow"; 
    fn receive_payment(from: lasr::AddressOrNamespace, amount: Option<U256>, token_ids: Vec<U256>) -> Vec<Instruction> {
        vec![
            Instruction::Transfer(
                TransferInstruction::new(
                    AddressOrNamespace::Namespace(Namespace(Self::NAMESPACE.to_string())),
                    from,
                    lasr::AddressOrNamespace::Namespace(
                        Namespace(Self::NAMESPACE.to_string())
                    ),
                    amount,
                    token_ids
            ))
        ]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HelloEscrowConditions {
    pub deposit_id: Option<[u8; 32]>,
    pub depositor: [u8; 20],
    pub contracted_item_address: [u8; 20],
    pub contracted_item: U256, 
    pub redeemer: [u8; 20],
    pub payment_token: [u8; 20],
    pub payment_amount: U256,
    pub by: Option<u128> 
}

impl Escrow for EscrowContract {
    type Conditions = HelloEscrowConditions;
    fn deposit(
        depositor: [u8; 20],
        payment_token: [u8; 20], 
        payment_amount: U256, 
        redeemer: [u8; 20],
        contracted_item_address: [u8; 20],
        contracted_item: U256,
        // Escrow can also be timed, probably in production would make sense 
        // to have 2 separate methods for timed deposits vs condition based deposits
        // and another for both
        maturity: Option<u128>,
    ) -> Vec<Instruction> {
        // Create the conditions:
        // TODO(asmith): create a unique deposit_id
        let conditions = HelloEscrowConditions {
            deposit_id: None,
            depositor,
            contracted_item_address,
            contracted_item,
            redeemer,
            payment_token,
            payment_amount: payment_amount.into(),
            by: maturity
        };

        let program_update = generate_deposit_update(
            conditions, 
            Namespace(Self::NAMESPACE.to_string())
        );

        if program_update.len() == 0 { return vec![] }

        let update = UpdateInstruction::new(program_update);

        let transfer = generate_deposit_transfer(
            payment_token,
            depositor,
            payment_amount.into(),
            Namespace(Self::NAMESPACE.to_string())
        );

        vec![Instruction::Update(update), Instruction::Transfer(transfer)]
    }

    fn redeem(
        caller: [u8; 20],
        deposit_id: [u8; 32],
        conditions: HelloEscrowConditions,
        item_address: [u8; 20],
        item: U256
    ) -> Vec<Instruction> {
        if conditions.deposit_id != Some(deposit_id) {
            return vec![]
        }

        if conditions.contracted_item_address != item_address {
            return vec![]
        }

        if conditions.contracted_item != item {
            return vec![]
        }

        let transfer_item_instruction = Instruction::Transfer(
            TransferInstruction::new(
                AddressOrNamespace::Address(Address::from(item_address)),
                AddressOrNamespace::Address(Address::from(caller)),
                AddressOrNamespace::Address(Address::from(conditions.depositor)),
                None,
                vec![item.into()]
            ) 
        );

        let transfer_deposit_instruction = Instruction::Transfer(
            TransferInstruction::new(
                AddressOrNamespace::Address(Address::from(conditions.payment_token)),
                AddressOrNamespace::Namespace(Namespace(Self::NAMESPACE.to_string())),
                AddressOrNamespace::Address(Address::from(conditions.redeemer)),
                Some(conditions.payment_amount.into()),
                vec![]
            )
        );

        //TODO(asmith): Delete the instance of Conditions that were met here

        vec![transfer_item_instruction, transfer_deposit_instruction]
    }

    fn revoke(
        deposit_id: [u8; 32],
        depositor_address: [u8; 20], 
        conditions: HelloEscrowConditions
    ) -> Vec<Instruction> {
        let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(n) => n.as_millis(),
            Err(_) => return vec![]
        };

        if conditions.deposit_id != Some(deposit_id) {
            return vec![]
        }

        if conditions.depositor != depositor_address {
            return vec![]
        }

        if Some(timestamp) > conditions.by {
            let transfer_instruction = Instruction::Transfer(
                TransferInstruction::new(
                    AddressOrNamespace::Address(Address::from(conditions.payment_token)),
                    AddressOrNamespace::Namespace(Namespace(Self::NAMESPACE.to_string())),
                    AddressOrNamespace::Address(Address::from(depositor_address)),
                    Some(conditions.payment_amount.into()),
                    vec![]
                )
            );

            //TODO(asmith): Delete this condition entry from the ProgramAccount data
            return vec![transfer_instruction]
        }

        vec![]
    }
}

fn generate_deposit_update(
    conditions: HelloEscrowConditions, 
    namespace: Namespace,
) -> Vec<TokenOrProgramUpdate> {
    let serialized_conditions = bincode::serialize(&conditions);
    if let Err(_) = serialized_conditions {
        return vec![]
    }
    let account = AddressOrNamespace::Namespace(namespace);
    let program_field = ProgramField::Data;
    let data_value = DataValue::Extend(ArbitraryData::from(serialized_conditions.unwrap_or_default()));
    let program_field_value = ProgramFieldValue::Data(data_value);
    let program_field_updates = ProgramUpdateField::new(program_field, program_field_value);
    let program_update = ProgramUpdate::new(account, vec![program_field_updates]);
    vec![TokenOrProgramUpdate::ProgramUpdate(program_update)]
}

fn generate_deposit_transfer(
    payment_token: [u8; 20],
    depositor: [u8; 20],
    payment_amount: U256,
    program_account_namespace: Namespace,
) -> TransferInstruction {
    let transfer = TransferInstruction::new(
        AddressOrNamespace::Address(Address::from(payment_token)),
        AddressOrNamespace::Address(Address::from(depositor)),
        AddressOrNamespace::Namespace(program_account_namespace),
        Some(payment_amount.into()),
        vec![],
    ); 

    transfer
}

fn parse_inputs(inputs: Inputs) -> Result<Vec<Instruction>, std::io::Error> {
    let function = inputs.op;
    let instruction_results = execute_function(&function, inputs.inputs)?;

    Ok(instruction_results)
}

fn execute_function(function: &str, function_inputs: String) -> Result<Vec<Instruction>, std::io::Error> {
    match function {
        "deposit" => {
            let function_inputs: DepositInputs = serde_json::from_str(&function_inputs)?;
            return Ok(EscrowContract::deposit(
                function_inputs.depositor,
                function_inputs.payment_token,
                function_inputs.payment_amount,
                function_inputs.redeemer,
                function_inputs.contracted_item_address,
                function_inputs.contracted_item,
                function_inputs.maturity
            ))
        },
        "redeem" => {
            let function_inputs: RedeemInputs = serde_json::from_str(&function_inputs)?;
            return Ok(
                EscrowContract::redeem(
                    function_inputs.caller, 
                    function_inputs.deposit_id,
                    function_inputs.conditions,
                    function_inputs.item_address,
                    function_inputs.item
                )
            )
        },
        "revoke" => {
            let function_inputs: RevokeInputs = serde_json::from_str(&function_inputs)?;
            return Ok(
                EscrowContract::revoke(
                    function_inputs.deposit_id,
                    function_inputs.depositor_address,
                    function_inputs.conditions
                )
            )
        }
        _ => return Err(std::io::Error::new(std::io::ErrorKind::Other, "Invalid function name"))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DepositInputs {
    pub depositor: [u8; 20],
    pub payment_token: [u8; 20],
    pub payment_amount: U256,
    pub redeemer: [u8; 20],
    pub contracted_item_address: [u8; 20],
    pub contracted_item: U256,
    pub maturity: Option<u128>, 
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RedeemInputs {
    pub caller: [u8; 20],
    pub deposit_id: [u8; 32],
    pub conditions: HelloEscrowConditions,
    pub item_address: [u8; 20],
    pub item: U256,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RevokeInputs {
    pub deposit_id: [u8; 32],
    pub depositor_address: [u8; 20],
    pub conditions: HelloEscrowConditions,
}

fn main() -> Result<(), std::io::Error> {
    let mut input_str = String::new();
    match std::io::stdin().read_to_string(&mut input_str) {
        Ok(_) => {
            let inputs: Inputs = serde_json::from_str(&input_str)?;
            let instructions = parse_inputs(inputs.clone())?;
            let output = Outputs::new(inputs, instructions);
            let output_json = serde_json::to_string(&output)?;
            std::io::stdout().write_all(output_json.as_bytes())?;
        }
        Err(e) => {
            std::io::stderr().write_all(e.to_string().as_bytes())?;
        }
    }

    Ok(())
}
