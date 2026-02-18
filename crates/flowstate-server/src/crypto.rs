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
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
            .join("flowstate")
            .join("server.key")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".config/flowstate")
            .join("server.key")
    } else {
        PathBuf::from("flowstate").join("server.key")
    }
}
