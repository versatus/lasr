use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncodedBlob {
    data: String
}

impl EncodedBlob {
    pub fn from_str(value: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(value)
    }

    pub fn data(&self) -> String {
        self.data.clone()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecodedBlob {
    data: Vec<u8>
}

impl DecodedBlob {
    pub fn from_encoded(blob: EncodedBlob) -> Result<Self, base64::DecodeError> {
        let decoded = base64::decode(&blob.data())?;

        Ok(Self {
            data: decoded
        })
    }

    pub fn data(&self) -> Vec<u8> {
        self.data.clone()
    }

    pub fn data_to_string(&self) -> Result<String, std::string::FromUtf8Error> {
        todo!();
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}
