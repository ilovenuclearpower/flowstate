use async_trait::async_trait;
use bytes::Bytes;
use s3::creds::Credentials;
use s3::error::S3Error;
use s3::region::Region;
use s3::Bucket;

use crate::{ObjectStore, StoreConfig, StoreError};

pub struct S3Store {
    bucket: Box<Bucket>,
}

impl std::fmt::Debug for S3Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Store").finish_non_exhaustive()
    }
}

impl S3Store {
    pub fn new(config: &StoreConfig) -> Result<Self, StoreError> {
        let region = Region::Custom {
            region: config.region.clone().unwrap_or_else(|| "us-east-1".into()),
            endpoint: config.endpoint_url.clone().unwrap_or_default(),
        };

        let credentials = Credentials::new(
            config.access_key_id.as_deref(),
            config.secret_access_key.as_deref(),
            None,
            None,
            None,
        )
        .map_err(|e| StoreError::Internal(format!("credentials: {e}")))?;

        let bucket_name = config
            .bucket
            .as_deref()
            .ok_or_else(|| StoreError::Internal("bucket name required".into()))?;

        let mut bucket = Bucket::new(bucket_name, region, credentials)
            .map_err(|e| StoreError::Internal(format!("bucket: {e}")))?;
        bucket.set_path_style();

        Ok(Self { bucket })
    }
}

