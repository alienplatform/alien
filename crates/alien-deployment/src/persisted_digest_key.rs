//! A secrets digest key that survives a restart of the process that steps a deployment.
//!
//! The keyed checksums of workload secrets live in the deployment's saved
//! state. A key that changes on every start changes every checksum, so every
//! workload with secret environment variables would roll once per restart. A
//! process with local storage creates its key once and keeps it.

use std::path::Path;

use alien_error::AlienError;

use crate::{ErrorData, Result};

const KEY_LEN: usize = 32;

fn internal(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InternalError {
        message: message.into(),
    })
}

/// A new random key.
pub fn generate_digest_key() -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut key);
    key
}

/// The text form of a key, for storage.
pub fn encode_digest_key(key: &[u8; KEY_LEN]) -> String {
    hex::encode(key)
}

/// Reads the text form back. The error never contains the stored text.
pub fn decode_digest_key(stored: &str) -> Result<[u8; KEY_LEN]> {
    hex::decode(stored.trim())
        .ok()
        .and_then(|bytes| <[u8; KEY_LEN]>::try_from(bytes).ok())
        .ok_or_else(|| internal("The stored secrets digest key is damaged"))
}

/// Reads the key in `path`, creating the file with a new key on the first call.
///
/// The key is created once and never rotated. The file is readable by its
/// owner only. Two processes that start together end up with the same key: the
/// new file is linked into place, which fails if another process got there
/// first, and the loser reads the winner's key.
pub fn load_or_create_digest_key_file(path: &Path) -> Result<[u8; KEY_LEN]> {
    match std::fs::read_to_string(path) {
        Ok(stored) => return decode_digest_key(&stored),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(internal("Could not read the secrets digest key file")),
    }

    let dir = path
        .parent()
        .ok_or_else(|| internal("The secrets digest key file has no directory"))?;
    std::fs::create_dir_all(dir)
        .map_err(|_| internal("Could not create the directory of the secrets digest key file"))?;

    let key = generate_digest_key();
    let pending = dir.join(format!(".secrets-digest-key.{}", uuid::Uuid::new_v4()));
    write_private(&pending, encode_digest_key(&key).as_bytes())
        .map_err(|_| internal("Could not write the secrets digest key file"))?;
    let linked = std::fs::hard_link(&pending, path);
    let _ = std::fs::remove_file(&pending);
    match linked {
        Ok(()) => Ok(key),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let stored = std::fs::read_to_string(path)
                .map_err(|_| internal("Could not read the secrets digest key file"))?;
            decode_digest_key(&stored)
        }
        Err(_) => Err(internal("Could not create the secrets digest key file")),
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Uses the key kept in `path` for this process, unless a key is already set.
pub fn use_persistent_secrets_digest_key(path: &Path) -> Result<()> {
    let key = load_or_create_digest_key_file(path)?;
    crate::set_secrets_digest_key_if_unset(key);
    Ok(())
}

/// Uses the key kept in the user's config directory for this process, for a
/// command line that steps deployments from a person's machine. Every command
/// of the same user shares the key. When the key cannot be kept, a key that
/// lasts for this process is used, so the next run may roll workloads once.
pub fn use_user_secrets_digest_key() {
    let path = dirs::config_dir().map(|dir| dir.join("alien").join("secrets-digest-key"));
    match path.map(|path| use_persistent_secrets_digest_key(&path)) {
        Some(Ok(())) => {}
        Some(Err(error)) => {
            tracing::warn!(%error, "Could not keep the secrets digest key; using one for this run only");
            crate::set_ephemeral_secrets_digest_key();
        }
        None => {
            tracing::warn!(
                "No user config directory; using a secrets digest key for this run only"
            );
            crate::set_ephemeral_secrets_digest_key();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_created_once_and_read_back_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("secrets-digest-key");
        let first = load_or_create_digest_key_file(&path).unwrap();
        let second = load_or_create_digest_key_file(&path).unwrap();
        assert_eq!(first, second);
        assert_ne!(first, [0u8; KEY_LEN]);
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no temporary file stays behind");
    }

    #[cfg(unix)]
    #[test]
    fn the_key_file_is_private_to_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets-digest-key");
        load_or_create_digest_key_file(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_damaged_file_is_an_error_not_a_new_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets-digest-key");
        std::fs::write(&path, "not a key").unwrap();
        let error = load_or_create_digest_key_file(&path).unwrap_err();
        assert!(error.to_string().contains("damaged"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not a key");
    }

    #[test]
    fn processes_starting_together_agree_on_one_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets-digest-key");
        let keys: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || load_or_create_digest_key_file(&path).unwrap())
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert!(keys.windows(2).all(|pair| pair[0] == pair[1]));
    }
}
