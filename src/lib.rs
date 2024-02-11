pub mod account;
pub mod program;
pub mod rpc;
pub mod certificate;
pub mod actors;
pub mod sequencer;
pub mod clients;
pub mod token;
pub mod transaction;
pub mod compute;
pub mod interfaces;

pub use crate::interfaces::*;
pub use crate::sequencer::*;
pub use crate::account::*;
pub use crate::program::*;
pub use crate::rpc::*;
pub use crate::certificate::*;
pub use crate::actors::*;
pub use crate::clients::*;
pub use crate::token::*;
pub use crate::transaction::*;
pub use crate::compute::*;

pub const MAX_BATCH_SIZE: usize = 1024 * 512;
pub const ETH_PROGRAM_ID: [u8; 20] = [0u8; 20]; 
pub const VERSE_PROGRAM_ID: [u8; 20] = [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1];

#[macro_use]
extern crate derive_builder;

#[cfg(test)]
mod tests {
}
