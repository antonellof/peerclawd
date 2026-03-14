//! Bootstrap - Base directory resolution and environment loading.

use std::path::PathBuf;

/// Get the base directory for PeerClaw'd data.
///
/// Priority:
/// 1. `PEERCLAWD_HOME` environment variable
/// 2. `~/.peerclawd` on Unix systems
/// 3. `%APPDATA%\peerclawd` on Windows
pub fn base_dir() -> PathBuf {
    if let Ok(home) = std::env::var("PEERCLAWD_HOME") {
        return PathBuf::from(home);
    }

    dirs::home_dir()
        .map(|h| h.join(".peerclawd"))
        .unwrap_or_else(|| PathBuf::from(".peerclawd"))
}

/// Get the directory for WASM tools.
pub fn tools_dir() -> PathBuf {
    base_dir().join("tools")
}

/// Get the directory for agent specs.
pub fn agents_dir() -> PathBuf {
    base_dir().join("agents")
}

/// Get the directory for data (database, etc.).
pub fn data_dir() -> PathBuf {
    base_dir().join("data")
}

/// Get the directory for models (LLM weights).
pub fn models_dir() -> PathBuf {
    base_dir().join("models")
}

/// Get the path to the identity key file.
pub fn identity_path() -> PathBuf {
    base_dir().join("identity.key")
}

/// Get the path to the database file.
pub fn database_path() -> PathBuf {
    data_dir().join("peerclawd.redb")
}

/// Load environment variables from `.peerclawd/.env` if present.
pub fn load_env() {
    let env_path = base_dir().join(".env");
    if env_path.exists() {
        if let Err(e) = dotenvy::from_path(&env_path) {
            tracing::warn!("Failed to load .env from {:?}: {}", env_path, e);
        }
    }
}

/// Ensure all required directories exist.
pub fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(base_dir())?;
    std::fs::create_dir_all(tools_dir())?;
    std::fs::create_dir_all(agents_dir())?;
    std::fs::create_dir_all(data_dir())?;
    std::fs::create_dir_all(models_dir())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_dir_exists() {
        let dir = base_dir();
        assert!(dir.ends_with(".peerclawd"));
    }

    #[test]
    fn test_subdirs() {
        assert!(tools_dir().ends_with("tools"));
        assert!(agents_dir().ends_with("agents"));
        assert!(data_dir().ends_with("data"));
        assert!(models_dir().ends_with("models"));
    }
}
