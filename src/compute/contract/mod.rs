pub mod contract;

pub use contract::*;

use sha3::{Digest, Keccak256};
use crate::{Address, Transaction};

pub fn create_program_id(content_id: String, transaction: &Transaction) -> Result<Address, Box<dyn std::error::Error + Send>> {
    let pubkey = transaction.sig().map_err(|e| {
        Box::new(
            std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string()
            )    
        ) as Box<dyn std::error::Error + Send>
    })?.recover(&transaction.hash()).map_err(|e| {
        Box::new(
            std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string()
            )
        ) as Box<dyn std::error::Error + Send>
    })?;

    let mut hasher = Keccak256::new();
    hasher.update(content_id.clone());
    hasher.update(pubkey.to_string());
    let hash = hasher.finalize();
    let mut addr_bytes = [0u8; 20];
    addr_bytes.copy_from_slice(&hash[..20]);
    Ok(Address::from(addr_bytes))
}
