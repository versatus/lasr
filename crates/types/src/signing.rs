use schemars::JsonSchema;
use serde::{Serialize, Deserialize};
use ethers_core::types::Signature as ElectrumSignature;
use std::collections::BTreeSet;
use secp256k1::{PublicKey, ecdsa::{RecoverableSignature as Signature, RecoveryId}, Message};
use crate::{deserialize_sig_bytes_or_string, Address};
use derive_builder::Builder;


// Custom serializer for byte arrays to hex strings
fn serialize_as_hex<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let hex_string = hex::encode(bytes);
    serializer.serialize_str(&format!("0x{}", hex_string))
}

/// Represents a recoverable ECDSA signature.
///
/// This structure stores the components of a recoverable signature, consisting of
/// two 32-byte arrays `r` and `s`, and a recovery id `v`. The signature can be
/// used in cryptographic operations where the public key needs to be recovered
/// from the signature and the original message.
#[derive(Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct RecoverableSignature {
    #[serde(serialize_with = "serialize_as_hex", deserialize_with = "deserialize_sig_bytes_or_string")]
    r: [u8; 32],
    #[serde(serialize_with = "serialize_as_hex", deserialize_with = "deserialize_sig_bytes_or_string")]
    s: [u8; 32],
    v: i32
}

impl RecoverableSignature {
    /// Recovers the public key from the signature and the original message.
    ///
    /// This method takes a message slice as input, computes its SHA3-256 hash,
    /// and then attempts to recover the public key that was used to sign the message.
    /// It returns the recovered public key if successful.
    pub fn recover(&self, message_bytes: &[u8]) -> Result<Address, secp256k1::Error> {
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&self.get_r());
        sig_bytes.copy_from_slice(&self.get_s());
        let sig_string = hex::encode(&sig_bytes);
        log::warn!("attempting to recover from {}", sig_string);
        log::warn!("MessageBytes: {}", hex::encode(message_bytes));
        if self.v >= 27 && self.v <=28 {
            log::warn!("v is >= 27, <= 28, using electrum signature");
            let esig = ElectrumSignature {
                r: ethers_core::abi::ethereum_types::U256::from(self.get_r()),
                s: ethers_core::abi::ethereum_types::U256::from(self.get_s()),
                v: self.get_v() as u64
            };

            let eaddr = esig.recover(message_bytes).map_err(|_| secp256k1::Error::InvalidSignature)?; 
            log::warn!("{:?}", eaddr);

            Ok(Address::from(eaddr))
        } else {
            let message = Message::from_digest_slice(message_bytes)?;
            let secp = secp256k1::Secp256k1::new();
            let recoverable_sig = Signature::try_from(self)?;
            let pk = secp.recover_ecdsa(&message, &recoverable_sig)?;
            Ok(Address::from(pk))
        }
    }

    pub fn verify(&self, message: &[u8], pk: PublicKey) -> Result<(), secp256k1::Error> {
        log::info!("attemting to recover signature");
        let sig = Signature::try_from(self)?.to_standard();
        log::info!("reconstructing message: {}", hex::encode(message));
        let msg = Message::from_digest_slice(&message)?;
        log::info!("verifying message: {} with pubkey: {}", hex::encode(message), pk.to_string());
        sig.verify(&msg, &pk)
    }

    /// Converts the signature into a vector of bytes.
    ///
    /// This method serializes the signature components (`r`, `s`, and `v`) into
    /// a single byte vector, which can be used for storage or transmission.
    pub fn to_vec(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend(self.r);
        bytes.extend(self.s);
        bytes.extend(self.v.to_le_bytes());
        bytes
    }

    pub fn serialize(&self) -> Result<Vec<u8>, serde_json::Error> {
        Ok(serde_json::to_string(&self)?.as_bytes().to_vec())
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_str(&String::from_utf8_lossy(bytes).to_owned())
    }

    pub fn get_r(&self) -> [u8; 32] {
        self.r
    }

    pub fn get_s(&self) -> [u8; 32] {
        self.s
    }

    pub fn get_v(&self) -> i32 {
        self.v
    }

    pub fn v_into_bytes(&self) -> [u8; 4] {
        self.v.to_le_bytes()
    }
}

impl From<Signature> for RecoverableSignature {
    fn from(value: Signature) -> Self {
        let (v, rs) = value.serialize_compact();
        let mut r = [0u8; 32];
        let mut s = [0u8; 32];

        r.copy_from_slice(&rs[0..32]);
        s.copy_from_slice(&rs[32..]);

        Self { r, s, v: v.to_i32() }
    }
}

