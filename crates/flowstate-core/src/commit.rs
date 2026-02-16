use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitLink {
    pub id: String,
    pub task_id: String,
    pub sha: String,
    pub message: String,
    pub author: String,
    pub committed_at: Option<DateTime<Utc>>,
    pub linked_at: DateTime<Utc>,
}
