use serde::{Serialize, Deserialize};
use sha3::{Digest, Sha3_256};
use std::collections::BTreeSet;
use secp256k1::{PublicKey, ecdsa::{RecoverableSignature as Signature, RecoveryId}, Message};

/// Represents a recoverable ECDSA signature.
///
/// This structure stores the components of a recoverable signature, consisting of
/// two 32-byte arrays `r` and `s`, and a recovery id `v`. The signature can be
/// used in cryptographic operations where the public key needs to be recovered
/// from the signature and the original message.
#[derive(Builder, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)] 
pub struct RecoverableSignature {
    r: [u8; 32],
    s: [u8; 32],
    v: i32
}

impl RecoverableSignature {
    /// Recovers the public key from the signature and the original message.
    ///
    /// This method takes a message slice as input, computes its SHA3-256 hash,
    /// and then attempts to recover the public key that was used to sign the message.
    /// It returns the recovered public key if successful.
    pub fn recover(&self, message: &[u8]) -> Result<PublicKey, secp256k1::Error> {
        let secp = secp256k1::Secp256k1::new();
        let mut hasher = Sha3_256::new();
        hasher.update(message);
        let message_hash = hasher.finalize();
        let message = Message::from_digest_slice(&message_hash)?;
        let mut serialized_sig = [0u8; 64];
        serialized_sig[..32].copy_from_slice(&self.r);
        serialized_sig[32..].copy_from_slice(&self.s);
        let recovery_id = RecoveryId::from_i32(self.v)?;

        let recoverable_sig = Signature::from_compact(&serialized_sig, recovery_id)?;

        secp.recover_ecdsa(&message, &recoverable_sig)
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

    pub fn serialize(&self) -> Result<Vec<u8>, Box<bincode::ErrorKind>> {
        bincode::serialize(&self)
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, Box<bincode::ErrorKind>> {
        bincode::deserialize(bytes)
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

#[derive(Builder, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

    pub fn serialize(&self) -> Result<Vec<u8>, Box<bincode::ErrorKind>> {
        bincode::serialize(&self)
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, Box<bincode::ErrorKind>> {
        bincode::deserialize(bytes)
    }
}
