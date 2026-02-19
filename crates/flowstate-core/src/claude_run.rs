use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeAction {
    Research,
    Design,
    Plan,
    Build,
    Verify,
    ResearchDistill,
    DesignDistill,
    PlanDistill,
    VerifyDistill,
}

impl ClaudeAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaudeAction::Research => "research",
            ClaudeAction::Design => "design",
            ClaudeAction::Plan => "plan",
            ClaudeAction::Build => "build",
            ClaudeAction::Verify => "verify",
            ClaudeAction::ResearchDistill => "research_distill",
            ClaudeAction::DesignDistill => "design_distill",
            ClaudeAction::PlanDistill => "plan_distill",
            ClaudeAction::VerifyDistill => "verify_distill",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "research" => Some(ClaudeAction::Research),
            "design" => Some(ClaudeAction::Design),
            "plan" => Some(ClaudeAction::Plan),
            "build" => Some(ClaudeAction::Build),
            "verify" => Some(ClaudeAction::Verify),
            "research_distill" => Some(ClaudeAction::ResearchDistill),
            "design_distill" => Some(ClaudeAction::DesignDistill),
            "plan_distill" => Some(ClaudeAction::PlanDistill),
            "verify_distill" => Some(ClaudeAction::VerifyDistill),
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
    #[serde(default)]
    pub progress_message: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateClaudeRun {
    pub task_id: String,
    pub action: ClaudeAction,
}
