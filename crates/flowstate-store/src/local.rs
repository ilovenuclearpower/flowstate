use std::path::PathBuf;

use async_trait::async_trait;
use bytes::Bytes;

use crate::{ObjectStore, StoreConfig, StoreError};

pub struct LocalStore {
    base_dir: PathBuf,
}

impl LocalStore {
    pub fn new(config: &StoreConfig) -> Self {
        let base_dir = config
            .local_data_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(default_data_dir);
        Self { base_dir }
    }

    pub fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    fn resolve(&self, key: &str) -> PathBuf {
        self.base_dir.join(key)
    }
}

/// Reproduce the same default data directory logic as `flowstate_db::data_dir()`
/// without taking a dependency on the db crate.
fn default_data_dir() -> PathBuf {
    let base = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/share")
    } else {
        PathBuf::from(".")
    };
    base.join("flowstate")
}

#[async_trait]
impl ObjectStore for LocalStore {
    async fn put(&self, key: &str, data: Bytes) -> Result<(), StoreError> {
        let path = self.resolve(key);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| StoreError::Internal(format!("mkdir: {e}")))?;
        }
        tokio::fs::write(&path, &data)
            .await
            .map_err(|e| StoreError::Internal(format!("write {}: {e}", path.display())))
    }

    async fn get(&self, key: &str) -> Result<Bytes, StoreError> {
        let path = self.resolve(key);
        match tokio::fs::read(&path).await {
            Ok(data) => Ok(Bytes::from(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StoreError::NotFound(key.to_string()))
            }
            Err(e) => Err(StoreError::Internal(format!(
                "read {}: {e}",
                path.display()
            ))),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        let path = self.resolve(key);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError::Internal(format!(
                "delete {}: {e}",
                path.display()
            ))),
        }
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let dir = self.resolve(prefix);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut keys = Vec::new();
        let mut stack = vec![dir];
        while let Some(current) = stack.pop() {
            let mut entries = match tokio::fs::read_dir(&current).await {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(StoreError::Internal(format!(
                        "list {}: {e}",
                        current.display()
                    )))
                }
            };
            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|e| StoreError::Internal(format!("read_dir entry: {e}")))?
            {
                let path = entry.path();
                let ft = entry
                    .file_type()
                    .await
                    .map_err(|e| StoreError::Internal(format!("file_type: {e}")))?;
                if ft.is_dir() {
                    stack.push(path);
                } else {
                    // Produce a key relative to base_dir
                    if let Ok(rel) = path.strip_prefix(&self.base_dir) {
                        keys.push(rel.to_string_lossy().to_string());
                    }
                }
            }
        }
        keys.sort();
        Ok(keys)
    }

    async fn exists(&self, key: &str) -> Result<bool, StoreError> {
        let path = self.resolve(key);
        match tokio::fs::try_exists(&path).await {
            Ok(exists) => Ok(exists),
            Err(e) => Err(StoreError::Internal(format!(
                "exists {}: {e}",
                path.display()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store(dir: &std::path::Path) -> LocalStore {
        let config = StoreConfig {
            endpoint_url: None,
            region: None,
            bucket: None,
            access_key_id: None,
            secret_access_key: None,
            local_data_dir: Some(dir.to_string_lossy().to_string()),
        };
        LocalStore::new(&config)
    }

    #[tokio::test]
    async fn put_then_get_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());

        store
            .put("tasks/abc/specification.md", Bytes::from("hello world"))
            .await
            .unwrap();
        let data = store.get("tasks/abc/specification.md").await.unwrap();
        assert_eq!(data.as_ref(), b"hello world");
    }

    #[tokio::test]
    async fn get_missing_returns_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());

        let err = store.get("nonexistent/key").await.unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn get_opt_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());

        let result = store.get_opt("nonexistent/key").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn put_overwrites_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());

        store
            .put("key", Bytes::from("first"))
            .await
            .unwrap();
        store
            .put("key", Bytes::from("second"))
            .await
            .unwrap();

        let data = store.get("key").await.unwrap();
        assert_eq!(data.as_ref(), b"second");
    }

    #[tokio::test]
    async fn delete_removes_object() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());

        store.put("key", Bytes::from("data")).await.unwrap();
        assert!(store.exists("key").await.unwrap());

        store.delete("key").await.unwrap();
        assert!(!store.exists("key").await.unwrap());
    }

    #[tokio::test]
    async fn delete_missing_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());

        // Should not error
        store.delete("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn list_returns_keys_with_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());

        store
            .put("tasks/a/specification.md", Bytes::from("spec a"))
            .await
            .unwrap();
        store
            .put("tasks/a/plan.md", Bytes::from("plan a"))
            .await
            .unwrap();
        store
            .put("tasks/b/specification.md", Bytes::from("spec b"))
            .await
            .unwrap();
        store
            .put("other/file.txt", Bytes::from("other"))
            .await
            .unwrap();

        let keys = store.list("tasks/a").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"tasks/a/plan.md".to_string()));
        assert!(keys.contains(&"tasks/a/specification.md".to_string()));

        let all_tasks = store.list("tasks").await.unwrap();
        assert_eq!(all_tasks.len(), 3);
    }

    #[tokio::test]
    async fn list_empty_prefix_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());

        let keys = store.list("nonexistent").await.unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn exists_returns_correct_values() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());

        assert!(!store.exists("key").await.unwrap());
        store.put("key", Bytes::from("data")).await.unwrap();
        assert!(store.exists("key").await.unwrap());
    }

    #[tokio::test]
    async fn unicode_content_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());

        let content = "# Spécification 🚀\n\nCeci est un test avec des caractères spéciaux: é, ñ, ü, 日本語";
        store
            .put("tasks/unicode/specification.md", Bytes::from(content))
            .await
            .unwrap();
        let data = store.get("tasks/unicode/specification.md").await.unwrap();
        assert_eq!(std::str::from_utf8(&data).unwrap(), content);
    }
}
