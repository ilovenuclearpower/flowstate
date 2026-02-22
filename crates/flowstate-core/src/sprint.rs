use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SprintStatus {
    Planned,
    Active,
    Completed,
}

impl SprintStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SprintStatus::Planned => "planned",
            SprintStatus::Active => "active",
            SprintStatus::Completed => "completed",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            SprintStatus::Planned => "Planned",
            SprintStatus::Active => "Active",
            SprintStatus::Completed => "Completed",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "planned" => Some(SprintStatus::Planned),
            "active" => Some(SprintStatus::Active),
            "completed" => Some(SprintStatus::Completed),
            _ => None,
        }
    }
}

impl fmt::Display for SprintStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sprint {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub goal: String,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
    pub status: SprintStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSprint {
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub starts_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub ends_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateSprint {
    pub name: Option<String>,
    pub goal: Option<String>,
    pub status: Option<SprintStatus>,
    pub starts_at: Option<Option<DateTime<Utc>>>,
    pub ends_at: Option<Option<DateTime<Utc>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprint_status_parse_str_round_trip() {
        let all = [
            SprintStatus::Planned,
            SprintStatus::Active,
            SprintStatus::Completed,
        ];
        for s in all {
            assert_eq!(
                SprintStatus::parse_str(s.as_str()),
                Some(s),
                "SprintStatus::{:?} should round-trip",
                s
            );
        }
    }

    #[test]
    fn test_sprint_status_parse_str_invalid() {
        assert_eq!(SprintStatus::parse_str("cancelled"), None);
        assert_eq!(SprintStatus::parse_str(""), None);
    }

    #[test]
    fn test_sprint_status_display() {
        assert_eq!(format!("{}", SprintStatus::Planned), "Planned");
        assert_eq!(format!("{}", SprintStatus::Active), "Active");
        assert_eq!(format!("{}", SprintStatus::Completed), "Completed");
    }
}