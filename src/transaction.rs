use schemars::JsonSchema;
use serde::{Serialize, Deserialize, Deserializer};
use sha3::{Sha3_256, Digest};
use crate::{Address, Token, TokenBuilder, Metadata, ArbitraryData, Status};
use crate::{RecoverableSignature, RecoverableSignatureBuilder};
use std::collections::BTreeMap;
use std::fmt::{LowerHex, Display};
use secp256k1::PublicKey;
use thiserror::Error;

#[derive(Clone, Debug, Error)]
pub enum ToTokenError {
    Custom(String)
}

impl Display for ToTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
#[serde(rename_all = "camelCase")]
pub enum TransactionType {
    BridgeIn(crate::U256),
    Send(crate::U256),
    Call(crate::U256),
    BridgeOut(crate::U256),
    RegisterProgram(crate::U256)
}

impl TransactionType {
    pub fn is_send(&self) -> bool {
        match self {
            TransactionType::Send(_) => true,
            _ => false
        }
    }

    pub fn is_bridge_in(&self) -> bool {
        match self {
            TransactionType::BridgeIn(_) => true,
            _ => false
        }
    }

    pub fn is_call(&self) -> bool {
        match self {
            TransactionType::Call(_) => true,
            _ => false
        }
    }
    
    pub fn is_bridge_out(&self) -> bool {
        match self {
            TransactionType::BridgeOut(_) => true,
            _ => false
        }
    }

    pub fn is_register_program(&self) -> bool {
        match self {
            TransactionType::RegisterProgram(_) => true,
            _ => false
        }
    }
}

impl ToString for TransactionType {
    fn to_string(&self) -> String {
        match self {
            TransactionType::BridgeIn(n) => format!("bridgeIn{n}"),
            TransactionType::Send(n) => format!("send{n}"),
            TransactionType::Call(n) => format!("call{n}"),
            TransactionType::BridgeOut(n) => format!("bridgeOut{n}"),
            TransactionType::RegisterProgram(n) => format!("deploy{n}"),
        }
    }
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
#[serde(rename_all = "camelCase")]
pub struct Payload {
    transaction_type: TransactionType,
    from: [u8; 20],
    to: [u8; 20],
    program_id: [u8; 20],
    op: String,
    inputs: String,
    value: crate::U256,
    nonce: crate::U256,
}

impl Payload {
    pub fn transaction_type(&self) -> TransactionType {
        self.transaction_type.clone()
    }

    pub fn from(&self) -> [u8; 20] {
        self.from
    }

    pub fn to(&self) -> [u8; 20] {
        self.to
    }

    pub fn program_id(&self) -> [u8; 20] {
        self.program_id
    }

    pub fn op(&self) -> String {
        self.op.clone()
    }

    pub fn inputs(&self) -> String {
        self.inputs.clone()
    }

    pub fn value(&self) -> crate::U256 {
        self.value
    }

    pub fn nonce(&self) -> crate::U256 {
        self.nonce
    }

    pub fn hash_string(&self) -> String {
        let mut hasher = Sha3_256::new();
        hasher.update(&self.as_bytes());
        let res = hasher.finalize();
        format!("0x{:x}", res)
    }