impl TryFrom<RecoverableSignature> for Signature {
    type Error = secp256k1::Error;
    fn try_from(value: RecoverableSignature) -> Result<Signature, Self::Error> {
        let mut data = Vec::new();
        let mut recovery_id = value.get_v();
        if recovery_id >= 0 && recovery_id <= 3 {
            log::info!("using r: {:?} and s: {:?} with recovery_id: {:?} to convert to secp256k1 signature", &value.get_r(), value.get_s(), value.get_v());
            data.extend_from_slice(&value.get_r());
            data.extend_from_slice(&value.get_s());
            return Signature::from_compact(&data, RecoveryId::from_i32(recovery_id)?)
        } else if recovery_id >= 27 && recovery_id <= 30 {
            recovery_id -= 27;
            log::info!("using r: {:?} and s: {:?} with recovery_id: {:?} to convert to secp256k1 signature", &value.get_r(), value.get_s(), value.get_v());
            data.extend_from_slice(&value.get_r());
            data.extend_from_slice(&value.get_s());
            return Signature::from_compact(&data, RecoveryId::from_i32(recovery_id)?)
        } else if recovery_id >= 35 && recovery_id <= 38 {
            recovery_id -= 35;
            log::info!("using r: {:?} and s: {:?} with recovery_id: {:?} to convert to secp256k1 signature", &value.get_r(), value.get_s(), value.get_v());
            data.extend_from_slice(&value.get_r());
            data.extend_from_slice(&value.get_s());
            return Signature::from_compact(&data, RecoveryId::from_i32(recovery_id)?)
        } else {
            log::info!("using r: {:?} and s: {:?} with recovery_id: {:?} to convert to secp256k1 signature", &value.get_r(), value.get_s(), value.get_v());
            data.extend_from_slice(&value.get_r());
            data.extend_from_slice(&value.get_s());
            return Signature::from_compact(&data, RecoveryId::from_i32(recovery_id)?)
        }
    }
}

impl TryFrom<&RecoverableSignature> for Signature {
    type Error = secp256k1::Error;
    fn try_from(value: &RecoverableSignature) -> Result<Signature, Self::Error> {
        let mut data = Vec::new();
        let mut recovery_id = value.get_v();
        if recovery_id >= 0 && recovery_id <= 3 {
            log::warn!("using r: {:?} and s: {:?} with recovery_id: {:?} to convert to secp256k1 signature", &value.get_r(), value.get_s(), value.get_v());
            data.extend_from_slice(&value.get_r());
            data.extend_from_slice(&value.get_s());
            return Signature::from_compact(&data, RecoveryId::from_i32(recovery_id)?)
        } else if recovery_id >= 27 && recovery_id <= 30 {
            recovery_id -= 27;
            log::warn!("using r: {:?} and s: {:?} with recovery_id: {:?} to convert to secp256k1 signature", &value.get_r(), value.get_s(), value.get_v());
            data.extend_from_slice(&value.get_r());
            data.extend_from_slice(&value.get_s());
            return Signature::from_compact(&data, RecoveryId::from_i32(recovery_id)?)
        } else if recovery_id >= 35 && recovery_id <= 38 {
            recovery_id -= 35;
            log::warn!("using r: {:?} and s: {:?} with recovery_id: {:?} to convert to secp256k1 signature", &value.get_r(), value.get_s(), value.get_v());
            data.extend_from_slice(&value.get_r());
            data.extend_from_slice(&value.get_s());
            return Signature::from_compact(&data, RecoveryId::from_i32(recovery_id)?)
        } else {
            log::info!("using r: {:?} and s: {:?} with recovery_id: {:?} to convert to secp256k1 signature", &value.get_r(), value.get_s(), value.get_v());
            data.extend_from_slice(&value.get_r());
            data.extend_from_slice(&value.get_s());
            return Signature::from_compact(&data, RecoveryId::from_i32(recovery_id)?)
        }
    }
}


#[derive(Builder, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Certificate {
    quorum_id: [u8; 20],
    quorum_sigs: BTreeSet<RecoverableSignature>
}

impl Certificate {
    // Converts the certificate into a vector of bytes with the first 20
    // being the quorum id, followed by PublicKey (33) and Signature (
    pub fn to_vec(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(self.quorum_id);

        let sigs: &BTreeSet<RecoverableSignature> = &self.quorum_sigs;
        bytes.extend(
            &sigs.iter()
                .map(|sig| sig.to_vec())
                .collect::<Vec<Vec<u8>>>()
                .into_iter()
                .flatten()
                .collect::<Vec<u8>>()
        );

        bytes
    }

    pub fn serialize(&self) -> Result<Vec<u8>, serde_json::Error> {
        Ok(serde_json::to_string(&self)?.as_bytes().to_vec())
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_str(&String::from_utf8_lossy(bytes).to_owned())
    }
}

mod tests {
    use super::*;
}
