use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    #[default]
    Github,
    Gitea,
}

impl ProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderType::Github => "github",
            ProviderType::Gitea => "gitea",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "github" => Some(ProviderType::Github),
            "gitea" => Some(ProviderType::Gitea),
            _ => None,
        }
    }
}

impl fmt::Display for ProviderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub description: String,
    #[serde(default)]
    pub repo_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<ProviderType>,
    #[serde(default)]
    pub skip_tls_verify: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProject {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub repo_url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub description: Option<String>,
    pub repo_url: Option<String>,
    pub repo_token: Option<String>,
    pub provider_type: Option<ProviderType>,
    pub skip_tls_verify: Option<bool>,
}
