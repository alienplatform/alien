use std::path::Path;

/// Write content to a file with owner-only permissions (0600 on Unix).
///
/// Use this for any file containing sensitive data: tokens, secrets, credentials,
/// encryption keys, or other material that should not be world-readable.
pub fn write_secret_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(content)?;
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content)
    }
}
