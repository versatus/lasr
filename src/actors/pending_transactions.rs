use std::collections::HashMap;
use crate::{Address, Token};
use ractor::concurrency::{OneshotSender, OneshotReceiver};
use tokio::sync::mpsc::Receiver;
use futures::stream::{FuturesUnordered, StreamExt};
use std::fmt::Display;
use thiserror::Error;

#[derive(Debug)]
pub struct PendingTokens {
    map: HashMap<Address, Vec<OneshotSender<(Address, Address)>>>
}

impl PendingTokens {
    pub fn new(token: Token, sender: OneshotSender<(Address, Address)>) -> Self {
        let mut map = HashMap::new();
        map.insert(token.program_id(), vec![sender]);
        Self {
            map
        }
    }

    pub(crate) fn insert(
        &mut self,
        token: Token,
        sender: OneshotSender<(Address, Address)>
    ) -> Result<(), Box<dyn std::error::Error>> {
        let address = token.program_id();
        if let Some(entry) = self.map.get_mut(&address) {
            entry.push(sender);
            return Ok(())
        } 
        self.map.insert(address, vec![sender]);
        Ok(())
    }

    pub(crate) fn remove(
        &mut self,
        token: &Token
    ) -> Option<Vec<OneshotSender<(Address, Address)>>> {
        let program_id = token.program_id();
        self.map.remove(&program_id)
    }

    pub(crate) fn get(
        &mut self,
        token: &Token
    ) -> Option<&mut Vec<OneshotSender<(Address, Address)>>> {
        let program_id = token.program_id();
        self.map.get_mut(&program_id)
    }
}

#[derive(Debug)]
pub struct PendingTransactions {
    pending: HashMap<Address, PendingTokens>,
    receivers: FuturesUnordered<OneshotReceiver<(Address, Token)>>,
    writer: Receiver<(Address, Token, OneshotSender<(Address, Address)>)>
}

#[derive(Debug, Clone)]
pub struct PendingTransactionActor;

#[derive(Debug, Clone, Error)]
pub struct  PendingTransactionError;

impl Display for PendingTransactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", &self)
    }
}

impl PendingTransactions {
    fn handle_new_pending(
        &mut self,
        address: Address,
        token: Token,
        tx: OneshotSender<(Address, Address)>
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(entry) = self.pending.get_mut(&address) {
            let _ = entry.insert(token, tx)?;
            return Ok(())
        }
        let _pending_token = PendingTokens::new(token, tx); 
        Ok(())
    }

    fn handle_confirmed(
        &mut self, 
        address: Address,
        token: Token
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut remove: bool = false;
        if let Some(pending) = self.pending.get_mut(&address) {
            if let Some(senders) = pending.get(&token) {
                let sender = senders.remove(0);
                let _ = sender.send((address, token.program_id()));
                if senders.len() == 0 {
                    remove = true;
                }
            }

            if remove {
                pending.remove(&token);
            }
        }

        Ok(())
    }

    pub async fn run(
        mut self,
        mut stop: OneshotReceiver<u8>
    ) -> Result<(), Box<dyn std::error::Error>> {
        log::info!("Starting Pending Transaction Cache");
        while let Err(_) = stop.try_recv() {
            tokio::select! {
                res = self.receivers.next() => {
                    match res {
                        Some(Ok((address, token))) => {
                            let _ = self.handle_confirmed(address, token);
                        },
                        _ => {}
                    }
                }

                write = self.writer.recv() => {
                    match write {
                        Some((address, token, tx)) => {
                            let _ = self.handle_new_pending(address, token, tx);
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }
}
