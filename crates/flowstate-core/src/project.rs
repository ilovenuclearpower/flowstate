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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_type_default_is_github() {
        assert_eq!(ProviderType::default(), ProviderType::Github);
    }

    #[test]
    fn provider_type_as_str() {
        assert_eq!(ProviderType::Github.as_str(), "github");
        assert_eq!(ProviderType::Gitea.as_str(), "gitea");
    }

    #[test]
    fn provider_type_parse_str() {
        assert_eq!(
            ProviderType::parse_str("github"),
            Some(ProviderType::Github)
        );
        assert_eq!(ProviderType::parse_str("gitea"), Some(ProviderType::Gitea));
        assert_eq!(ProviderType::parse_str("unknown"), None);
        assert_eq!(ProviderType::parse_str(""), None);
    }

    #[test]
    fn provider_type_display() {
        assert_eq!(format!("{}", ProviderType::Github), "github");
        assert_eq!(format!("{}", ProviderType::Gitea), "gitea");
    }

    #[test]
    fn provider_type_serde_roundtrip() {
        let github = ProviderType::Github;
        let json = serde_json::to_string(&github).unwrap();
        assert_eq!(json, "\"github\"");
        let parsed: ProviderType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, github);

        let gitea = ProviderType::Gitea;
        let json = serde_json::to_string(&gitea).unwrap();
        assert_eq!(json, "\"gitea\"");
        let parsed: ProviderType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, gitea);
    }

    #[test]
    fn create_project_serde() {
        let cp = CreateProject {
            name: "Test".into(),
            slug: "test".into(),
            description: String::new(),
            repo_url: String::new(),
        };
        let json = serde_json::to_string(&cp).unwrap();
        let parsed: CreateProject = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "Test");
        assert_eq!(parsed.slug, "test");
    }

    #[test]
    fn update_project_default() {
        let up = UpdateProject::default();
        assert!(up.name.is_none());
        assert!(up.description.is_none());
        assert!(up.repo_url.is_none());
        assert!(up.repo_token.is_none());
        assert!(up.provider_type.is_none());
        assert!(up.skip_tls_verify.is_none());
    }

    #[test]
    fn update_project_serde() {
        let up = UpdateProject {
            name: Some("New Name".into()),
            provider_type: Some(ProviderType::Gitea),
            skip_tls_verify: Some(true),
            ..Default::default()
        };
        let json = serde_json::to_string(&up).unwrap();
        let parsed: UpdateProject = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("New Name"));
        assert_eq!(parsed.provider_type, Some(ProviderType::Gitea));
        assert_eq!(parsed.skip_tls_verify, Some(true));
    }

    #[test]
    fn provider_type_copy_clone() {
        let a = ProviderType::Github;
        let b = a;
        let c = a.clone();
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn provider_type_debug() {
        let s = format!("{:?}", ProviderType::Github);
        assert!(s.contains("Github"));
    }
}
