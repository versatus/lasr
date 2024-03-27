use crate::{check_account_cache, check_da_for_account};
use async_trait::async_trait;
use lasr_messages::{ActorType, BatcherMessage, PendingTransactionMessage, ValidatorMessage};
use ractor::{Actor, ActorProcessingErr, ActorRef};
use std::{collections::HashMap, fmt::Display};
use thiserror::Error;

use lasr_types::{
    Account, AccountType, AddressOrNamespace, Instruction, Outputs, TokenFieldValue,
    TokenOrProgramUpdate, Transaction, TransactionType,
};

#[derive(Debug)]
pub struct ValidatorCore {
    pool: rayon::ThreadPool,
}

impl Default for ValidatorCore {
    fn default() -> Self {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(50)
            .build()
            .unwrap();

        Self { pool }
    }
}

impl ValidatorCore {
    fn validate_bridge_in(
        &self,
    ) -> impl FnOnce(Transaction) -> Result<(), Box<dyn std::error::Error + Send>> {
        |tx| {
            if tx.from() != tx.to() {
                return Err(Box::new(ValidatorError::Custom(
                    "bridge tx from and to must be the same".to_string(),
                )) as Box<dyn std::error::Error + Send>);
            }

            let batcher: ActorRef<BatcherMessage> =
                ractor::registry::where_is(ActorType::Batcher.to_string())
                    .ok_or(Box::new(ValidatorError::Custom(
                        "unable to acquire pending transaction actor".to_string(),
                    )) as Box<dyn std::error::Error + Send>)?
                    .into();
            let message = BatcherMessage::AppendTransaction {
                transaction: tx.clone(),
                outputs: None,
            };
            batcher
                .cast(message)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            Ok(())
        }
    }

