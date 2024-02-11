use crate::{
    Address,
    TransactionType,
    RecoverableSignature,
    U256
};

use secp256k1::{Error as Secp256k1Error, PublicKey};
use std::error::Error;

pub trait ProtocolTransaction {
    fn program_id(&self) -> Address;
    fn from(&self) -> Address; 
    fn to(&self) -> Address; 
    fn transaction_type(&self) -> TransactionType; 
    fn op(&self) -> String; 
    fn inputs(&self) -> String; 
    fn value(&self) -> U256; 
    fn nonce(&self) -> U256;
    fn sig(&self) -> Result<RecoverableSignature, Box<dyn Error>>; 
    fn recover(&self) -> Result<PublicKey, Box<dyn Error>>;
    fn message(&self) -> String;
    fn hash_string(&self) -> String;
    fn hash(&self) -> Vec<u8>;
    fn as_bytes(&self) -> Vec<u8>;
    fn verify_signature(&self) -> Result<(), Secp256k1Error>; 
    fn get_accounts_involved(&self) -> Vec<Address>; 
}

pub trait TransactionPayload {
    fn transaction_type(&self) -> TransactionType; 
    fn from(&self) -> [u8; 20]; 
    fn to(&self) -> [u8; 20]; 
    fn program_id(&self) -> [u8; 20]; 
    fn op(&self) -> String; 
    fn inputs(&self) -> String; 
    fn value(&self) -> crate::U256; 
    fn nonce(&self) -> crate::U256;
    fn hash_string(&self) -> String; 
    fn hash(&self) -> Vec<u8>; 
    fn as_bytes(&self) -> Vec<u8>; 
}
