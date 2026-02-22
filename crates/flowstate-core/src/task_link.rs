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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_type_parse_str() {
        assert_eq!(LinkType::parse_str("blocks"), Some(LinkType::Blocks));
        assert_eq!(LinkType::parse_str("relates_to"), Some(LinkType::RelatesTo));
        assert_eq!(LinkType::parse_str("duplicates"), Some(LinkType::Duplicates));
        assert_eq!(LinkType::parse_str("invalid"), None);
        assert_eq!(LinkType::parse_str("depends_on"), None);
        assert_eq!(LinkType::parse_str(""), None);
    }

    #[test]
    fn link_type_as_str_roundtrip() {
        let all = [LinkType::Blocks, LinkType::RelatesTo, LinkType::Duplicates];
        for l in &all {
            assert_eq!(LinkType::parse_str(l.as_str()), Some(*l));
        }
    }

    #[test]
    fn link_type_display_name() {
        assert_eq!(LinkType::Blocks.display_name(), "Blocks");
        assert_eq!(LinkType::RelatesTo.display_name(), "Relates To");
        assert_eq!(LinkType::Duplicates.display_name(), "Duplicates");
    }

    #[test]
    fn link_type_display() {
        let all = [LinkType::Blocks, LinkType::RelatesTo, LinkType::Duplicates];
        for l in &all {
            assert_eq!(format!("{l}"), l.display_name());
        }
    }
}