    fn validate_send(
        &self,
    ) -> impl FnOnce(Transaction, Account) -> Result<(), Box<dyn std::error::Error + Send>> {
        |tx, account| {
            let pending_transactions: ActorRef<PendingTransactionMessage> =
                ractor::registry::where_is(ActorType::PendingTransactions.to_string())
                    .ok_or(Box::new(ValidatorError::Custom(
                        "unable to acquire pending transaction actor".to_string(),
                    )) as Box<dyn std::error::Error + Send>)?
                    .into();

            match (
                account.validate_program_id(&tx.program_id()),
                account.validate_balance(&tx.program_id(), tx.value()),
                account.validate_nonce(tx.nonce()),
                tx.verify_signature()
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>),
            ) {
                (Err(e), _, _, _) | (_, Err(e), _, _) | (_, _, Err(e), _) | (_, _, _, Err(e)) => {
                    let error_string = e.to_string();
                    let message = PendingTransactionMessage::Invalid {
                        transaction: tx.clone(),
                        e,
                    };
                    let _ = pending_transactions.cast(message);
                    log::error!("{}", &error_string);
                    return Err(Box::new(ValidatorError::Custom(error_string))
                        as Box<dyn std::error::Error + Send>);
                }
                _ => {}
            }

            let batcher: ActorRef<BatcherMessage> =
                ractor::registry::where_is(ActorType::Batcher.to_string())
                    .ok_or(Box::new(ValidatorError::Custom(
                        "unable to acquire batcher actor".to_string(),
                    )) as Box<dyn std::error::Error + Send>)?
                    .into();

            let message = BatcherMessage::AppendTransaction {
                transaction: tx.clone(),
                outputs: None,
            };
            batcher
                .cast(message)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            Ok(())
        }
    }

    #[allow(unused)]
    fn validate_call(
        &self,
    ) -> impl FnOnce(
        HashMap<AddressOrNamespace, Option<Account>>,
        Outputs,
        Transaction,
    ) -> Result<(), Box<dyn std::error::Error + Send>> {
        |account_map, outputs, tx| {
            let pending_transactions: ActorRef<PendingTransactionMessage> =
                ractor::registry::where_is(ActorType::PendingTransactions.to_string())
                    .ok_or(Box::new(ValidatorError::Custom(
                        "unable to acquire pending transaction actor".to_string(),
                    )) as Box<dyn std::error::Error + Send>)?
                    .into();
            log::warn!("attempting to validate call: {}", tx.hash_string());

            if let Err(e) = tx.verify_signature() {
                let error_string = e.to_string();
                let message = PendingTransactionMessage::Invalid {
                    transaction: tx.clone(),
                    e: Box::new(e),
                };
                let _ = pending_transactions.cast(message);
                return Err(Box::new(ValidatorError::Custom(error_string))
                    as Box<dyn std::error::Error + Send>);
            }

            log::warn!("signature is valid");
            log::warn!("acquiring caller from account map");
            let caller = match account_map.get(&AddressOrNamespace::Address(tx.from())) {
                Some(Some(account)) => account,
                _ => {
                    let error_string = format!(
                        "unable to acquire `caller` account {}, does not exist",
                        tx.from().to_full_string()
                    );
                    let err = Box::new(ValidatorError::Custom(error_string.clone()));
                    let message = PendingTransactionMessage::Invalid {
                        transaction: tx.clone(),
                        e: err,
                    };
                    let _ = pending_transactions.cast(message);
                    return Err(Box::new(ValidatorError::Custom(error_string))
                        as Box<dyn std::error::Error + Send>);
                }
            };

            log::warn!("validating caller nonce");
            if let Err(e) = caller.clone().validate_nonce(tx.nonce()) {
                let error_string = e.to_string();
                let message = PendingTransactionMessage::Invalid {
                    transaction: tx.clone(),
                    e,
                };
                let _ = pending_transactions.cast(message);
                return Err(Box::new(ValidatorError::Custom(error_string))
                    as Box<dyn std::error::Error + Send>);
            }

            let instructions = outputs.instructions();
            log::warn!("call returned {} instruction", instructions.len());
            for instruction in instructions {
                match instruction {
                    Instruction::Transfer(transfer) => {
                        // Get the address we are transferring from
                        let transfer_from = transfer.from();
                        // Get the program id of the program that was executed to
                        // return this transfer instruction
                        let program_id = tx.to();

                        // get the program address of the token being transfered
                        let token_address = transfer.token();

                        log::warn!(
                            "validating caller information: {:?}",
                            caller.programs().get(token_address)
                        );
                        // Check if the transferrer is the caller
                        if transfer_from.clone()
                            == AddressOrNamespace::Address(caller.clone().owner_address())
                        {
                            if let Some(amt) = transfer.amount() {
                                match caller.validate_balance(token_address, *amt) {
                                    Err(e) => {
                                        let error_string = e.to_string();
                                        let message = PendingTransactionMessage::Invalid {
                                            transaction: tx.clone(),
                                            e,
                                        };
                                        let _ = pending_transactions.cast(message);
                                        return Err(Box::new(ValidatorError::Custom(error_string))
                                            as Box<dyn std::error::Error + Send>);
                                    }
                                    _ => {
                                        log::info!("`caller` balance is valid");
                                    }
                                }
                            } else {
                                match caller.validate_token_ownership(&program_id, transfer.ids()) {
                                    Err(e) => {
                                        let error_string = e.to_string();
                                        let message = PendingTransactionMessage::Invalid {
                                            transaction: tx.clone(),
                                            e,
                                        };
                                        let _ = pending_transactions.cast(message);
                                        return Err(Box::new(ValidatorError::Custom(error_string))
                                            as Box<dyn std::error::Error + Send>);
                                    }
                                    _ => {
                                        log::info!("`caller` token ownership is valid");
                                    }
                                }
                            }
                        } else {
                            // If not attempt to get the account for the transferrer
                            let transfer_from_account = match account_map.get(transfer_from) {
                                Some(Some(account)) => account,
                                _ => {
                                    let error_string =
                                        "unable to acquire transferFrom account, does not exist"
                                            .to_string();
                                    let err =
                                        Box::new(ValidatorError::Custom(error_string.clone()));
                                    let message = PendingTransactionMessage::Invalid {
                                        transaction: tx.clone(),
                                        e: err,
                                    };
                                    let _ = pending_transactions.cast(message);
                                    return Err(Box::new(ValidatorError::Custom(error_string))
                                        as Box<dyn std::error::Error + Send>);
                                }
                            };

                            // check that the account being debited indeed has the token
                            // we are debiting
                            let token = match transfer_from_account.programs().get(token_address) {
                                Some(token) => token,
                                None => {
                                    let error_string = format!(
                                        "unable to acquire token {} from account, does not exist",
                                        token_address
                                    );
                                    let err =
                                        Box::new(ValidatorError::Custom(error_string.clone()));
                                    let message = PendingTransactionMessage::Invalid {
                                        transaction: tx.clone(),
                                        e: err,
                                    };
                                    let _ = pending_transactions.cast(message);
                                    return Err(Box::new(ValidatorError::Custom(error_string))
                                        as Box<dyn std::error::Error + Send>);
                                }
                            };

                            // If fungible token, check balance
                            if let Some(amt) = transfer.amount() {
                                let tf_address = if let AccountType::Program(program_address) =
                                    transfer_from_account.account_type()
                                {
                                    program_address.to_full_string()
                                } else {
                                    transfer_from_account.owner_address().to_full_string()
                                };

                                if let Err(e) =
                                    transfer_from_account.validate_balance(token_address, *amt)
                                {
                                    let error_string = format!("account {} has insufficient balance for token {}: Error: {}", tf_address, token_address.to_full_string(), e);
                                    let err =
                                        Box::new(ValidatorError::Custom(error_string.clone()));
                                    let message = PendingTransactionMessage::Invalid {
                                        transaction: tx.clone(),
                                        e: err,
                                    };
                                    let _ = pending_transactions.cast(message);
                                    return Err(Box::new(ValidatorError::Custom(error_string))
                                        as Box<dyn std::error::Error + Send>);
                                }

                                // Check that the caller or the program being called
                                // is approved to spend this token
                                if let AccountType::Program(program_addr) =
                                    transfer_from_account.account_type()
                                {
                                    if program_addr != tx.to() {
                                        if transfer_from_account
                                            .validate_approved_spend(
                                                token_address,
                                                &caller.owner_address().clone(),
                                                amt,
                                            )
                                            .is_err()
                                        {
                                            match transfer_from_account.validate_approved_spend(
                                                token_address,
                                                &program_id,
                                                amt,
                                            ) {
                                                Err(e) => {
                                                    let error_string = e.to_string();
                                                    let message =
                                                        PendingTransactionMessage::Invalid {
                                                            transaction: tx.clone(),
                                                            e,
                                                        };
                                                    let _ = pending_transactions.cast(message);
                                                    return Err(Box::new(ValidatorError::Custom(
                                                        error_string,
                                                    ))
                                                        as Box<dyn std::error::Error + Send>);
                                                }
                                                _ => {
                                                    log::info!("is approved spender");
                                                }
                                            };
                                        } else {
                                            log::info!("is approved spender");
                                        }
                                    }
                                } else if transfer_from_account
                                    .validate_approved_spend(
                                        token_address,
                                        &caller.owner_address().clone(),
                                        amt,
                                    )
                                    .is_err()
                                {
                                    match transfer_from_account.validate_approved_spend(
                                        token_address,
                                        &program_id,
                                        amt,
                                    ) {
                                        Err(e) => {
                                            let error_string = e.to_string();
                                            let message = PendingTransactionMessage::Invalid {
                                                transaction: tx.clone(),
                                                e,
                                            };
                                            let _ = pending_transactions.cast(message);
                                            return Err(Box::new(ValidatorError::Custom(
                                                error_string,
                                            ))
                                                as Box<dyn std::error::Error + Send>);
                                        }
                                        _ => {
                                            log::info!("is approved spender");
                                        }
                                    };
                                } else {
                                    log::info!("is approved spender");
                                }
                            } else {
                                // If non-fungible token check ids
                                match transfer_from_account
                                    .validate_token_ownership(token_address, transfer.ids())
                                {
                                    Err(e) => {
                                        let error_string = e.to_string();
                                        let message = PendingTransactionMessage::Invalid {
                                            transaction: tx.clone(),
                                            e,
                                        };
                                        let _ = pending_transactions.cast(message);
                                        return Err(Box::new(ValidatorError::Custom(error_string))
                                            as Box<dyn std::error::Error + Send>);
                                    }
                                    _ => {
                                        log::info!("is token owner")
                                    }
                                }

                                // Check that the caller or the program being called
                                // is approved to transfer these tokens
                                if transfer_from_account
                                    .validate_approved_token_transfer(
                                        token_address,
                                        &caller.owner_address().clone(),
                                        transfer.ids(),
                                    )
                                    .is_err()
                                {
                                    match transfer_from_account.validate_approved_token_transfer(
                                        token_address,
                                        &program_id,
                                        transfer.ids(),
                                    ) {
                                        Err(e) => {
                                            let error_string = e.to_string();
                                            let message = PendingTransactionMessage::Invalid {
                                                transaction: tx.clone(),
                                                e,
                                            };
                                            let _ = pending_transactions.cast(message);
                                            return Err(Box::new(ValidatorError::Custom(
                                                error_string,
                                            ))
                                                as Box<dyn std::error::Error + Send>);
                                        }
                                        _ => {
                                            log::info!("is approved");
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Instruction::Burn(burn) => {
                        // Get the address we are burning from
                        let burn_from = burn.from();
                        // Get the program id of the program that was executed to
                        // return this transfer instruction
                        let program_id = match burn.program_id() {
                            AddressOrNamespace::Address(addr) => *addr,
                            AddressOrNamespace::This => tx.to(),
                            _ => {
                                let err = {
                                    Box::new(ValidatorError::Custom(
                                        "program namespaces not yet implemented".to_string(),
                                    ))
                                        as Box<dyn std::error::Error + Send>
                                };
                                let error_string = err.to_string();
                                let message = PendingTransactionMessage::Invalid {
                                    transaction: tx.clone(),
                                    e: err,
                                };
                                let _ = pending_transactions.cast(message);
                                return Err(Box::new(ValidatorError::Custom(
                                    "program namespaces not yet implemented".to_string(),
                                ))
                                    as Box<dyn std::error::Error + Send>);
                            }
                        };

                        // get the program address of the token being burned
                        let token_address = burn.token();

                        // Check if the transferrer is the caller
                        if burn_from.clone()
                            == AddressOrNamespace::Address(caller.clone().owner_address())
                        {
                            if let Some(amt) = burn.amount() {
                                match caller.validate_balance(token_address, *amt) {
                                    Err(e) => {
                                        let error_string = e.to_string();
                                        let message = PendingTransactionMessage::Invalid {
                                            transaction: tx.clone(),
                                            e,
                                        };
                                        let _ = pending_transactions.cast(message);
                                        return Err(Box::new(ValidatorError::Custom(error_string))
                                            as Box<dyn std::error::Error + Send>);
                                    }
                                    _ => {
                                        log::info!("balance is valid");
                                    }
                                }
                            } else {
                                match caller.validate_token_ownership(&program_id, burn.token_ids())
                                {
                                    Err(e) => {
                                        let error_string = e.to_string();
                                        let message = PendingTransactionMessage::Invalid {
                                            transaction: tx.clone(),
                                            e,
                                        };
                                        let _ = pending_transactions.cast(message);
                                        return Err(Box::new(ValidatorError::Custom(error_string))
                                            as Box<dyn std::error::Error + Send>);
                                    }
                                    _ => {
                                        log::info!("token ownership is valid");
                                    }
                                }
                            }
                        } else {
                            // If not attempt to get the account for the transferrer
                            log::info!("Attempting to burn from non caller address");
                            let burn_from_account = match account_map.get(burn_from) {
                                Some(Some(account)) => account,
                                _ => {
                                    let error_string =
                                        "account being debited does not exist".to_string();
                                    let e = Box::new(ValidatorError::Custom(error_string.clone()))
                                        as Box<dyn std::error::Error + Send>;
                                    let message = PendingTransactionMessage::Invalid {
                                        transaction: tx.clone(),
                                        e,
                                    };
                                    let _ = pending_transactions.cast(message);
                                    return Err(Box::new(ValidatorError::Custom(error_string))
                                        as Box<dyn std::error::Error + Send>);
                                }
                            };

                            // check that the account being debited indeed has the token
                            // we are debiting
                            let token = match burn_from_account.programs().get(token_address) {
                                Some(token) => token,
                                None => {
                                    let e = {
                                        Box::new(ValidatorError::Custom(
                                            "account being debited does not hold token".to_string(),
                                        ))
                                            as Box<dyn std::error::Error + Send>
                                    };
                                    let error_string = e.to_string();
                                    let message = PendingTransactionMessage::Invalid {
                                        transaction: tx.clone(),
                                        e,
                                    };
                                    let _ = pending_transactions.cast(message);
                                    return Err(Box::new(ValidatorError::Custom(error_string))
                                        as Box<dyn std::error::Error + Send>);
                                }
                            };

                            // If fungible token, check balance
                            if let Some(amt) = burn.amount() {
                                match burn_from_account.validate_balance(token_address, *amt) {
                                    Err(e) => {
                                        let error_string = e.to_string();
                                        let message = PendingTransactionMessage::Invalid {
                                            transaction: tx.clone(),
                                            e,
                                        };
                                        let _ = pending_transactions.cast(message);
                                        return Err(Box::new(ValidatorError::Custom(error_string))
                                            as Box<dyn std::error::Error + Send>);
                                    }
                                    _ => {
                                        log::info!("balance is valid");
                                    }
                                }
                                // Check that the caller or the program being called
                                // is approved to spend this token
                                if burn_from_account
                                    .validate_approved_spend(
                                        token_address,
                                        &caller.owner_address().clone(),
                                        amt,
                                    )
                                    .is_err()
                                {
                                    match burn_from_account.validate_approved_spend(
                                        token_address,
                                        &program_id,
                                        amt,
                                    ) {
                                        Err(e) => {
                                            let error_string = e.to_string();
                                            let message = PendingTransactionMessage::Invalid {
                                                transaction: tx.clone(),
                                                e,
                                            };
                                            let _ = pending_transactions.cast(message);
                                            return Err(Box::new(ValidatorError::Custom(
                                                error_string,
                                            ))
                                                as Box<dyn std::error::Error + Send>);
                                        }
                                        _ => {
                                            log::info!("approved spender");
                                        }
                                    }
                                } else {
                                    log::info!("approved spender");
                                }
                            } else {
                                // If non-fungible token check ids
                                match burn_from_account
                                    .validate_token_ownership(token_address, burn.token_ids())
                                {
                                    Err(e) => {
                                        let error_string = e.to_string();
                                        let message = PendingTransactionMessage::Invalid {
                                            transaction: tx.clone(),
                                            e,
                                        };
                                        let _ = pending_transactions.cast(message);
                                        return Err(Box::new(ValidatorError::Custom(error_string))
                                            as Box<dyn std::error::Error + Send>);
                                    }
                                    _ => {
                                        log::info!("valid token ownership");
                                    }
                                }

                                // Check that the caller or the program being called
                                // is approved to transfer these tokens
                                if burn_from_account
                                    .validate_approved_token_transfer(
                                        token_address,
                                        &caller.owner_address().clone(),
                                        burn.token_ids(),
                                    )
                                    .is_err()
                                {
                                    match burn_from_account.validate_approved_token_transfer(
                                        token_address,
                                        &program_id,
                                        burn.token_ids(),
                                    ) {
                                        Err(e) => {
                                            let error_string = e.to_string();
                                            let message = PendingTransactionMessage::Invalid {
                                                transaction: tx.clone(),
                                                e,
                                            };
                                            let _ = pending_transactions.cast(message);
                                            return Err(Box::new(ValidatorError::Custom(
                                                error_string,
                                            ))
                                                as Box<dyn std::error::Error + Send>);
                                        }
                                        _ => {
                                            log::info!("approved spender");
                                        }
                                    }
                                } else {
                                    log::info!("approved spender");
                                }
                            }
                        }
                    }
                    Instruction::Update(updates) => {
                        log::info!("call {} returned update instruction", tx.hash_string());
                        for update in updates.updates() {
                            match update {
                                TokenOrProgramUpdate::TokenUpdate(token_update) => {
                                    for update_field in token_update.updates() {
                                        match update_field.value() {
                                            TokenFieldValue::Balance(_) => {
                                                let err = {
                                                    Box::new(
                                                        ValidatorError::Custom(
                                                            "Update Instruction cannot be used to update balance, use Transfer or Burn Instruction instead".to_string()
                                                        )
                                                    ) as Box<dyn std::error::Error + Send>
                                                };
                                                let error_string = err.to_string();
                                                let message = PendingTransactionMessage::Invalid {
                                                    transaction: tx.clone(),
                                                    e: err,
                                                };
                                                let _ = pending_transactions.cast(message);
                                                return Err(
                                                    Box::new(
                                                        ValidatorError::Custom(
                                                            "Update Instruction cannot be used to update balance, use Transfer or Burn Instruction instead".to_string()
                                                        )
                                                    ) as Box<dyn std::error::Error + Send>
                                                );
                                            }
                                            TokenFieldValue::Approvals(approval_value) => {
                                                if let Some(Some(acct)) =
                                                    account_map.get(token_update.account())
                                                {
                                                    if acct.owner_address()
                                                        != caller.owner_address()
                                                    {
                                                        let err = {
                                                            Box::new(
                                                                ValidatorError::Custom(
                                                                    "Approvals can only be updated by the account owner".to_string()
                                                                )
                                                            ) as Box<dyn std::error::Error + Send>
                                                        };
                                                        let error_string = err.to_string();
                                                        let message =
                                                            PendingTransactionMessage::Invalid {
                                                                transaction: tx.clone(),
                                                                e: err,
                                                            };
                                                        let _ = pending_transactions.cast(message);
                                                        return Err(
                                                            Box::new(
                                                                ValidatorError::Custom(
                                                                    "Approvals can only be updated by the account owner".to_string()
                                                                )
                                                            ) as Box<dyn std::error::Error + Send>
                                                        );
                                                    }
                                                } else {
                                                    let err = {
                                                        Box::new(
                                                            ValidatorError::Custom(
                                                                "Approvals can only be updated on accounts that exist".to_string()
                                                            )
                                                        ) as Box<dyn std::error::Error + Send>
                                                    };
                                                    let error_string = err.to_string();
                                                    let message =
                                                        PendingTransactionMessage::Invalid {
                                                            transaction: tx.clone(),
                                                            e: err,
                                                        };
                                                    let _ = pending_transactions.cast(message);
                                                    return Err(
                                                        Box::new(
                                                            ValidatorError::Custom(
                                                                "Approvals can only be updated on accounts that exist".to_string()
                                                            )
                                                        ) as Box<dyn std::error::Error + Send>
                                                    );
                                                }
                                            }
                                            TokenFieldValue::Allowance(allowance_value) => {
                                                if let Some(Some(acct)) =
                                                    account_map.get(token_update.account())
                                                {
                                                    if acct.owner_address()
                                                        != caller.owner_address()
                                                    {
                                                        let err = {
                                                            Box::new(
                                                                ValidatorError::Custom(
                                                                    "Allowances can only be updated by the account owner".to_string()
                                                                )
                                                            )
                                                        };
                                                        let error_string = err.to_string();
                                                        let message =
                                                            PendingTransactionMessage::Invalid {
                                                                transaction: tx.clone(),
                                                                e: err,
                                                            };
                                                        let _ = pending_transactions.cast(message);
                                                        return Err(
                                                            Box::new(
                                                                ValidatorError::Custom(
                                                                    "Allowances can only be updated by the account owner".to_string()
                                                                )
                                                            )
                                                        );
                                                    }
                                                } else {
                                                    let err = {
                                                        Box::new(
                                                            ValidatorError::Custom(
                                                                "Allowances can only be updated on accounts that exist".to_string()
                                                            )
                                                        )
                                                    };
                                                    let error_string = err.to_string();
                                                    let message =
                                                        PendingTransactionMessage::Invalid {
                                                            transaction: tx.clone(),
                                                            e: err,
                                                        };
                                                    let _ = pending_transactions.cast(message);
                                                    return Err(
                                                        Box::new(
                                                            ValidatorError::Custom(
                                                                "Allowances can only be updated on accounts that exist".to_string()
                                                            )
                                                        ) as Box<dyn std::error::Error + Send>
                                                    );
                                                }
                                            }
                                            _ => {
                                                let token_address = {
                                                    match token_update.token() {
                                                        AddressOrNamespace::This => tx.to(),
                                                        AddressOrNamespace::Address(addr) => *addr,
                                                        AddressOrNamespace::Namespace(
                                                            namespace,
                                                        ) => {
                                                            let err = {
                                                                Box::new(
                                                                    ValidatorError::Custom(
                                                                        "Namespaces not yet implemented for token updates".to_string()
                                                                    )
                                                                )
                                                            };
                                                            let error_string = err.to_string();
                                                            let message = PendingTransactionMessage::Invalid { transaction: tx.clone(), e: err };
                                                            let _ =
                                                                pending_transactions.cast(message);
                                                            return Err(
                                                                Box::new(
                                                                    ValidatorError::Custom(
                                                                        "Namespaces not yet implemented for token updates".to_string()
                                                                    )
                                                                )
                                                            );
                                                        }
                                                    }
                                                };
                                                if let Some(Some(account)) =
                                                    account_map.get(token_update.account())
                                                {
                                                    if account.owner_address()
                                                        != caller.owner_address()
                                                    {
                                                        if let Some(program) =
                                                            account.programs().get(&token_address)
                                                        {
                                                            let approvals = program.approvals();
                                                            let program_approved =
                                                                approvals.get(&tx.to());
                                                            let caller_approved =
                                                                approvals.get(&tx.from());
                                                            if let (None, None) =
                                                                (program_approved, caller_approved)
                                                            {
                                                                let err = {
                                                                    Box::new(
                                                                        ValidatorError::Custom(
                                                                            "the caller does not own this account, and the account owner has not approved either the caller of the called program".to_string()
                                                                        )
                                                                    ) as Box<dyn std::error::Error + Send>
                                                                };
                                                                let message = PendingTransactionMessage::Invalid { transaction: tx.clone(), e: err };
                                                                let _ = pending_transactions
                                                                    .cast(message);
                                                                return Err(
                                                                    Box::new(
                                                                        ValidatorError::Custom(
                                                                            "the caller does not own this account, and the account owner has not approved either the caller of the called program".to_string()
                                                                        )
                                                                    ) as Box<dyn std::error::Error + Send>
                                                                );
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                TokenOrProgramUpdate::ProgramUpdate(program_update) => {
                                    for update_field in program_update.updates() {
                                        if &AddressOrNamespace::Address(tx.to())
                                            != program_update.account()
                                        {
                                            match account_map.get(program_update.account()) {
                                                Some(Some(acct)) => {
                                                    if acct.owner_address() != tx.from()
                                                        && !acct
                                                            .program_account_linked_programs()
                                                            .contains(&AddressOrNamespace::Address(
                                                                tx.to(),
                                                            ))
                                                    {
                                                        let err = {
                                                            Box::new(
                                                                 ValidatorError::Custom(
                                                                     "program called must be called by program owner, be the program itself, or a linked program to update another program account".to_string()
                                                                 )
                                                             ) as Box<dyn std::error::Error + Send>
                                                        };
                                                        let message =
                                                            PendingTransactionMessage::Invalid {
                                                                transaction: tx.clone(),
                                                                e: err,
                                                            };
                                                        let _ = pending_transactions.cast(message);
                                                        return Err(
                                                             Box::new(
                                                                 ValidatorError::Custom(
                                                                     "program called must be called by program owner, be the program itself, or a linked program to update another program account".to_string()
                                                                 )
                                                             ) as Box<dyn std::error::Error + Send>
                                                         );
                                                    }
                                                }
                                                _ => {
                                                    let err = {
                                                        Box::new(
                                                            ValidatorError::Custom(
                                                                "program accounts must exist to be updated".to_string()
                                                            )
                                                        )
                                                    };
                                                    let message =
                                                        PendingTransactionMessage::Invalid {
                                                            transaction: tx.clone(),
                                                            e: err,
                                                        };
                                                    let _ = pending_transactions.cast(message);
                                                    return Err(Box::new(ValidatorError::Custom(
                                                        "program accounts must exist to be updated"
                                                            .to_string(),
                                                    )));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Instruction::Create(create) => {
                        if &caller.owner_address() != create.program_owner() {
                            let err = Box::new(ValidatorError::Custom(
                                "caller must be program owner for Create Instruction".to_string(),
                            ))
                                as Box<dyn std::error::Error + Send>;
                            let error_string = err.to_string();
                            let message = PendingTransactionMessage::Invalid {
                                transaction: tx.clone(),
                                e: err,
                            };
                            let _ = pending_transactions.cast(message);
                            return Err(Box::new(ValidatorError::Custom(
                                "caller must be program owner for Create Instruction".to_string(),
                            ))
                                as Box<dyn std::error::Error + Send>);
                        }
                        //TODO(asmith): validate against program schema
                    }
                    Instruction::Log(log) => {}
                }
            }

            log::warn!("Completed the validation of all instruction");
            let batcher: ActorRef<BatcherMessage> =
                ractor::registry::where_is(ActorType::Batcher.to_string())
                    .ok_or(Box::new(ValidatorError::Custom(
                        "unable to acquire batcher actor".to_string(),
                    )) as Box<dyn std::error::Error + Send>)?
                    .into();
            log::info!("transaction {} is valid, responding", tx.hash_string());
            let message = BatcherMessage::AppendTransaction {
                transaction: tx,
                outputs: Some(outputs),
            };
            batcher
                .cast(message)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            Ok(())
        }
        // Validate transaction structure and caller signature, including nonce
        // balance as it relates to value, and then validate instructions
        // instruction validation includes checking the balance of and Transfer or
        // Burn instructions to the accounts that will have their balance reduced
        // For creates and updates it includes validating the caller is the owner
        // or has approval, or the program has approval etc.
        // After transaction is validated, send the transaction and instructions
        // to the batcher to apply to the accounts in question
    }

    #[allow(unused)]
    async fn validate_bridge_out(
        &self,
    ) -> impl FnOnce(Transaction) -> Result<bool, Box<dyn std::error::Error>> {
        |_tx| Ok(false)
    }

    fn validate_register_program(
        &self,
    ) -> impl FnOnce(Transaction) -> Result<bool, Box<dyn std::error::Error>> {
        |_tx| Ok(true)
        // Validate signature
        // Validate schema
        // validate account nonce in transaction
    }
}

#[derive(Clone, Debug, Default)]
pub struct Validator;

#[derive(Clone, Debug, Error)]
pub enum ValidatorError {
    Custom(String),
}

impl Display for ValidatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Default for ValidatorError {
    fn default() -> Self {
        ValidatorError::Custom("Validator unable to acquire actor".to_string())
    }
}

impl Validator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Actor for Validator {
    type Msg = ValidatorMessage;
    type State = ValidatorCore;
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: (),
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ValidatorCore::default())
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ValidatorMessage::PendingTransaction { transaction } => {
                log::info!(
                    "Received transaction to validate: {}",
                    transaction.hash_string()
                );
                // spin up thread
                let transaction_type = transaction.transaction_type();
                match transaction_type {
                    TransactionType::Send(_) => {
                        log::info!("Received send transaction, checking account_cache for account {} from validator", &transaction.from().to_full_string());
                        let account = if let Some(account) =
                            check_account_cache(transaction.from()).await
                        {
                            log::info!("found account in cache");
                            Some(account)
                        } else if let Some(account) = check_da_for_account(transaction.from()).await
                        {
                            log::info!("found account in da");
                            Some(account)
                        } else {
                            log::info!("unable to find account in cache or da");
                            None
                        };

                        if account.is_none() {
                            let actor: ActorRef<PendingTransactionMessage> =
                                ractor::registry::where_is(
                                    ActorType::PendingTransactions.to_string(),
                                )
                                .ok_or(Box::new(ValidatorError::Custom(
                                    "unable to acquire pending transaction actor".to_string(),
                                )))?
                                .into();

                            let message = PendingTransactionMessage::Invalid {
                                transaction,
                                e: Box::new(std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    "account does not exist",
                                ))
                                    as Box<dyn std::error::Error + Send>,
                            };

                            actor.cast(message)?;
                        } else {
                            log::info!("validating send transaction");
                            let op = state.validate_send();
                            state.pool.spawn_fifo(move || {
                                let _ = op(transaction.clone(), account.unwrap());
                            });
                        }
                    }
                    TransactionType::Call(_) => {
                        // get account
                        // build op
                        // install op
                    }
                    TransactionType::BridgeIn(_) => {
                        let _account = if let Some(account) =
                            check_account_cache(transaction.from()).await
                        {
                            Some(account)
                        } else if let Some(account) = check_da_for_account(transaction.from()).await
                        {
                            Some(account)
                        } else {
                            log::info!("unable to find account in cache or da");
                            None
                        };
                        let op = state.validate_bridge_in();
                        state.pool.spawn_fifo(move || {
                            let _ = op(transaction);
                        });
                        // get account
                        // check bridged balance in EO
                        // for address
                        // naively validate
                    }
                    TransactionType::RegisterProgram(_) => {
                        // get program
                        // build program
                        // validate sender sig
                        // validate sender balance for deployment fees
                        // commit contract blob
                        //
                    }
                    TransactionType::BridgeOut(_) => {
                        // get program
                        // check for corresponding program
                        // validate sender sig
                        // validate sender balance
                        // execute bridge fn in program
                        // settle bridge transaction on settlement networ
                    }
                }
            }
            ValidatorMessage::PendingCall {
                outputs,
                transaction,
            } => {
                log::warn!(
                    "pending call received by validator for: {}",
                    &transaction.hash_string()
                );
                // Acquire all relevant accounts.
                if let Some(outputs) = outputs {
                    let mut accounts_involved: Vec<AddressOrNamespace> = outputs
                        .instructions()
                        .iter()
                        .flat_map(|inst| inst.get_accounts_involved())
                        .collect();

                    accounts_involved.push(AddressOrNamespace::Address(transaction.from()));
                    accounts_involved.push(AddressOrNamespace::Address(transaction.to()));

                    let mut validator_accounts: HashMap<AddressOrNamespace, Option<Account>> =
                        HashMap::new();
                    for address in &accounts_involved {
                        //TODO(asmith): Replace this block with a parallel iterator to optimize
                        match &address {
                            AddressOrNamespace::This => {
                                let addr = transaction.to();
                                log::info!(
                                    "Received call transaction checking account {} from validator",
                                    &addr.to_full_string()
                                );
                                if let Some(account) = check_account_cache(addr).await {
                                    log::info!("Found `this` account in cache");
                                    validator_accounts.insert(address.clone(), Some(account));
                                } else if let Some(account) = check_da_for_account(addr).await {
                                    log::info!("found `this` account in da");
                                    validator_accounts.insert(address.clone(), Some(account));
                                } else {
                                    log::info!("unable to find account in cache or da");
                                    validator_accounts.insert(address.clone(), None);
                                }
                            }
                            AddressOrNamespace::Address(addr) => {
                                log::info!(
                                    "looking for account {:?} in cache from validator",
                                    addr.to_full_string()
                                );
                                if let Some(account) = check_account_cache(*addr).await {
                                    log::info!("found account in cache");
                                    validator_accounts.insert(address.clone(), Some(account));
                                } else if let Some(account) = check_da_for_account(*addr).await {
                                    log::info!("found account in da");
                                    validator_accounts.insert(address.clone(), Some(account));
                                } else {
                                    log::info!("unable to find account in cache or da");
                                    validator_accounts.insert(address.clone(), None);
                                };
                            }
                            AddressOrNamespace::Namespace(_namespace) => {
                                //TODO(asmith): implement check_account_cache and check_da_for_account for
                                //Namespaces
                                validator_accounts.insert(address.clone(), None);
                            }
                        }
                    }

                    let op = state.validate_call();
                    state.pool.spawn_fifo(move || {
                        let _ = op(validator_accounts, outputs, transaction);
                    });
                } else {
                    log::error!("Call transactions must have an output associated with them");
                    if let Some(actor) =
                        ractor::registry::where_is(ActorType::PendingTransactions.to_string())
                    {
                        let pending_transactions: ActorRef<PendingTransactionMessage> =
                            actor.into();
                        let e = Box::new(ValidatorError::Custom(
                            "Call transaction missing associated outputs".to_string(),
                        ));
                        let message = PendingTransactionMessage::Invalid {
                            transaction: transaction.clone(),
                            e,
                        };
                        let _ = pending_transactions.cast(message);
                    }
                }
            }
            ValidatorMessage::PendingRegistration { transaction } => {
                let op = state.validate_register_program();
                state.pool.spawn_fifo(move || {
                    let _ = op(transaction);
                });
            }
        }
        return Ok(());
    }
}
