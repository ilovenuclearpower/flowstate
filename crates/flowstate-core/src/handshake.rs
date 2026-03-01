use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerHandshake {
    pub id: String,
    pub runner_id: String,
    pub hostname: String,
    pub status: String,
    pub api_key_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
