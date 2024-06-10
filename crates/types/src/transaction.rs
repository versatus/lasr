use crate::{Address, ArbitraryData, Metadata, Status, Token, TokenBuilder};
use crate::{RecoverableSignature, RecoverableSignatureBuilder};
use derive_builder::Builder;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::BTreeMap;
use std::fmt::{Display, LowerHex};
use thiserror::Error;

#[derive(Clone, Debug, Error)]
pub enum ToTokenError {
    Custom(String),
}

impl Display for ToTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub enum TransactionType {
    BridgeIn(crate::U256),
    Send(crate::U256),
    Call(crate::U256),
    BridgeOut(crate::U256),
    RegisterProgram(crate::U256),
}

impl TransactionType {
    pub fn is_send(&self) -> bool {
        matches!(self, TransactionType::Send(_))
    }

    pub fn is_bridge_in(&self) -> bool {
        matches!(self, TransactionType::BridgeIn(_))
    }

    pub fn is_call(&self) -> bool {
        matches!(self, TransactionType::Call(_))
    }

    pub fn is_bridge_out(&self) -> bool {
        matches!(self, TransactionType::BridgeOut(_))
    }

    pub fn is_register_program(&self) -> bool {
        matches!(self, TransactionType::RegisterProgram(_))
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::BridgeIn(n) => serde_json::json!({"bridgeIn": format!("0x{:064x}", n)}),
            Self::Send(n) => serde_json::json!({"send": format!("0x{:064x}", n)}),
            Self::Call(n) => serde_json::json!({"call": format!("0x{:064x}", n)}),
            Self::RegisterProgram(n) => {
                serde_json::json!({"registerProgram": format!("0x{:064x}", n)})
            }
            Self::BridgeOut(n) => serde_json::json!({"bridgeOut": format!("0x{:064x}", n)}),
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

#[derive(
    Builder, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    transaction_type: TransactionType,
    #[serde(deserialize_with = "deserialize_address_bytes_or_string")]
    from: [u8; 20],
    #[serde(deserialize_with = "deserialize_address_bytes_or_string")]
    to: [u8; 20],
    #[serde(deserialize_with = "deserialize_address_bytes_or_string")]
    program_id: [u8; 20],
    op: String,
    #[serde(rename(serialize = "transactionInputs", deserialize = "transactionInputs"))]
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
        let mut hasher = Keccak256::new();
        hasher.update(&self.as_bytes());
        let res = hasher.finalize();
        format!("0x{:x}", res)
    }

    pub fn hash(&self) -> Vec<u8> {
        let mut hasher = Keccak256::new();
        hasher.update(&self.as_bytes());
        let res = hasher.finalize();
        log::info!("transaction hash: 0x{:x}", res);
        res.to_vec()
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let transaction_json = serde_json::json!({
            "transactionType": self.transaction_type().to_json(),
            "from": Address::from(self.from()).to_full_string(),
            "to": Address::from(self.to()).to_full_string(),
            "programId": Address::from(self.program_id()).to_full_string(),
            "op": self.op.clone(),
            "transactionInputs": self.inputs().clone(),
            "value": format!("0x{:064x}", self.value()),
            "nonce": format!("0x{:064x}", self.nonce())
        })
        .to_string();

        log::info!("converted payload to json: {}", &transaction_json);
        transaction_json.as_bytes().to_vec()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HexOr20Bytes {
    Hex(String),
    Bytes([u8; 20]),
}

// Custom serializer for byte arrays to hex strings
fn serialize_as_hex<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let hex_string = hex::encode(bytes);
    serializer.serialize_str(&format!("0x{}", hex_string))
}

pub fn deserialize_address_bytes_or_string<'de, D>(deserializer: D) -> Result<[u8; 20], D::Error>
where
    D: Deserializer<'de>,
{
    let input = HexOr20Bytes::deserialize(deserializer)?;
    match input {
        HexOr20Bytes::Hex(value) => {
            if let Some(value) = value.strip_prefix("0x") {
                let bytes =
                    hex::decode(value).map_err(|e| serde::de::Error::custom(e.to_string()))?;
                bytes.try_into().map_err(|_| {
                    serde::de::Error::custom("Hex string does not represent valid 20-byte array")
                })
            } else if value.starts_with('[') && value.ends_with(']') {
                let bytes_str = &value[1..value.len() - 1];
                let bytes: Vec<u8> = bytes_str
                    .split(',')
                    .map(str::trim)
                    .map(|s| s.parse::<u8>().map_err(serde::de::Error::custom))
                    .collect::<Result<Vec<u8>, D::Error>>()?;
                bytes.try_into().map_err(|_| {
                    serde::de::Error::custom(
                        "Bytes string does not represent a value 20-byte array",
                    )
                })
            } else {
                let bytes =
                    hex::decode(&value).map_err(|e| serde::de::Error::custom(e.to_string()))?;
                bytes.try_into().map_err(|_| {
                    serde::de::Error::custom("Hex string does not represent valid 20 byte array")
                })
            }
        }
        HexOr20Bytes::Bytes(v) => Ok(v),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HexOr32Bytes {
    Hex(String),
    Bytes([u8; 32]),
}

pub fn deserialize_sig_bytes_or_string<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
where
    D: Deserializer<'de>,
{
    let input = HexOr32Bytes::deserialize(deserializer)?;
    match input {
        HexOr32Bytes::Hex(value) => {
            if let Some(value) = value.strip_prefix("0x") {
                let bytes =
                    hex::decode(value).map_err(|e| serde::de::Error::custom(e.to_string()))?;
                bytes.try_into().map_err(|_| {
                    serde::de::Error::custom("Hex string does not represent valid 20-byte array")
                })
            } else if value.starts_with('[') && value.ends_with(']') {
                let bytes_str = &value[1..value.len() - 1];
                let bytes: Vec<u8> = bytes_str
                    .split(',')
                    .map(str::trim)
                    .map(|s| s.parse::<u8>().map_err(serde::de::Error::custom))
                    .collect::<Result<Vec<u8>, D::Error>>()?;
                bytes.try_into().map_err(|_| {
                    serde::de::Error::custom(
                        "Bytes string does not represent a value 20-byte array",
                    )
                })
            } else {
                Err(serde::de::Error::custom(
                    "unable to deserialize as a string",
                ))
            }
        }
        HexOr32Bytes::Bytes(v) => Ok(v),
    }
}

#[derive(
    Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    transaction_type: TransactionType,
    #[serde(
        serialize_with = "serialize_as_hex",
        deserialize_with = "deserialize_address_bytes_or_string"
    )]
    from: [u8; 20],
    #[serde(
        serialize_with = "serialize_as_hex",
        deserialize_with = "deserialize_address_bytes_or_string"
    )]
    to: [u8; 20],
    #[serde(
        serialize_with = "serialize_as_hex",
        deserialize_with = "deserialize_address_bytes_or_string",
        alias = "token",
        alias = "token_address",
        alias = "program_address"
    )]
    program_id: [u8; 20],
    op: String,
    #[serde(rename(serialize = "transactionInputs", deserialize = "transactionInputs"))]
    inputs: String,
    value: crate::U256,
    nonce: crate::U256,
    v: i32,
    #[serde(
        serialize_with = "serialize_as_hex",
        deserialize_with = "deserialize_sig_bytes_or_string"
    )]
    r: [u8; 32],
    #[serde(
        serialize_with = "serialize_as_hex",
        deserialize_with = "deserialize_sig_bytes_or_string"
    )]
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
            s: [0u8; 32],
        }
    }
}

