use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPr {
    pub id: String,
    pub task_id: String,
    pub claude_run_id: Option<String>,
    pub pr_url: String,
    pub pr_number: i64,
    pub branch_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskPr {
    pub task_id: String,
    pub claude_run_id: Option<String>,
    pub pr_url: String,
    pub pr_number: i64,
    pub branch_name: String,
}
