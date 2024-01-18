use sha3::{Digest, Sha3_256};

// May want to change the name of this trait to `PayableContract`
pub trait PayableContract {
    fn generate_program_address(&self, program_id: [u8; 32], seed: &[u8]) -> Result<[u8; 32], String> {
        for bump in 0u8..=255 {
            let mut hasher = Sha3_256::new();
            hasher.update(program_id);
            hasher.update(seed);
            hasher.update(&[bump]);
            let result = hasher.finalize();

            let mut data = [0u8; 32];
            data.copy_from_slice(&result[0..32]); 
            if data.is_off_curve() {
                return Ok(data)
            }
        }

        return Err(String::from("Unable to find an off-curve public key, try again with different seed")); 
    }
}

trait OffCurve: AsRef<[u8]> {
    fn is_off_curve(&self) -> bool; 
}

impl OffCurve for [u8; 32] {
    fn is_off_curve(&self) -> bool {
        secp256k1::PublicKey::from_slice(self).is_err()
    }
}
