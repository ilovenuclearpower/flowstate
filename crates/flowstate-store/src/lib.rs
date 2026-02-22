mod local;
#[cfg(feature = "s3")]
mod s3;

pub use local::LocalStore;
#[cfg(feature = "s3")]
pub use s3::S3Store;

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("store error: {0}")]
    Internal(String),
}

/// A store for opaque blobs keyed by string paths.
#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// Write (create or overwrite) an object.
    async fn put(&self, key: &str, data: Bytes) -> Result<(), StoreError>;

    /// Read an object. Returns `StoreError::NotFound` if absent.
    async fn get(&self, key: &str) -> Result<Bytes, StoreError>;

    /// Read an object, returning `None` if it does not exist.
    async fn get_opt(&self, key: &str) -> Result<Option<Bytes>, StoreError> {
        match self.get(key).await {
            Ok(data) => Ok(Some(data)),
            Err(StoreError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Delete an object. No-op if absent.
    async fn delete(&self, key: &str) -> Result<(), StoreError>;

    /// List object keys under a prefix.
    async fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError>;

    /// Check if an object exists.
    async fn exists(&self, key: &str) -> Result<bool, StoreError> {
        match self.get(key).await {
            Ok(_) => Ok(true),
            Err(StoreError::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

// -- Key helpers --

pub fn task_spec_key(task_id: &str) -> String {
    format!("tasks/{task_id}/specification.md")
}

pub fn task_plan_key(task_id: &str) -> String {
    format!("tasks/{task_id}/plan.md")
}

pub fn task_research_key(task_id: &str) -> String {
    format!("tasks/{task_id}/research.md")
}

pub fn task_verification_key(task_id: &str) -> String {
    format!("tasks/{task_id}/verification.md")
}

pub fn task_attachment_key(task_id: &str, attachment_id: &str, filename: &str) -> String {
    format!("tasks/{task_id}/attachments/{attachment_id}/{filename}")
}

pub fn claude_run_prompt_key(run_id: &str) -> String {
    format!("claude_runs/{run_id}/prompt.md")
}

pub fn claude_run_output_key(run_id: &str) -> String {
    format!("claude_runs/{run_id}/output.txt")
}

// -- Configuration --

/// Configuration for the object store backend.
pub struct StoreConfig {
    /// S3-compatible endpoint URL (e.g., "http://127.0.0.1:3900").
    /// When `None`, use local filesystem.
    pub endpoint_url: Option<String>,
    /// S3 region (e.g., "garage", "us-east-1").
    pub region: Option<String>,
    /// S3 bucket name.
    pub bucket: Option<String>,
    /// AWS access key ID.
    pub access_key_id: Option<String>,
    /// AWS secret access key.
    pub secret_access_key: Option<String>,
    /// Local filesystem base directory (used when S3 is not configured).
    pub local_data_dir: Option<String>,
}

impl StoreConfig {
    /// Build from environment variables.
    /// If `FLOWSTATE_S3_ENDPOINT` (or `AWS_ENDPOINT_URL`) is set along with
    /// credentials and a bucket name, use S3. Otherwise, fall back to local filesystem.
    pub fn from_env() -> Self {
        Self {
            endpoint_url: std::env::var("FLOWSTATE_S3_ENDPOINT")
                .or_else(|_| std::env::var("AWS_ENDPOINT_URL"))
                .ok(),
            region: std::env::var("FLOWSTATE_S3_REGION")
                .or_else(|_| std::env::var("AWS_REGION"))
                .ok(),
            bucket: std::env::var("FLOWSTATE_S3_BUCKET")
                .or_else(|_| std::env::var("GARAGE_BUCKET"))
                .ok(),
            access_key_id: std::env::var("FLOWSTATE_S3_ACCESS_KEY_ID")
                .or_else(|_| std::env::var("AWS_ACCESS_KEY_ID"))
                .ok(),
            secret_access_key: std::env::var("FLOWSTATE_S3_SECRET_ACCESS_KEY")
                .or_else(|_| std::env::var("AWS_SECRET_ACCESS_KEY"))
                .ok(),
            local_data_dir: None,
        }
    }

    pub fn is_s3(&self) -> bool {
        self.endpoint_url.is_some()
            && self.access_key_id.is_some()
            && self.secret_access_key.is_some()
            && self.bucket.is_some()
    }
}

// -- Factory --

/// Create an `ObjectStore` from configuration.
pub fn create_store(config: &StoreConfig) -> Result<Arc<dyn ObjectStore>, StoreError> {
    if config.is_s3() {
        #[cfg(feature = "s3")]
        {
            Ok(Arc::new(S3Store::new(config)?))
        }
        #[cfg(not(feature = "s3"))]
        {
            Err(StoreError::Internal(
                "S3 configuration detected but the 's3' feature is not enabled".into(),
            ))
        }
    } else {
        Ok(Arc::new(LocalStore::new(config)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_helpers_produce_expected_paths() {
        assert_eq!(
            task_spec_key("abc-123"),
            "tasks/abc-123/specification.md"
        );
        assert_eq!(task_plan_key("abc-123"), "tasks/abc-123/plan.md");
        assert_eq!(
            task_attachment_key("abc-123", "att-1", "image.png"),
            "tasks/abc-123/attachments/att-1/image.png"
        );
        assert_eq!(
            claude_run_prompt_key("run-1"),
            "claude_runs/run-1/prompt.md"
        );
        assert_eq!(
            claude_run_output_key("run-1"),
            "claude_runs/run-1/output.txt"
        );
        assert_eq!(
            task_research_key("abc-123"),
            "tasks/abc-123/research.md"
        );
        assert_eq!(
            task_verification_key("abc-123"),
            "tasks/abc-123/verification.md"
        );
    }

    #[test]
    fn store_config_is_s3_requires_all_fields() {
        let config = StoreConfig {
            endpoint_url: Some("http://localhost:3900".into()),
            region: Some("garage".into()),
            bucket: Some("flowstate".into()),
            access_key_id: Some("key".into()),
            secret_access_key: Some("secret".into()),
            local_data_dir: None,
        };
        assert!(config.is_s3());

        // Missing bucket
        let config = StoreConfig {
            endpoint_url: Some("http://localhost:3900".into()),
            region: Some("garage".into()),
            bucket: None,
            access_key_id: Some("key".into()),
            secret_access_key: Some("secret".into()),
            local_data_dir: None,
        };
        assert!(!config.is_s3());

        // Missing credentials
        let config = StoreConfig {
            endpoint_url: Some("http://localhost:3900".into()),
            region: Some("garage".into()),
            bucket: Some("flowstate".into()),
            access_key_id: None,
            secret_access_key: None,
            local_data_dir: None,
        };
        assert!(!config.is_s3());

        // No endpoint → local
        let config = StoreConfig {
            endpoint_url: None,
            region: None,
            bucket: None,
            access_key_id: None,
            secret_access_key: None,
            local_data_dir: None,
        };
        assert!(!config.is_s3());
    }

    #[test]
    fn create_store_local_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let config = StoreConfig {
            endpoint_url: None,
            region: None,
            bucket: None,
            access_key_id: None,
            secret_access_key: None,
            local_data_dir: Some(tmp.path().to_string_lossy().to_string()),
        };
        assert!(!config.is_s3());
        let store = create_store(&config);
        assert!(store.is_ok(), "local store creation should succeed");
    }

    #[test]
    fn create_store_no_local_dir_uses_default() {
        let config = StoreConfig {
            endpoint_url: None,
            region: None,
            bucket: None,
            access_key_id: None,
            secret_access_key: None,
            local_data_dir: None,
        };
        let store = create_store(&config);
        assert!(store.is_ok(), "should fall back to default local dir");
    }

    // These subtests mutate global env vars and must run sequentially
    // in a single test to avoid races with parallel test execution.
    #[test]
    fn store_config_from_env_scenarios() {
        use std::sync::Mutex;
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();

        let clear_all = || {
            for var in [
                "FLOWSTATE_S3_ENDPOINT", "AWS_ENDPOINT_URL",
                "FLOWSTATE_S3_REGION", "AWS_REGION",
                "FLOWSTATE_S3_BUCKET", "GARAGE_BUCKET",
                "FLOWSTATE_S3_ACCESS_KEY_ID", "AWS_ACCESS_KEY_ID",
                "FLOWSTATE_S3_SECRET_ACCESS_KEY", "AWS_SECRET_ACCESS_KEY",
            ] {
                std::env::remove_var(var);
            }
        };

        // Scenario 1: no vars set → all None
        clear_all();
        let config = StoreConfig::from_env();
        assert!(config.endpoint_url.is_none());
        assert!(config.region.is_none());
        assert!(config.bucket.is_none());
        assert!(config.access_key_id.is_none());
        assert!(config.secret_access_key.is_none());
        assert!(!config.is_s3());

        // Scenario 2: AWS_* fallbacks
        clear_all();
        std::env::set_var("AWS_ENDPOINT_URL", "http://aws-endpoint:443");
        std::env::set_var("AWS_REGION", "us-west-2");
        std::env::set_var("AWS_ACCESS_KEY_ID", "aws-key");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "aws-secret");
        std::env::set_var("GARAGE_BUCKET", "my-bucket");
        let config = StoreConfig::from_env();
        assert_eq!(config.endpoint_url.as_deref(), Some("http://aws-endpoint:443"));
        assert_eq!(config.region.as_deref(), Some("us-west-2"));
        assert_eq!(config.bucket.as_deref(), Some("my-bucket"));
        assert_eq!(config.access_key_id.as_deref(), Some("aws-key"));
        assert_eq!(config.secret_access_key.as_deref(), Some("aws-secret"));
        assert!(config.is_s3());

        // Scenario 3: FLOWSTATE_S3_* take precedence over AWS_*
        clear_all();
        std::env::set_var("FLOWSTATE_S3_ENDPOINT", "http://flowstate:3900");
        std::env::set_var("AWS_ENDPOINT_URL", "http://aws:443");
        std::env::set_var("FLOWSTATE_S3_REGION", "garage");
        std::env::set_var("FLOWSTATE_S3_BUCKET", "fs-bucket");
        std::env::set_var("FLOWSTATE_S3_ACCESS_KEY_ID", "fs-key");
        std::env::set_var("FLOWSTATE_S3_SECRET_ACCESS_KEY", "fs-secret");
        let config = StoreConfig::from_env();
        assert_eq!(config.endpoint_url.as_deref(), Some("http://flowstate:3900"));
        assert_eq!(config.region.as_deref(), Some("garage"));
        assert_eq!(config.bucket.as_deref(), Some("fs-bucket"));
        assert_eq!(config.access_key_id.as_deref(), Some("fs-key"));
        assert_eq!(config.secret_access_key.as_deref(), Some("fs-secret"));

        clear_all();
    }
}
