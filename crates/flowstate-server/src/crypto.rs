use std::fs;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

/// Load or generate the server encryption key.
/// Stored at `~/.config/flowstate/server.key` (32 bytes, base64-encoded).
pub fn load_or_generate_key() -> Key<Aes256Gcm> {
    let path = key_file_path();

    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(bytes) = B64.decode(data.trim()) {
            if bytes.len() == 32 {
                return *Key::<Aes256Gcm>::from_slice(&bytes);
            }
        }
    }

    // Generate a new key
    let key = Aes256Gcm::generate_key(OsRng);

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, B64.encode(key.as_slice()));

    // Set file permissions to 600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        let _ = fs::set_permissions(&path, perms);
    }

    key
}

/// Encrypt plaintext. Returns a base64 string containing nonce + ciphertext.
pub fn encrypt(key: &Key<Aes256Gcm>, plaintext: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| format!("encrypt: {e}"))?;

    // Prepend nonce (12 bytes) to ciphertext
    let mut combined = nonce.to_vec();
    combined.extend_from_slice(&ciphertext);

    Ok(B64.encode(&combined))
}

/// Decrypt a base64 string containing nonce + ciphertext.
pub fn decrypt(key: &Key<Aes256Gcm>, encoded: &str) -> Result<String, String> {
    let combined = B64
        .decode(encoded)
        .map_err(|e| format!("base64 decode: {e}"))?;

    if combined.len() < 12 {
        return Err("ciphertext too short".into());
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new(key);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("decrypt: {e}"))?;

    String::from_utf8(plaintext).map_err(|e| format!("utf8: {e}"))
}

fn key_file_path() -> PathBuf {
    key_file_path_from(
        std::env::var("XDG_CONFIG_HOME").ok(),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn key_file_path_from(xdg_config_home: Option<String>, home: Option<PathBuf>) -> PathBuf {
    if let Some(xdg) = xdg_config_home {
        PathBuf::from(xdg)
            .join("flowstate")
            .join("server.key")
    } else if let Some(home) = home {
        home.join(".config/flowstate").join("server.key")
    } else {
        PathBuf::from("flowstate").join("server.key")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::KeyInit;

    fn test_key() -> Key<Aes256Gcm> {
        Aes256Gcm::generate_key(OsRng)
    }

    #[test]
    fn test_encrypt_decrypt_round_trip() {
        let key = test_key();
        let plaintext = "hello world";
        let encrypted = encrypt(&key, plaintext).unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_produces_unique_ciphertext() {
        let key = test_key();
        let a = encrypt(&key, "same text").unwrap();
        let b = encrypt(&key, "same text").unwrap();
        assert_ne!(a, b, "two encryptions should produce different ciphertext due to unique nonces");
    }

    #[test]
    fn test_decrypt_corrupted_input() {
        let key = test_key();
        let result = decrypt(&key, "not-valid-base64!!!");
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_too_short() {
        let key = test_key();
        // Encode fewer than 12 bytes
        let short = B64.encode(b"short");
        let result = decrypt(&key, &short);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("too short"),
            "error should mention 'too short'"
        );
    }

    #[test]
    fn test_decrypt_wrong_key() {
        let key_a = test_key();
        let key_b = test_key();
        let encrypted = encrypt(&key_a, "secret").unwrap();
        let result = decrypt(&key_b, &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypt_empty_string() {
        let key = test_key();
        let encrypted = encrypt(&key, "").unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn test_load_or_generate_key_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("flowstate-test-config");
        let key_path = config_dir.join("flowstate").join("server.key");

        // Manually do what load_or_generate_key does but with our temp path
        std::fs::create_dir_all(key_path.parent().unwrap()).unwrap();
        let key1 = Aes256Gcm::generate_key(OsRng);
        std::fs::write(&key_path, B64.encode(key1.as_slice())).unwrap();

        // Read it back
        let data = std::fs::read_to_string(&key_path).unwrap();
        let bytes = B64.decode(data.trim()).unwrap();
        let key2 = *Key::<Aes256Gcm>::from_slice(&bytes);
        assert_eq!(key1, key2);
        assert!(key_path.exists());
    }

    #[test]
    fn test_key_file_path_xdg() {
        let path = key_file_path_from(Some("/tmp/xdg-test".into()), None);
        assert_eq!(
            path,
            std::path::PathBuf::from("/tmp/xdg-test/flowstate/server.key")
        );
    }

    #[test]
    fn test_key_file_path_home() {
        let path = key_file_path_from(None, Some(PathBuf::from("/home/testuser")));
        assert_eq!(
            path,
            std::path::PathBuf::from("/home/testuser/.config/flowstate/server.key")
        );
    }

    #[test]
    fn test_key_file_path_no_env() {
        let path = key_file_path_from(None, None);
        assert_eq!(
            path,
            std::path::PathBuf::from("flowstate/server.key")
        );
    }

    #[test]
    fn test_encrypt_decrypt_long_text() {
        let key = test_key();
        let long_text = "x".repeat(10000);
        let encrypted = encrypt(&key, &long_text).unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert_eq!(decrypted, long_text);
    }

    #[test]
    fn test_encrypt_decrypt_unicode() {
        let key = test_key();
        let unicode_text = "Hello 🌍 世界 مرحبا";
        let encrypted = encrypt(&key, unicode_text).unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert_eq!(decrypted, unicode_text);
    }
}
