use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkType {
    Blocks,
    RelatesTo,
    Duplicates,
}

impl LinkType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LinkType::Blocks => "blocks",
            LinkType::RelatesTo => "relates_to",
            LinkType::Duplicates => "duplicates",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "blocks" => Some(LinkType::Blocks),
            "relates_to" => Some(LinkType::RelatesTo),
            "duplicates" => Some(LinkType::Duplicates),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            LinkType::Blocks => "Blocks",
            LinkType::RelatesTo => "Relates To",
            LinkType::Duplicates => "Duplicates",
        }
    }
}

impl fmt::Display for LinkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLink {
    pub id: String,
    pub source_task_id: String,
    pub target_task_id: String,
    pub link_type: LinkType,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskLink {
    pub source_task_id: String,
    pub target_task_id: String,
    pub link_type: LinkType,
}