impl ractor::serialization::BytesConvertable for Transaction {
    fn into_bytes(self) -> Vec<u8> {
        self.as_bytes()
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        serde_json::from_slice(&bytes).unwrap()
    }
}

#[derive(
    Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all(deserialize = "lowercase"))]
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
            .build()
            .map_err(Box::new)?;

        Ok(sig)
    }

    pub fn recover(&self) -> Result<Address, Box<dyn std::error::Error>> {
        let r = self.r;
        let s = self.s;
        let v = self.v;

        if (27..=28).contains(&v) {
            let sig = ethers_core::types::Signature {
                r: ethers_core::abi::ethereum_types::U256::from(r),
                s: ethers_core::abi::ethereum_types::U256::from(s),
                v: v as u64,
            };
            log::warn!("attempting to recover from {}", sig.to_string());
            let addr = sig.recover(self.hash())?;
            return Ok(addr.into());
        }
        let addr = self.sig()?.recover(&self.hash())?;
        Ok(addr)
    }

    pub fn message(&self) -> String {
        format!("{:02x}", self)
    }

    pub fn hash_string(&self) -> String {
        let mut hasher = Keccak256::new();
        hasher.update(&self.as_bytes());
        let res = hasher.finalize();
        format!("0x{:x}", res)
    }

    pub fn hash(&self) -> Vec<u8> {
        let mut hasher = Keccak256::new();
        hasher.update(&self.as_bytes());
        let res = hasher.finalize();
        log::info!("transaction hash: 0x{:x}", res);
        res.to_vec()
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let transaction_json = serde_json::json!({
            "transactionType": self.transaction_type().to_json(),
            "from": self.from().to_full_string(),
            "to": self.to().to_full_string(),
            "programId": self.program_id().to_full_string(),
            "op": self.op.clone(),
            "transactionInputs": self.inputs().clone(),
            "value": format!("0x{:064x}", self.value()),
            "nonce": format!("0x{:064x}", self.nonce())
        })
        .to_string();

        log::info!("converted payload to json: {}", &transaction_json);
        transaction_json.as_bytes().to_vec()
    }

    pub fn verify_signature(&self) -> Result<(), secp256k1::Error> {
        let addr = self
            .sig()
            .map_err(|_| secp256k1::Error::InvalidMessage)?
            .recover(&self.hash())?;
        if self.from() != addr {
            log::error!(
                "self.from() {} != addr {}",
                self.from().to_full_string(),
                addr.to_full_string()
            );
            return Err(secp256k1::Error::InvalidSignature);
        }

        Ok(())
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
            .build()
            .unwrap()
    }
}

impl TryFrom<(Token, Transaction)> for Token {
    type Error = Box<dyn std::error::Error + Send>;
    fn try_from(value: (Token, Transaction)) -> Result<Self, Self::Error> {
        if value.1.from() == value.0.owner_id() {
            if value.1.transaction_type().is_bridge_in() {
                return TokenBuilder::default()
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
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>);
            }
            return TokenBuilder::default()
                .program_id(value.0.program_id())
                .owner_id(value.0.owner_id())
                .balance(value.0.balance() - value.1.value())
                .metadata(value.0.metadata())
                .token_ids(value.0.token_ids())
                .allowance(value.0.allowance())
                .approvals(value.0.approvals())
                .data(value.0.data())
                .status(value.0.status())
                .build()
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>);
        }

        if value.1.to() == value.0.owner_id() {
            return TokenBuilder::default()
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
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>);
        }

        Err(Box::new(ToTokenError::Custom(
            "cannot convert into token, token owner_id does not match sender or receiver"
                .to_string(),
        )))
    }
}
