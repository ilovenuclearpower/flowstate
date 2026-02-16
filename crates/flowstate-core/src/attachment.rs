use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub task_id: String,
    pub filename: String,
    pub disk_path: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}
