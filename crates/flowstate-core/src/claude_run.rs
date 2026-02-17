use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeAction {
    Design,
    Plan,
    Build,
}

impl ClaudeAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaudeAction::Design => "design",
            ClaudeAction::Plan => "plan",
            ClaudeAction::Build => "build",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "design" => Some(ClaudeAction::Design),
            "plan" => Some(ClaudeAction::Plan),
            "build" => Some(ClaudeAction::Build),
            _ => None,
        }
    }
}

impl fmt::Display for ClaudeAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeRunStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ClaudeRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaudeRunStatus::Queued => "queued",
            ClaudeRunStatus::Running => "running",
            ClaudeRunStatus::Completed => "completed",
            ClaudeRunStatus::Failed => "failed",
            ClaudeRunStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(ClaudeRunStatus::Queued),
            "running" => Some(ClaudeRunStatus::Running),
            "completed" => Some(ClaudeRunStatus::Completed),
            "failed" => Some(ClaudeRunStatus::Failed),
            "cancelled" => Some(ClaudeRunStatus::Cancelled),
            _ => None,
        }
    }
}

impl fmt::Display for ClaudeRunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRun {
    pub id: String,
    pub task_id: String,
    pub action: ClaudeAction,
    pub status: ClaudeRunStatus,
    pub error_message: Option<String>,
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub pr_number: Option<i64>,
    #[serde(default)]
    pub branch_name: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateClaudeRun {
    pub task_id: String,
    pub action: ClaudeAction,
}
