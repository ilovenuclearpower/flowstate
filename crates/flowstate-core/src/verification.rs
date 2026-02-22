use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationStep {
    pub id: String,
    pub profile_id: String,
    pub name: String,
    pub command: String,
    pub working_dir: Option<String>,
    pub sort_order: i32,
    pub timeout_s: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileTemplate {
    pub name: String,
    pub description: String,
    pub steps: Vec<StepTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTemplate {
    pub name: String,
    pub command: String,
    pub working_dir: Option<String>,
    pub timeout_s: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Passed,
    Failed,
    Error,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Passed => "passed",
            RunStatus::Failed => "failed",
            RunStatus::Error => "error",
            RunStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "running" => Some(RunStatus::Running),
            "passed" => Some(RunStatus::Passed),
            "failed" => Some(RunStatus::Failed),
            "error" => Some(RunStatus::Error),
            "cancelled" => Some(RunStatus::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationRun {
    pub id: String,
    pub task_id: Option<String>,
    pub profile_id: Option<String>,
    pub triggered_by: String,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationRunStep {
    pub id: String,
    pub run_id: String,
    pub step_name: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub sort_order: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_status_parse_str_round_trip() {
        let all = [
            RunStatus::Running,
            RunStatus::Passed,
            RunStatus::Failed,
            RunStatus::Error,
            RunStatus::Cancelled,
        ];
        for s in all {
            assert_eq!(
                RunStatus::parse_str(s.as_str()),
                Some(s),
                "RunStatus::{:?} should round-trip",
                s
            );
        }
    }

    #[test]
    fn test_run_status_parse_str_invalid() {
        assert_eq!(RunStatus::parse_str("timeout"), None);
        assert_eq!(RunStatus::parse_str(""), None);
    }
}
