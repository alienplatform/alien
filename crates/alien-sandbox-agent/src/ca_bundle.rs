//! The trust bundle for a sandbox whose HTTPS the gateway intercepts to add credentials, on a
//! platform where the agent forwards the sandbox's traffic (no gateway egress mode of its own).
//!
//! The bundle is the image's system roots plus the per-sandbox CA the gateway signs with. It is
//! written once, readable by every command, and the commands' environment points the usual
//! trust variables at it. Only the public certificate is accepted.

use std::{
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// Where the bundle lives. A temp directory every command can read.
pub const BUNDLE_PATH: &str = "/tmp/alien-egress-ca-bundle.pem";

/// The image's system trust roots.
pub const SYSTEM_ROOTS: &str = "/etc/ssl/certs/ca-certificates.crt";

static INSTALLED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The path of the bundle this agent has installed, if any.
pub fn installed_path() -> Option<String> {
    INSTALLED.lock().ok().and_then(|path| path.clone())
}

const MAX_CERTIFICATE_BYTES: usize = 16 * 1024;

/// Checks that `certificate` is a single public PEM certificate.
pub fn validate(certificate: &str) -> Result<(), String> {
    let trimmed = certificate.trim();
    if trimmed.len() > MAX_CERTIFICATE_BYTES
        || !trimmed.starts_with("-----BEGIN CERTIFICATE-----")
        || !trimmed.ends_with("-----END CERTIFICATE-----")
        || trimmed.matches("-----BEGIN").count() != 1
        || trimmed.contains("PRIVATE KEY")
    {
        return Err("invalid egress CA certificate".into());
    }
    Ok(())
}

/// Writes system roots followed by `certificate` to `destination`, replacing it atomically.
pub fn install(
    certificate: &str,
    system_roots: &Path,
    destination: &Path,
) -> Result<PathBuf, String> {
    validate(certificate)?;
    let system = std::fs::read(system_roots)
        .map_err(|error| format!("read system trust roots: {error}"))?;
    let temporary = destination.with_extension(format!("pem.{}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)?;
        file.write_all(&system)?;
        if !system.ends_with(b"\n") {
            file.write_all(b"\n")?;
        }
        file.write_all(certificate.trim().as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o644))?;
        std::fs::rename(&temporary, destination)
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("write egress trust bundle: {error}"));
    }
    if let Ok(mut installed) = INSTALLED.lock() {
        *installed = Some(destination.to_string_lossy().into_owned());
    }
    Ok(destination.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CERT: &str = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";

    #[test]
    fn a_private_key_or_a_second_block_is_refused() {
        assert!(validate("-----BEGIN PRIVATE KEY-----\nx\n-----END PRIVATE KEY-----").is_err());
        let two = format!("{CERT}{CERT}");
        assert!(validate(&two).is_err());
        assert!(validate("hello").is_err());
        assert!(validate(&"-----BEGIN CERTIFICATE-----".repeat(1000)).is_err());
        assert!(validate(CERT).is_ok());
    }

    #[test]
    fn the_bundle_holds_system_roots_then_the_ca_and_is_replaced_whole() {
        let dir = tempfile::tempdir().unwrap();
        let roots = dir.path().join("roots.pem");
        std::fs::write(&roots, "ROOTS-NO-NEWLINE").unwrap();
        let bundle = dir.path().join("bundle.pem");
        install(CERT, &roots, &bundle).unwrap();
        let text = std::fs::read_to_string(&bundle).unwrap();
        assert!(text.starts_with("ROOTS-NO-NEWLINE\n-----BEGIN CERTIFICATE-----"));
        assert_eq!(text.matches("BEGIN CERTIFICATE").count(), 1);
        assert_eq!(
            std::fs::metadata(&bundle).unwrap().permissions().mode() & 0o777,
            0o644
        );
        // A second install replaces, not appends, and leaves no temporary file.
        install(CERT, &roots, &bundle).unwrap();
        assert_eq!(
            std::fs::read_to_string(&bundle)
                .unwrap()
                .matches("BEGIN CERTIFICATE")
                .count(),
            1
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_missing_system_bundle_or_a_bad_certificate_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("bundle.pem");
        assert!(install(CERT, &dir.path().join("none"), &bundle).is_err());
        let roots = dir.path().join("roots.pem");
        std::fs::write(&roots, "R").unwrap();
        assert!(install("junk", &roots, &bundle).is_err());
        assert!(!bundle.exists());
    }
}
