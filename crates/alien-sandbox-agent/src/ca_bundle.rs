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

/// Where images keep their system trust roots, most common first.
pub const SYSTEM_ROOTS: [&str; 3] = [
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/cert.pem",
];

/// The first readable system bundle among `candidates`; empty when the image has none, so
/// the sandbox trusts the per-sandbox CA alone rather than failing to start.
pub fn read_system_roots<P: AsRef<Path>>(candidates: &[P]) -> Vec<u8> {
    candidates
        .iter()
        .find_map(|path| std::fs::read(path).ok())
        .unwrap_or_default()
}

static INSTALLED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The path of the bundle this agent has installed, if any.
pub fn installed_path() -> Option<String> {
    INSTALLED.lock().ok().and_then(|path| path.clone())
}

/// The variables the common clients read for their trust roots.
pub const TRUST_VARIABLES: [&str; 4] = [
    "SSL_CERT_FILE",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "NODE_EXTRA_CA_CERTS",
];

/// Point every trust variable of a command's environment at the installed bundle, if any.
pub fn command_environment(env: &mut std::collections::BTreeMap<String, String>) {
    if let Some(path) = installed_path() {
        trust_environment(&path, env);
    }
}

fn trust_environment(path: &str, env: &mut std::collections::BTreeMap<String, String>) {
    for key in TRUST_VARIABLES {
        env.insert(key.into(), path.to_string());
    }
}

/// Install the CA a platform handed over in `ALIEN_SANDBOX_EGRESS_CA`, for an agent that has no
/// gateway egress mode of its own to do it (Azure). Nothing happens without the variable.
pub fn install_from_environment() -> Result<(), String> {
    let Ok(certificate) = std::env::var("ALIEN_SANDBOX_EGRESS_CA") else {
        return Ok(());
    };
    install(
        &certificate,
        &SYSTEM_ROOTS.map(Path::new),
        Path::new(BUNDLE_PATH),
    )
    .map(|_| ())
}

/// Install a handed-over CA, when there is one, at `destination`. A refused certificate is an
/// error, so an agent never starts believing it trusts a CA it does not.
#[cfg(test)]
fn install_handed_over(
    certificate: Option<&str>,
    roots: &[&Path],
    destination: &Path,
) -> Result<Option<PathBuf>, String> {
    certificate
        .map(|certificate| install(certificate, roots, destination))
        .transpose()
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
    system_roots: &[&Path],
    destination: &Path,
) -> Result<PathBuf, String> {
    validate(certificate)?;
    let system = read_system_roots(system_roots);
    let temporary = destination.with_extension(format!("pem.{}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)?;
        file.write_all(&system)?;
        if !system.is_empty() && !system.ends_with(b"\n") {
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
        let key = format!(
            "-----BEGIN {} KEY-----\nx\n-----END {} KEY-----",
            "PRIVATE", "PRIVATE"
        );
        assert!(validate(&key).is_err());
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
        install(CERT, &[roots.as_path()], &bundle).unwrap();
        let text = std::fs::read_to_string(&bundle).unwrap();
        assert!(text.starts_with("ROOTS-NO-NEWLINE\n-----BEGIN CERTIFICATE-----"));
        assert_eq!(text.matches("BEGIN CERTIFICATE").count(), 1);
        assert_eq!(
            std::fs::metadata(&bundle).unwrap().permissions().mode() & 0o777,
            0o644
        );
        // A second install replaces, not appends, and leaves no temporary file.
        install(CERT, &[roots.as_path()], &bundle).unwrap();
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
    fn a_handed_over_ca_is_installed_and_every_trust_variable_points_at_it() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("bundle.pem");
        let installed = install_handed_over(Some(CERT), &[], &bundle).unwrap().unwrap();
        assert!(std::fs::read_to_string(&installed).unwrap().contains("BEGIN CERTIFICATE"));
        let mut env = std::collections::BTreeMap::new();
        trust_environment(installed.to_str().unwrap(), &mut env);
        for key in TRUST_VARIABLES {
            assert_eq!(env[key], bundle.to_str().unwrap());
        }
        // No CA handed over: nothing is installed. A refused one is an error, not a skip.
        assert!(install_handed_over(None, &[], &bundle).unwrap().is_none());
        assert!(install_handed_over(Some("nope"), &[], &bundle).is_err());
    }

    #[test]
    fn a_bad_certificate_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("bundle.pem");
        let roots = dir.path().join("roots.pem");
        std::fs::write(&roots, "R").unwrap();
        assert!(install("junk", &[roots.as_path()], &bundle).is_err());
        assert!(!bundle.exists());
    }

    #[test]
    fn an_image_without_the_first_bundle_uses_the_next_and_none_installs_the_ca_alone() {
        let dir = tempfile::tempdir().unwrap();
        let second = dir.path().join("rhel-bundle.crt");
        std::fs::write(&second, "RHEL-ROOTS\n").unwrap();
        let missing = dir.path().join("none");
        let bundle = dir.path().join("bundle.pem");
        install(CERT, &[missing.as_path(), second.as_path()], &bundle).unwrap();
        assert!(std::fs::read_to_string(&bundle)
            .unwrap()
            .starts_with("RHEL-ROOTS\n-----BEGIN"));
        install(CERT, &[missing.as_path()], &bundle).unwrap();
        assert_eq!(
            std::fs::read_to_string(&bundle).unwrap(),
            format!("{}\n", CERT.trim())
        );
    }
}
