pub mod account;
pub mod certificate;
pub mod program;
pub mod token;
pub mod transaction;

pub use crate::account::*;
pub use crate::certificate::*;
pub use crate::program::*;
pub use crate::token::*;
pub use crate::transaction::*;

pub const MAX_BATCH_SIZE: usize = 1024 * 512;
pub const ETH_PROGRAM_ID: [u8; 20] = [0u8; 20];
pub const VERSE_PROGRAM_ID: [u8; 20] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];

#[macro_use]
extern crate derive_builder;

#[cfg(test)]
mod tests {}
