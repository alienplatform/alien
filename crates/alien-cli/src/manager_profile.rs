//! The manager this CLI talks to when it isn't the hosted platform.
//!
//! `alien login --manager <url> --token <key>` saves it; `alien logout` (or
//! `alien login` to the hosted platform) removes it. `ALIEN_MANAGER_URL` and
//! `ALIEN_API_KEY` override it for one invocation.

use std::{fs, path::PathBuf};

use alien_error::{AlienError, Context, IntoAlienError};
use dirs::config_dir;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerProfile {
    /// Manager URL, e.g. `https://manager.example.com`.
    pub url: String,
    /// Admin or developer API key.
    pub api_key: String,
}

fn path() -> PathBuf {
    config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("alien")
        .join("manager.json")
}

/// The saved manager, if any.
pub fn load() -> Result<Option<ManagerProfile>> {
    let path = path();
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!("Failed to read {}: {e}", path.display()),
            }))
        }
    };
    serde_json::from_str(&contents)
        .map(Some)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!(
                "{} is not valid. Run `alien login --manager <url>` again.",
                path.display()
            ),
        })
}

/// Save `profile`, readable only by the current user.
pub fn save(profile: &ManagerProfile) -> Result<()> {
    let path = path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to create {}", parent.display()),
            })?;
    }
    let contents = serde_json::to_vec_pretty(profile)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to encode the manager profile".to_string(),
        })?;
    write_private(&path, &contents)
}

/// Forget the saved manager.
pub fn clear() -> Result<bool> {
    match fs::remove_file(path()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to remove the manager profile: {e}"),
        })),
    }
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to write {}", path.display()),
        })?;
    file.write_all(contents)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to write {}", path.display()),
        })
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    fs::write(path, contents)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to write {}", path.display()),
        })
}
