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

    pub const SUBTASK_BOARD_COLUMNS: &[Status] = &[
        Status::Todo,
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

    /// Returns a numeric ordering index for workflow phases.
    /// Used to enforce forward-only auto-advance.
    pub fn ordinal(&self) -> u8 {
        match self {
            Status::Todo => 0,
            Status::Research => 1,
            Status::Design => 2,
            Status::Plan => 3,
            Status::Build => 4,
            Status::Verify => 5,
            Status::Done => 6,
            Status::Cancelled => 7,
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

impl Task {
    /// Returns true if this task is a subtask (has a parent).
    pub fn is_subtask(&self) -> bool {
        self.parent_id.is_some()
    }
}

/// Next status for a subtask (skips Research/Design/Plan).
pub fn next_subtask_status(s: Status) -> Option<Status> {
    match s {
        Status::Todo => Some(Status::Build),
        Status::Build => Some(Status::Verify),
        Status::Verify => Some(Status::Done),
        _ => None,
    }
}

/// Previous status for a subtask (skips Research/Design/Plan).
pub fn prev_subtask_status(s: Status) -> Option<Status> {
    match s {
        Status::Build => Some(Status::Todo),
        Status::Verify => Some(Status::Build),
        Status::Done => Some(Status::Verify),
        _ => None,
    }
}

/// Returns the board status a task should advance to when the given
/// approval field is approved.
pub fn status_after_approval(field: &str) -> Option<Status> {
    match field {
        "research" => Some(Status::Design),
        "spec" => Some(Status::Plan),
        "plan" => Some(Status::Build),
        "verify" => Some(Status::Done),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_after_approval() {
        assert_eq!(status_after_approval("research"), Some(Status::Design));
        assert_eq!(status_after_approval("spec"), Some(Status::Plan));
        assert_eq!(status_after_approval("plan"), Some(Status::Build));
        assert_eq!(status_after_approval("verify"), Some(Status::Done));
        assert_eq!(status_after_approval("unknown"), None);
    }

    #[test]
    fn test_status_ordinal_ordering() {
        assert!(Status::Todo.ordinal() < Status::Research.ordinal());
        assert!(Status::Research.ordinal() < Status::Design.ordinal());
        assert!(Status::Design.ordinal() < Status::Plan.ordinal());
        assert!(Status::Plan.ordinal() < Status::Build.ordinal());
        assert!(Status::Build.ordinal() < Status::Verify.ordinal());
        assert!(Status::Verify.ordinal() < Status::Done.ordinal());
    }

    #[test]
    fn test_forward_only_logic() {
        let current = Status::Build;
        let target = status_after_approval("research").unwrap();
        assert!(target.ordinal() <= current.ordinal());
    }

    #[test]
    fn parse_str_roundtrip_status() {
        assert_eq!(Status::parse_str("todo"), Some(Status::Todo));
        assert_eq!(Status::parse_str("research"), Some(Status::Research));
        assert_eq!(Status::parse_str("design"), Some(Status::Design));
        assert_eq!(Status::parse_str("plan"), Some(Status::Plan));
        assert_eq!(Status::parse_str("build"), Some(Status::Build));
        assert_eq!(Status::parse_str("verify"), Some(Status::Verify));
        assert_eq!(Status::parse_str("done"), Some(Status::Done));
        assert_eq!(Status::parse_str("cancelled"), Some(Status::Cancelled));
        assert_eq!(Status::parse_str("invalid"), None);
        assert_eq!(Status::parse_str("garbage"), None);
        assert_eq!(Status::parse_str(""), None);
        assert_eq!(Status::parse_str("TODO"), None);
        // Legacy aliases
        assert_eq!(Status::parse_str("backlog"), Some(Status::Todo));
        assert_eq!(Status::parse_str("in_progress"), Some(Status::Build));
        assert_eq!(Status::parse_str("in_review"), Some(Status::Verify));
    }

    #[test]
    fn parse_str_roundtrip_priority() {
        assert_eq!(Priority::parse_str("urgent"), Some(Priority::Urgent));
        assert_eq!(Priority::parse_str("high"), Some(Priority::High));
        assert_eq!(Priority::parse_str("medium"), Some(Priority::Medium));
        assert_eq!(Priority::parse_str("low"), Some(Priority::Low));
        assert_eq!(Priority::parse_str("none"), Some(Priority::None));
        assert_eq!(Priority::parse_str("invalid"), None);
        assert_eq!(Priority::parse_str("critical"), None);
        assert_eq!(Priority::parse_str(""), None);
    }

    #[test]
    fn parse_str_roundtrip_approval_status() {
        assert_eq!(ApprovalStatus::parse_str("none"), Some(ApprovalStatus::None));
        assert_eq!(ApprovalStatus::parse_str("pending"), Some(ApprovalStatus::Pending));
        assert_eq!(ApprovalStatus::parse_str("approved"), Some(ApprovalStatus::Approved));
        assert_eq!(ApprovalStatus::parse_str("rejected"), Some(ApprovalStatus::Rejected));
        assert_eq!(ApprovalStatus::parse_str("invalid"), None);
        assert_eq!(ApprovalStatus::parse_str("maybe"), None);
        assert_eq!(ApprovalStatus::parse_str(""), None);
    }

    #[test]
    fn as_str_consistency() {
        for s in Status::ALL {
            assert_eq!(Status::parse_str(s.as_str()), Some(*s));
        }
        let all_priorities = [
            Priority::Urgent,
            Priority::High,
            Priority::Medium,
            Priority::Low,
            Priority::None,
        ];
        for p in &all_priorities {
            assert_eq!(Priority::parse_str(p.as_str()), Some(*p));
        }
        let all_approvals = [
            ApprovalStatus::None,
            ApprovalStatus::Pending,
            ApprovalStatus::Approved,
            ApprovalStatus::Rejected,
        ];
        for a in &all_approvals {
            assert_eq!(ApprovalStatus::parse_str(a.as_str()), Some(*a));
        }
    }

    #[test]
    fn display_and_display_name_status() {
        for s in Status::ALL {
            assert_eq!(format!("{s}"), s.display_name());
        }
    }

    #[test]
    fn test_status_display_name() {
        assert_eq!(Status::Todo.display_name(), "Todo");
        assert_eq!(Status::Research.display_name(), "Research");
        assert_eq!(Status::Design.display_name(), "Design");
        assert_eq!(Status::Plan.display_name(), "Plan");
        assert_eq!(Status::Build.display_name(), "Build");
        assert_eq!(Status::Verify.display_name(), "Verify");
        assert_eq!(Status::Done.display_name(), "Done");
        assert_eq!(Status::Cancelled.display_name(), "Cancelled");
    }

    #[test]
    fn test_status_board_columns() {
        assert_eq!(Status::BOARD_COLUMNS.len(), 7);
        assert!(!Status::BOARD_COLUMNS.contains(&Status::Cancelled));
        for w in Status::BOARD_COLUMNS.windows(2) {
            assert!(w[0].ordinal() < w[1].ordinal());
        }
    }

    #[test]
    fn test_status_subtask_board_columns() {
        assert_eq!(
            Status::SUBTASK_BOARD_COLUMNS,
            &[Status::Todo, Status::Build, Status::Verify, Status::Done]
        );
    }

    #[test]
    fn display_and_display_name_priority() {
        let all = [
            Priority::Urgent,
            Priority::High,
            Priority::Medium,
            Priority::Low,
            Priority::None,
        ];
        for p in &all {
            assert_eq!(format!("{p}"), p.display_name());
        }
    }

    #[test]
    fn display_and_display_name_approval_status() {
        let all = [
            ApprovalStatus::None,
            ApprovalStatus::Pending,
            ApprovalStatus::Approved,
            ApprovalStatus::Rejected,
        ];
        for a in &all {
            assert_eq!(format!("{a}"), a.display_name());
        }
    }

    #[test]
    fn priority_symbol() {
        assert_eq!(Priority::Urgent.symbol(), "!!!");
        assert_eq!(Priority::High.symbol(), "!!");
        assert_eq!(Priority::Medium.symbol(), "!");
        assert_eq!(Priority::Low.symbol(), "-");
        assert_eq!(Priority::None.symbol(), " ");
    }

    #[test]
    fn approval_status_default() {
        assert_eq!(ApprovalStatus::default(), ApprovalStatus::None);
    }

    #[test]
    fn next_subtask_status_transitions() {
        assert_eq!(next_subtask_status(Status::Todo), Some(Status::Build));
        assert_eq!(next_subtask_status(Status::Build), Some(Status::Verify));
        assert_eq!(next_subtask_status(Status::Verify), Some(Status::Done));
        assert_eq!(next_subtask_status(Status::Done), None);
        assert_eq!(next_subtask_status(Status::Research), None);
        assert_eq!(next_subtask_status(Status::Design), None);
        assert_eq!(next_subtask_status(Status::Plan), None);
        assert_eq!(next_subtask_status(Status::Cancelled), None);
    }

    #[test]
    fn prev_subtask_status_transitions() {
        assert_eq!(prev_subtask_status(Status::Build), Some(Status::Todo));
        assert_eq!(prev_subtask_status(Status::Verify), Some(Status::Build));
        assert_eq!(prev_subtask_status(Status::Done), Some(Status::Verify));
        assert_eq!(prev_subtask_status(Status::Todo), None);
        assert_eq!(prev_subtask_status(Status::Research), None);
        assert_eq!(prev_subtask_status(Status::Design), None);
        assert_eq!(prev_subtask_status(Status::Plan), None);
        assert_eq!(prev_subtask_status(Status::Cancelled), None);
    }

    #[test]
    fn task_is_subtask() {
        let make_task = |parent_id: Option<String>| Task {
            id: "t1".into(),
            project_id: "p1".into(),
            sprint_id: None,
            parent_id,
            title: "Test".into(),
            description: String::new(),
            reviewer: String::new(),
            research_status: ApprovalStatus::None,
            spec_status: ApprovalStatus::None,
            plan_status: ApprovalStatus::None,
            verify_status: ApprovalStatus::None,
            spec_approved_hash: String::new(),
            research_approved_hash: String::new(),
            research_feedback: String::new(),
            spec_feedback: String::new(),
            plan_feedback: String::new(),
            verify_feedback: String::new(),
            status: Status::Todo,
            priority: Priority::None,
            sort_order: 0.0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert!(!make_task(None).is_subtask());
        assert!(make_task(Some("parent".into())).is_subtask());
    }

    #[test]
    fn test_update_task_default() {
        let u = UpdateTask::default();
        assert!(u.title.is_none());
        assert!(u.description.is_none());
        assert!(u.status.is_none());
        assert!(u.priority.is_none());
        assert!(u.sprint_id.is_none());
        assert!(u.sort_order.is_none());
        assert!(u.parent_id.is_none());
        assert!(u.reviewer.is_none());
    }
}
