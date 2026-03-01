use std::fs;
use std::path::PathBuf;

use anyhow::Result;

fn config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("flowstate")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config/flowstate")
    } else {
        PathBuf::from(".config/flowstate")
    }
}

fn credentials_path() -> PathBuf {
    config_dir().join("runner-credentials")
}

/// Load a saved API key from the credentials file.
pub fn load_saved_key() -> Option<String> {
    let path = credentials_path();
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Save an API key to the credentials file with restricted permissions.
pub fn save_key(api_key: &str) -> Result<()> {
    let path = credentials_path();
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, api_key)?;

    // Set file permissions to 0600 (owner read/write only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_nonexistent_returns_none() {
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/flowstate-test-nonexistent");
        let key = load_saved_key();
        assert!(key.is_none());
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        save_key("fs_test123").unwrap();
        let loaded = load_saved_key();
        assert_eq!(loaded.as_deref(), Some("fs_test123"));
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}
