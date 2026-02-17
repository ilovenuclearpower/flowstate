use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Backlog,
    Todo,
    InProgress,
    InReview,
    Done,
    Cancelled,
}

impl Status {
    pub const ALL: &[Status] = &[
        Status::Backlog,
        Status::Todo,
        Status::InProgress,
        Status::InReview,
        Status::Done,
        Status::Cancelled,
    ];

    pub const BOARD_COLUMNS: &[Status] = &[
        Status::Backlog,
        Status::Todo,
        Status::InProgress,
        Status::InReview,
        Status::Done,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Backlog => "backlog",
            Status::Todo => "todo",
            Status::InProgress => "in_progress",
            Status::InReview => "in_review",
            Status::Done => "done",
            Status::Cancelled => "cancelled",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Status::Backlog => "Backlog",
            Status::Todo => "Todo",
            Status::InProgress => "In Progress",
            Status::InReview => "In Review",
            Status::Done => "Done",
            Status::Cancelled => "Cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "backlog" => Some(Status::Backlog),
            "todo" => Some(Status::Todo),
            "in_progress" => Some(Status::InProgress),
            "in_review" => Some(Status::InReview),
            "done" => Some(Status::Done),
            "cancelled" => Some(Status::Cancelled),
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

    pub fn from_str(s: &str) -> Option<Self> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
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

    pub fn from_str(s: &str) -> Option<Self> {
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

impl Default for ApprovalStatus {
    fn default() -> Self {
        ApprovalStatus::None
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
    pub spec_status: ApprovalStatus,
    pub plan_status: ApprovalStatus,
    pub spec_approved_hash: String,
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
    pub spec_status: Option<ApprovalStatus>,
    pub plan_status: Option<ApprovalStatus>,
    pub spec_approved_hash: Option<String>,
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
