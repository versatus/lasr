use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize,  Deserialize)]
pub struct EigenDaBlobPayload {
    data: String,
    quorum_id: u32,
    adversary_threshold: u32,
    quorum_threshold: u32,
}

impl EigenDaBlobPayload {
    pub fn new(
        data: String,
        quorum_id: &u32,
        adversary_threshold: &u32, 
        quorum_threshold: &u32,
    ) -> Self {
        EigenDaBlobPayload { 
            data,
            quorum_id: *quorum_id,
            adversary_threshold: *adversary_threshold,
            quorum_threshold: *quorum_threshold,
        }
    }
}

impl From<EigenDaBlobPayload> for String {
    fn from(value: EigenDaBlobPayload) -> Self {
        let mut json_payload = String::new();
        json_payload.push_str(r#"{"data":"#);
        json_payload.push_str(&format!(r#""{}""#, &value.data));
        json_payload.push_str(r#", "security_params":[{"quorum_id":"#);
        json_payload.push_str(&value.quorum_id.to_string());
        json_payload.push_str(r#","adversary_threshold":"#);
        json_payload.push_str(&value.adversary_threshold.to_string());
        json_payload.push_str(r#","quorum_threshold":"#);
        json_payload.push_str(&value.quorum_threshold.to_string());
        json_payload.push_str(r#"}]}"#);

        json_payload
        
    }
}

impl From<&EigenDaBlobPayload> for String {
    fn from(value: &EigenDaBlobPayload) -> Self {
        let mut json_payload = String::new();

        json_payload.push_str(r#"{"data":"#);
        json_payload.push_str(&format!(r#""{}""#, &value.data));
        json_payload.push_str(r#", "security_params":[{"quorum_id":"#);
        json_payload.push_str(&value.quorum_id.to_string());
        json_payload.push_str(r#"","adversary_threshold":"#);
        json_payload.push_str(&value.adversary_threshold.to_string());
        json_payload.push_str(r#","quorum_threshold":"#);
        json_payload.push_str(&value.quorum_threshold.to_string());
        json_payload.push_str(r#"}]}"#);

        json_payload
        
    }
}