    pub fn hash(&self) -> Vec<u8> {
        let mut hasher = Sha3_256::new();
        hasher.update(&self.as_bytes());
        let res = hasher.finalize();
        res.to_vec()
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.transaction_type().to_string().as_bytes());
        bytes.extend_from_slice(&self.from().as_ref());
        bytes.extend_from_slice(&self.to().as_ref());
        bytes.extend_from_slice(&self.program_id().as_ref());
        bytes.extend_from_slice(self.inputs().to_string().as_bytes());
        let mut u256 = Vec::new(); 
        let value = self.value();
        value.0.iter().for_each(|n| { 
            let le = n.to_le_bytes();
            u256.extend_from_slice(&le);
        }); 
        bytes.extend_from_slice(&u256);
        bytes
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HexOr20Bytes {
    Hex(String),
    Bytes([u8; 20])
}

fn deserialize_address_bytes_or_string<'de, D>(deserializer: D) -> Result<[u8; 20], D::Error> 
where
    D: Deserializer<'de>
{
    let input = HexOr20Bytes::deserialize(deserializer)?;
    match input {
        HexOr20Bytes::Hex(value) => {
            if value.starts_with("0x") {
                let bytes = hex::decode(&value[2..]).map_err(|e| serde::de::Error::custom(e.to_string()))?;
                bytes.try_into().map_err(|_| serde::de::Error::custom("Hex string does not represent valid 20-byte array")) 
            } else if value.starts_with("[") && value.ends_with("]") {
                let bytes_str = &value[1..value.len() - 1];
                let bytes: Vec<u8> = bytes_str.split(',')
                    .map(str::trim)
                    .map(|s| s.parse::<u8>().map_err(serde::de::Error::custom)).collect::<Result<Vec<u8>, D::Error>>()?;
                bytes.try_into().map_err(|_| serde::de::Error::custom("Bytes string does not represent a value 20-byte array"))
            } else {
                let bytes = hex::decode(&value).map_err(|e| serde::de::Error::custom(e.to_string()))?;
                bytes.try_into().map_err(|_| serde::de::Error::custom("Hex string does not represent valid 20 byte array"))
            }
        }
        HexOr20Bytes::Bytes(v) => {
            Ok(v)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HexOr32Bytes {
    Hex(String),
    Bytes([u8; 32])
}

fn deserialize_sig_bytes_or_string<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error> 
where
    D: Deserializer<'de>
{
    let input = HexOr32Bytes::deserialize(deserializer)?;
    match input {
        HexOr32Bytes::Hex(value) => {
            if value.starts_with("0x") {
                let bytes = hex::decode(&value[2..]).map_err(|e| serde::de::Error::custom(e.to_string()))?;
                bytes.try_into().map_err(|_| serde::de::Error::custom("Hex string does not represent valid 20-byte array")) 
            } else if value.starts_with("[") && value.ends_with("]") {
                let bytes_str = &value[1..value.len() - 1];
                let bytes: Vec<u8> = bytes_str.split(',')
                    .map(str::trim)
                    .map(|s| s.parse::<u8>().map_err(serde::de::Error::custom)).collect::<Result<Vec<u8>, D::Error>>()?;
                bytes.try_into().map_err(|_| serde::de::Error::custom("Bytes string does not represent a value 20-byte array"))
            } else {
                Err(serde::de::Error::custom("unable to deserialize as a string"))
            }
        }
        HexOr32Bytes::Bytes(v) => {
            Ok(v)
        }
    }
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    transaction_type: TransactionType,
    #[serde(deserialize_with = "deserialize_address_bytes_or_string")]
    from: [u8; 20],
    #[serde(deserialize_with = "deserialize_address_bytes_or_string")]
    to: [u8; 20],
    #[serde(deserialize_with = "deserialize_address_bytes_or_string")]
    program_id: [u8; 20],
    op: String,
    inputs: String,
    value: crate::U256,
    nonce: crate::U256,
    v: i32,
    #[serde(deserialize_with = "deserialize_sig_bytes_or_string")]
    r: [u8; 32],
    #[serde(deserialize_with = "deserialize_sig_bytes_or_string")]
    s: [u8; 32],
}

impl Default for Transaction {
    fn default() -> Self {
        Self {
            transaction_type: TransactionType::Send(crate::U256::from(0)),
            from: [0u8; 20],
            to: [0u8; 20],
            program_id: [0u8; 20],
            op: String::from(""),
            inputs: String::from(""),
            value: crate::U256::from(0),
            nonce: crate::U256::from(0),
            v: 0,
            r: [0u8; 32],
            s: [0u8; 32]
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all(deserialize="lowercase"))]
pub enum TransactionFields {
    TransactionType,
    From,
    To,
    ProgramId,
    Op,
    Inputs,
    Value,
    Nonce,
    V,
    R,
    S,
}

impl Transaction {
    pub fn program_id(&self) -> Address {
        self.program_id.into()
    }

    pub fn from(&self) -> Address {
        self.from.into()
    }

    pub fn to(&self) -> Address {
        self.to.into()
    }

    pub fn transaction_type(&self) -> TransactionType {
        self.transaction_type.clone()
    }

    pub fn op(&self) -> String {
        self.op.to_string()
    }

    pub fn inputs(&self) -> String {
        self.inputs.to_string()
    }

    pub fn value(&self) -> crate::U256 {
        self.value
    }

    pub fn nonce(&self) -> crate::U256 {
        self.nonce
    }

    pub fn sig(&self) -> Result<RecoverableSignature, Box<dyn std::error::Error>> { 
        let sig = RecoverableSignatureBuilder::default()
            .r(self.r)
            .s(self.s)
            .v(self.v)
            .build().map_err(|e| Box::new(e))?;

        Ok(sig)
    }

    pub fn recover(&self) -> Result<PublicKey, Box<dyn std::error::Error>> {
        let pk = self.sig()?.recover(&self.as_bytes())?;
        Ok(pk)
    }

    pub fn message(&self) -> String {
        format!("{:02x}", self)
    }

    pub fn hash_string(&self) -> String {
        let mut hasher = Sha3_256::new();
        hasher.update(&self.as_bytes());
        let res = hasher.finalize();
        format!("0x{:x}", res)
    }

    pub fn hash(&self) -> Vec<u8> {
        let mut hasher = Sha3_256::new();
        hasher.update(&self.as_bytes());
        let res = hasher.finalize();
        res.to_vec()
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.transaction_type().to_string().as_bytes());
        bytes.extend_from_slice(&self.from().as_ref());
        bytes.extend_from_slice(&self.to().as_ref());
        bytes.extend_from_slice(&self.program_id().as_ref());
        bytes.extend_from_slice(self.inputs().to_string().as_bytes());
        let mut u256 = Vec::new(); 
        let value = self.value();
        value.0.iter().for_each(|n| { 
            let le = n.to_le_bytes();
            u256.extend_from_slice(&le);
        }); 
        bytes.extend_from_slice(&u256);
        bytes
    }

    pub fn verify_signature(&self) -> Result<(), secp256k1::Error> {
        self.sig().map_err(|_| secp256k1::Error::InvalidMessage)?.verify(&self.as_bytes())
    }

    pub fn get_accounts_involved(&self) -> Vec<Address> {
        vec![self.from(), self.to()]
    }
}

impl LowerHex for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.as_bytes() {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl From<(Payload, RecoverableSignature)> for Transaction {
    fn from(value: (Payload, RecoverableSignature)) -> Self {
        Transaction { 
            transaction_type: value.0.transaction_type(), 
            from: value.0.from(), 
            to: value.0.to(), 
            program_id: value.0.program_id(), 
            op: value.0.op(),
            inputs: value.0.inputs(), 
            value: value.0.value(), 
            nonce: value.0.nonce(),
            v: value.1.get_v(), 
            r: value.1.get_r(), 
            s: value.1.get_s(),
        }
    }
}

impl From<Transaction> for Token {
    fn from(value: Transaction) -> Self {
        TokenBuilder::default()
            .program_id(value.program_id())
            .owner_id(value.to())
            .balance(value.value())
            .metadata(Metadata::new())
            .token_ids(Vec::new())
            .allowance(BTreeMap::new())
            .approvals(BTreeMap::new())
            .data(ArbitraryData::new())
            .status(Status::Free)
            .build().unwrap()
    }
}

impl TryFrom<(Token, Transaction)> for Token {
    type Error = Box<dyn std::error::Error + Send>;
    fn try_from(value: (Token, Transaction)) -> Result<Self, Self::Error> {
        if value.1.from() == value.0.owner_id() {
            if value.1.transaction_type().is_bridge_in() {
                return Ok(TokenBuilder::default()
                    .program_id(value.0.program_id())
                    .owner_id(value.0.owner_id())
                    .balance(value.0.balance() + value.1.value())
                    .metadata(value.0.metadata())
                    .token_ids(value.0.token_ids())
                    .allowance(value.0.allowance())
                    .approvals(value.0.approvals())
                    .data(value.0.data())
                    .status(value.0.status())
                    .build()
                    .map_err(|e| {
                        Box::new(e) as Box<dyn std::error::Error + Send>
                    })?
                )
            }
            return Ok(TokenBuilder::default()
                .program_id(value.0.program_id())
                .owner_id(value.0.owner_id())
                .balance(value.0.balance() - value.1.value())
                .metadata(value.0.metadata())
                .token_ids(value.0.token_ids())
                .allowance(value.0.allowance())
                .approvals(value.0.approvals())
                .data(value.0.data())
                .status(value.0.status())
                .build().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?
            );
        }

        if value.1.to() == value.0.owner_id() {
            return Ok(TokenBuilder::default()
                .program_id(value.0.program_id())
                .owner_id(value.0.owner_id())
                .balance(value.0.balance() + value.1.value())
                .metadata(value.0.metadata())
                .token_ids(value.0.token_ids())
                .allowance(value.0.allowance())
                .approvals(value.0.approvals())
                .data(value.0.data())
                .status(value.0.status())
                .build().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?
            );
        }

        return Err(
            Box::new(
                ToTokenError::Custom(
                    "cannot convert into token, token owner_id does not match sender or receiver".to_string()
                )
            )
        )
    }
}
