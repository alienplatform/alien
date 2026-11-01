//! `alien release-key`: the key `alien release --signing-key-file` signs with.
//!
//! The private key stays with the developer (a CI secret, for example). Its
//! public key is what a protected deployment pins at install, so publish it
//! somewhere your customers already trust, such as your documentation.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use alien_core::release_signature::{key_fingerprint, ReleaseSigningKey};
use alien_error::{Context, IntoAlienError};
use clap::{Parser, Subcommand};
use rand::TryRngCore as _;

use crate::error::{ErrorData, Result};

#[derive(Parser, Debug, Clone)]
#[command(about = "Create and inspect release signing keys")]
pub struct ReleaseKeyArgs {
    #[command(subcommand)]
    pub cmd: ReleaseKeyCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ReleaseKeyCmd {
    /// Write a new ECDSA P-256 signing key and print its public key
    Generate {
        /// Where to write the private key (PKCS#8 PEM, readable only by you).
        /// An existing file is never overwritten.
        #[arg(long)]
        out: PathBuf,
    },
    /// Print the public key of a signing key file
    PublicKey {
        /// The private key file
        #[arg(long)]
        key_file: PathBuf,
    },
}

pub async fn release_key_task(args: ReleaseKeyArgs) -> Result<()> {
    let key = match args.cmd {
        ReleaseKeyCmd::Generate { out } => {
            let key = generate()?;
            write_new_private_file(&out, &key.to_pkcs8_pem().context(key_error())?)?;
            eprintln!(
                "Wrote the private key to {}. Keep it secret.",
                out.display()
            );
            key
        }
        ReleaseKeyCmd::PublicKey { key_file } => load_signing_key(&key_file)?,
    };
    let public_key = key.public_key();
    let fingerprint = key_fingerprint(&public_key).context(key_error())?;
    eprintln!("Fingerprint: {fingerprint}");
    println!("{public_key}");
    Ok(())
}

/// Read the signing key `alien release --signing-key-file` names.
pub fn load_signing_key(path: &Path) -> Result<ReleaseSigningKey> {
    let pem =
        fs::read_to_string(path)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: path.display().to_string(),
                reason: "Failed to read the release signing key".to_string(),
            })?;
    ReleaseSigningKey::from_pkcs8_pem(&pem).context(ErrorData::ConfigurationError {
        message: format!(
            "{} is not a release signing key; create one with `alien release-key generate`",
            path.display()
        ),
    })
}

fn generate() -> Result<ReleaseSigningKey> {
    // Random bytes are a valid P-256 scalar except with negligible
    // probability; draw again rather than fail if they aren't.
    loop {
        let mut secret = [0u8; 32];
        rand::rngs::OsRng
            .try_fill_bytes(&mut secret)
            .into_alien_error()
            .context(key_error())?;
        if let Ok(key) = ReleaseSigningKey::from_secret_bytes(&secret) {
            return Ok(key);
        }
    }
}

fn write_new_private_file(path: &Path, contents: &str) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let failed = |reason: &str| ErrorData::FileOperationFailed {
        operation: "write".to_string(),
        file_path: path.display().to_string(),
        reason: reason.to_string(),
    };
    let mut file = options.open(path).into_alien_error().context(failed(
        "Failed to create the key file; it must not exist yet",
    ))?;
    file.write_all(contents.as_bytes())
        .into_alien_error()
        .context(failed("Failed to write the key file"))
}

fn key_error() -> ErrorData {
    ErrorData::ConfigurationError {
        message: "Failed to create the release signing key".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_key_is_written_privately_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("release.pem");
        let key = generate().unwrap();
        write_new_private_file(&path, &key.to_pkcs8_pem().unwrap()).unwrap();

        assert_eq!(
            load_signing_key(&path).unwrap().public_key(),
            key.public_key()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        write_new_private_file(&path, "other")
            .expect_err("an existing key file must never be overwritten");
        assert_eq!(
            load_signing_key(&path).unwrap().public_key(),
            key.public_key()
        );
    }

    #[test]
    fn two_generated_keys_differ() {
        assert_ne!(
            generate().unwrap().public_key(),
            generate().unwrap().public_key()
        );
    }
}