fn content_type_for_key(key: &str) -> &'static str {
    if key.ends_with(".md") {
        "text/markdown"
    } else if key.ends_with(".txt") {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

fn map_s3_error(e: S3Error) -> StoreError {
    StoreError::Internal(format!("s3: {e}"))
}

#[async_trait]
impl ObjectStore for S3Store {
    async fn put(&self, key: &str, data: Bytes) -> Result<(), StoreError> {
        let content_type = content_type_for_key(key);
        self.bucket
            .put_object_with_content_type(key, &data, content_type)
            .await
            .map_err(map_s3_error)?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Bytes, StoreError> {
        let response = self.bucket.get_object(key).await.map_err(map_s3_error)?;
        if response.status_code() == 404 {
            return Err(StoreError::NotFound(key.to_string()));
        }
        if response.status_code() >= 400 {
            return Err(StoreError::Internal(format!(
                "s3 get {}: status {}",
                key,
                response.status_code()
            )));
        }
        Ok(Bytes::from(response.to_vec()))
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.bucket.delete_object(key).await.map_err(map_s3_error)?;
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let results = self
            .bucket
            .list(prefix.to_string(), None)
            .await
            .map_err(map_s3_error)?;

        let mut keys = Vec::new();
        for result in results {
            for object in result.contents {
                keys.push(object.key);
            }
        }
        keys.sort();
        Ok(keys)
    }

    async fn exists(&self, key: &str) -> Result<bool, StoreError> {
        let response = self.bucket.get_object(key).await.map_err(map_s3_error)?;
        Ok(response.status_code() != 404)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_bucket_produces_error() {
        let config = StoreConfig {
            endpoint_url: Some("http://localhost:3900".into()),
            region: Some("garage".into()),
            bucket: None,
            access_key_id: Some("key".into()),
            secret_access_key: Some("secret".into()),
            local_data_dir: None,
        };
        let err = S3Store::new(&config).unwrap_err();
        assert!(err.to_string().contains("bucket name required"));
    }

    #[test]
    fn valid_config_creates_store() {
        let config = StoreConfig {
            endpoint_url: Some("http://localhost:3900".into()),
            region: Some("garage".into()),
            bucket: Some("test-bucket".into()),
            access_key_id: Some("key".into()),
            secret_access_key: Some("secret".into()),
            local_data_dir: None,
        };
        let store = S3Store::new(&config);
        assert!(store.is_ok());
    }

    #[test]
    fn content_type_detection() {
        assert_eq!(
            content_type_for_key("tasks/a/specification.md"),
            "text/markdown"
        );
        assert_eq!(
            content_type_for_key("claude_runs/r/output.txt"),
            "text/plain"
        );
        assert_eq!(
            content_type_for_key("some/file.png"),
            "application/octet-stream"
        );
    }

    // -- S3 integration tests (require running Garage/MinIO) --

    fn s3_config() -> Option<StoreConfig> {
        let config = StoreConfig::from_env();
        if config.is_s3() {
            Some(config)
        } else {
            None
        }
    }

    #[tokio::test]
    #[ignore]
    async fn s3_crud_roundtrip() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();
        let key = "integration-test/crud-roundtrip.txt";

        // put
        store.put(key, Bytes::from("hello s3")).await.unwrap();

        // get
        let data = store.get(key).await.unwrap();
        assert_eq!(data.as_ref(), b"hello s3");

        // exists
        assert!(store.exists(key).await.unwrap());

        // delete
        store.delete(key).await.unwrap();

        // verify deleted
        let err = store.get(key).await.unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[tokio::test]
    #[ignore]
    async fn s3_not_found() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();

        let err = store
            .get("integration-test/nonexistent-key-12345")
            .await
            .unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[tokio::test]
    #[ignore]
    async fn s3_overwrite() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();
        let key = "integration-test/overwrite.txt";

        store.put(key, Bytes::from("first")).await.unwrap();
        store.put(key, Bytes::from("second")).await.unwrap();

        let data = store.get(key).await.unwrap();
        assert_eq!(data.as_ref(), b"second");

        // cleanup
        store.delete(key).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn s3_list_prefix() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();
        let prefix = "integration-test/list-prefix";

        store
            .put(&format!("{prefix}/a.txt"), Bytes::from("a"))
            .await
            .unwrap();
        store
            .put(&format!("{prefix}/b.txt"), Bytes::from("b"))
            .await
            .unwrap();
        store
            .put(&format!("{prefix}/sub/c.txt"), Bytes::from("c"))
            .await
            .unwrap();

        let keys = store.list(prefix).await.unwrap();
        assert_eq!(keys.len(), 3);

        // cleanup
        for key in &keys {
            store.delete(key).await.unwrap();
        }
    }

    #[tokio::test]
    #[ignore]
    async fn s3_unicode_roundtrip() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();
        let key = "integration-test/unicode.md";
        let content = "# Spécification 🚀\n日本語テスト";

        store.put(key, Bytes::from(content)).await.unwrap();
        let data = store.get(key).await.unwrap();
        assert_eq!(std::str::from_utf8(&data).unwrap(), content);

        store.delete(key).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn s3_large_object() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();
        let key = "integration-test/large.bin";
        let data = vec![0x42u8; 1_000_000]; // 1 MB

        store.put(key, Bytes::from(data.clone())).await.unwrap();
        let result = store.get(key).await.unwrap();
        assert_eq!(result.len(), 1_000_000);
        assert_eq!(result.as_ref(), data.as_slice());

        store.delete(key).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn s3_concurrent_operations() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = std::sync::Arc::new(S3Store::new(&config).unwrap());
        let prefix = "integration-test/concurrent";

        let mut handles = Vec::new();
        for i in 0..5 {
            let store = store.clone();
            let key = format!("{prefix}/{i}.txt");
            handles.push(tokio::spawn(async move {
                store
                    .put(&key, Bytes::from(format!("data-{i}")))
                    .await
                    .unwrap();
                let data = store.get(&key).await.unwrap();
                assert_eq!(data.as_ref(), format!("data-{i}").as_bytes());
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // cleanup
        let keys = store.list(prefix).await.unwrap();
        for key in &keys {
            store.delete(key).await.unwrap();
        }
    }

    #[tokio::test]
    #[ignore]
    async fn s3_delete_nonexistent_is_noop() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();
        // Deleting a key that doesn't exist should not error
        store
            .delete("integration-test/nonexistent-delete-target")
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn s3_list_empty_prefix() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();
        let keys = store
            .list("integration-test/guaranteed-empty-prefix-xyz")
            .await
            .unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn s3_exists_returns_correct_values() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();
        let key = "integration-test/exists-check.txt";

        // Should not exist initially
        assert!(!store.exists(key).await.unwrap());

        // Put something
        store.put(key, Bytes::from("data")).await.unwrap();
        assert!(store.exists(key).await.unwrap());

        // Cleanup
        store.delete(key).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn s3_get_opt_returns_none_for_missing() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();
        let result = store
            .get_opt("integration-test/nonexistent-opt")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore]
    async fn s3_get_opt_returns_some_for_existing() {
        let config = s3_config().expect("S3 not configured — skipped via #[ignore]");
        let store = S3Store::new(&config).unwrap();
        let key = "integration-test/get-opt-existing.txt";

        store.put(key, Bytes::from("hello")).await.unwrap();
        let result = store.get_opt(key).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_ref(), b"hello");

        store.delete(key).await.unwrap();
    }
}
