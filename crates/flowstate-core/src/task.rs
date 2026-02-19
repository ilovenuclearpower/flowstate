use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Todo,
    Research,
    Design,
    Plan,
    Build,
    Verify,
    Done,
    Cancelled,
}

impl Status {
    pub const ALL: &[Status] = &[
        Status::Todo,
        Status::Research,
        Status::Design,
        Status::Plan,
        Status::Build,
        Status::Verify,
        Status::Done,
        Status::Cancelled,
    ];

    pub const BOARD_COLUMNS: &[Status] = &[
        Status::Todo,
        Status::Research,
        Status::Design,
        Status::Plan,
        Status::Build,
        Status::Verify,
        Status::Done,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Todo => "todo",
            Status::Research => "research",
            Status::Design => "design",
            Status::Plan => "plan",
            Status::Build => "build",
            Status::Verify => "verify",
            Status::Done => "done",
            Status::Cancelled => "cancelled",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Status::Todo => "Todo",
            Status::Research => "Research",
            Status::Design => "Design",
            Status::Plan => "Plan",
            Status::Build => "Build",
            Status::Verify => "Verify",
            Status::Done => "Done",
            Status::Cancelled => "Cancelled",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "todo" => Some(Status::Todo),
            "research" => Some(Status::Research),
            "design" => Some(Status::Design),
            "plan" => Some(Status::Plan),
            "build" => Some(Status::Build),
            "verify" => Some(Status::Verify),
            "done" => Some(Status::Done),
            "cancelled" => Some(Status::Cancelled),
            // Legacy aliases for backward-compatible deserialization
            "backlog" => Some(Status::Todo),
            "in_progress" => Some(Status::Build),
            "in_review" => Some(Status::Verify),
            _ => None,
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Urgent,
    High,
    Medium,
    Low,
    None,
}

impl Priority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::Urgent => "urgent",
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
            Priority::None => "none",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Priority::Urgent => "Urgent",
            Priority::High => "High",
            Priority::Medium => "Medium",
            Priority::Low => "Low",
            Priority::None => "None",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Priority::Urgent => "!!!",
            Priority::High => "!!",
            Priority::Medium => "!",
            Priority::Low => "-",
            Priority::None => " ",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "urgent" => Some(Priority::Urgent),
            "high" => Some(Priority::High),
            "medium" => Some(Priority::Medium),
            "low" => Some(Priority::Low),
            "none" => Some(Priority::None),
            _ => None,
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    #[default]
    None,
    Pending,
    Approved,
    Rejected,
}

impl ApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalStatus::None => "none",
            ApprovalStatus::Pending => "pending",
            ApprovalStatus::Approved => "approved",
            ApprovalStatus::Rejected => "rejected",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "none" => Some(ApprovalStatus::None),
            "pending" => Some(ApprovalStatus::Pending),
            "approved" => Some(ApprovalStatus::Approved),
            "rejected" => Some(ApprovalStatus::Rejected),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ApprovalStatus::None => "None",
            ApprovalStatus::Pending => "Pending",
            ApprovalStatus::Approved => "Approved",
            ApprovalStatus::Rejected => "Rejected",
        }
    }
}


impl fmt::Display for ApprovalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub sprint_id: Option<String>,
    pub parent_id: Option<String>,
    pub title: String,
    pub description: String,
    pub reviewer: String,
    pub research_status: ApprovalStatus,
    pub spec_status: ApprovalStatus,
    pub plan_status: ApprovalStatus,
    pub verify_status: ApprovalStatus,
    pub spec_approved_hash: String,
    pub research_approved_hash: String,
    pub research_feedback: String,
    pub spec_feedback: String,
    pub plan_feedback: String,
    pub verify_feedback: String,
    pub status: Status,
    pub priority: Priority,
    pub sort_order: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTask {
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: Status,
    pub priority: Priority,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub reviewer: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateTask {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<Status>,
    pub priority: Option<Priority>,
    pub sprint_id: Option<Option<String>>,
    pub sort_order: Option<f64>,
    pub parent_id: Option<Option<String>>,
    pub reviewer: Option<String>,
    pub research_status: Option<ApprovalStatus>,
    pub spec_status: Option<ApprovalStatus>,
    pub plan_status: Option<ApprovalStatus>,
    pub verify_status: Option<ApprovalStatus>,
    pub spec_approved_hash: Option<String>,
    pub research_approved_hash: Option<String>,
    pub research_feedback: Option<String>,
    pub spec_feedback: Option<String>,
    pub plan_feedback: Option<String>,
    pub verify_feedback: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub project_id: Option<String>,
    pub status: Option<Status>,
    pub priority: Option<Priority>,
    pub sprint_id: Option<String>,
    pub parent_id: Option<Option<String>>,
    pub limit: Option<i64>,
}
