//! Authorize an HTTP route using the cloud's authenticated native file transport.
//!
//! The token only authorizes this boot of this sandbox. It is never a cloud credential.
//! Guest root can read it, but it cannot grant commands capabilities or reach another sandbox.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

use alien_error::{Context, IntoAlienError};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use subtle::ConstantTimeEq;

use crate::error::{ErrorData, Result};

/// Deliberately has no Debug implementation: neither logs nor errors may contain the token.
pub struct NativeToken(String);

impl NativeToken {
    /// Replace any previous boot's credential before the image command starts or HTTP serves.
    /// The containing directory is installed by the image assembler and owned by root.
    pub fn create(path: &Path) -> Result<Self> {
        let failed = || ErrorData::ConfigInvalid {
            setting: "native-file authorization".into(),
            reason: "cannot install the per-boot credential".into(),
        };
        let mut random = [0u8; 32];
        File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut random))
            .into_alien_error()
            .context(failed())?;
        let token = Self(STANDARD.encode(random));
        let temporary = path.with_extension(uuid::Uuid::new_v4().to_string());
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .into_alien_error()
            .context(failed())?;
        file.write_all(token.0.as_bytes())
            .into_alien_error()
            .context(failed())?;
        fs::rename(&temporary, path)
            .into_alien_error()
            .context(failed())?;
        Ok(token)
    }

    /// Compare fixed-length credentials without leaking matching prefixes.
    pub fn accepts(&self, presented: &str) -> bool {
        self.0.as_bytes().ct_eq(presented.as_bytes()).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn a_new_boot_replaces_the_credential_without_following_an_old_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        let target = dir.path().join("untouched");
        fs::write(&target, "original").unwrap();
        symlink(&target, &path).unwrap();
        let first = NativeToken::create(&path).unwrap();
        let old = fs::read_to_string(&path).unwrap();
        assert!(first.accepts(&old));
        assert!(!first.accepts(""));
        assert!(!first.accepts(&"x".repeat(old.len())));
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let second = NativeToken::create(&path).unwrap();
        assert!(!second.accepts(&old));
        assert!(second.accepts(&fs::read_to_string(&path).unwrap()));
    }
}
