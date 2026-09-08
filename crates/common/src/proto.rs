use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyResponse {
    pub build_id: String,
    pub priv_armored: String,
    pub passphrase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    pub id: String,
    pub created_at: String,
    pub status: String,
}
