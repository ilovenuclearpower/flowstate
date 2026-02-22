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
    TimedOut,
    Salvaging,
}

impl ClaudeRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaudeRunStatus::Queued => "queued",
            ClaudeRunStatus::Running => "running",
            ClaudeRunStatus::Completed => "completed",
            ClaudeRunStatus::Failed => "failed",
            ClaudeRunStatus::Cancelled => "cancelled",
            ClaudeRunStatus::TimedOut => "timed_out",
            ClaudeRunStatus::Salvaging => "salvaging",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(ClaudeRunStatus::Queued),
            "running" => Some(ClaudeRunStatus::Running),
            "completed" => Some(ClaudeRunStatus::Completed),
            "failed" => Some(ClaudeRunStatus::Failed),
            "cancelled" => Some(ClaudeRunStatus::Cancelled),
            "timed_out" => Some(ClaudeRunStatus::TimedOut),
            "salvaging" => Some(ClaudeRunStatus::Salvaging),
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
    #[serde(default)]
    pub runner_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub required_capability: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateClaudeRun {
    pub task_id: String,
    pub action: ClaudeAction,
    #[serde(default)]
    pub required_capability: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_action_parse_str_all() {
        assert_eq!(ClaudeAction::parse_str("research"), Some(ClaudeAction::Research));
        assert_eq!(ClaudeAction::parse_str("design"), Some(ClaudeAction::Design));
        assert_eq!(ClaudeAction::parse_str("plan"), Some(ClaudeAction::Plan));
        assert_eq!(ClaudeAction::parse_str("build"), Some(ClaudeAction::Build));
        assert_eq!(ClaudeAction::parse_str("verify"), Some(ClaudeAction::Verify));
        assert_eq!(ClaudeAction::parse_str("research_distill"), Some(ClaudeAction::ResearchDistill));
        assert_eq!(ClaudeAction::parse_str("design_distill"), Some(ClaudeAction::DesignDistill));
        assert_eq!(ClaudeAction::parse_str("plan_distill"), Some(ClaudeAction::PlanDistill));
        assert_eq!(ClaudeAction::parse_str("verify_distill"), Some(ClaudeAction::VerifyDistill));
        assert_eq!(ClaudeAction::parse_str("invalid"), None);
        assert_eq!(ClaudeAction::parse_str("compile"), None);
        assert_eq!(ClaudeAction::parse_str(""), None);
    }

    #[test]
    fn claude_action_as_str_roundtrip() {
        let all = [
            ClaudeAction::Research,
            ClaudeAction::Design,
            ClaudeAction::Plan,
            ClaudeAction::Build,
            ClaudeAction::Verify,
            ClaudeAction::ResearchDistill,
            ClaudeAction::DesignDistill,
            ClaudeAction::PlanDistill,
            ClaudeAction::VerifyDistill,
        ];
        for a in &all {
            assert_eq!(ClaudeAction::parse_str(a.as_str()), Some(*a));
        }
    }

    #[test]
    fn claude_action_display() {
        let all = [
            ClaudeAction::Research,
            ClaudeAction::Design,
            ClaudeAction::Plan,
            ClaudeAction::Build,
            ClaudeAction::Verify,
            ClaudeAction::ResearchDistill,
            ClaudeAction::DesignDistill,
            ClaudeAction::PlanDistill,
            ClaudeAction::VerifyDistill,
        ];
        for a in &all {
            assert_eq!(format!("{a}"), a.as_str());
        }
    }

    #[test]
    fn claude_run_status_parse_str_all() {
        assert_eq!(ClaudeRunStatus::parse_str("queued"), Some(ClaudeRunStatus::Queued));
        assert_eq!(ClaudeRunStatus::parse_str("running"), Some(ClaudeRunStatus::Running));
        assert_eq!(ClaudeRunStatus::parse_str("completed"), Some(ClaudeRunStatus::Completed));
        assert_eq!(ClaudeRunStatus::parse_str("failed"), Some(ClaudeRunStatus::Failed));
        assert_eq!(ClaudeRunStatus::parse_str("cancelled"), Some(ClaudeRunStatus::Cancelled));
        assert_eq!(ClaudeRunStatus::parse_str("timed_out"), Some(ClaudeRunStatus::TimedOut));
        assert_eq!(ClaudeRunStatus::parse_str("salvaging"), Some(ClaudeRunStatus::Salvaging));
        assert_eq!(ClaudeRunStatus::parse_str("invalid"), None);
        assert_eq!(ClaudeRunStatus::parse_str("pending"), None);
        assert_eq!(ClaudeRunStatus::parse_str(""), None);
    }

    #[test]
    fn claude_run_status_as_str_roundtrip() {
        let all = [
            ClaudeRunStatus::Queued,
            ClaudeRunStatus::Running,
            ClaudeRunStatus::Completed,
            ClaudeRunStatus::Failed,
            ClaudeRunStatus::Cancelled,
            ClaudeRunStatus::TimedOut,
            ClaudeRunStatus::Salvaging,
        ];
        for s in &all {
            assert_eq!(ClaudeRunStatus::parse_str(s.as_str()), Some(*s));
        }
    }

    #[test]
    fn claude_run_status_display() {
        let all = [
            ClaudeRunStatus::Queued,
            ClaudeRunStatus::Running,
            ClaudeRunStatus::Completed,
            ClaudeRunStatus::Failed,
            ClaudeRunStatus::Cancelled,
            ClaudeRunStatus::TimedOut,
            ClaudeRunStatus::Salvaging,
        ];
        for s in &all {
            assert_eq!(format!("{s}"), s.as_str());
        }
    }
}
