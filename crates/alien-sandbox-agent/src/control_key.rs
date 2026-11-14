//! The key that authorizes control requests where the cloud's ingress cannot.
//!
//! On the GCP Agent Platform the edge accepts a credential that is execute-capable for the whole
//! sandbox and routes it to any declared port, the agent's own included. The credential a preview
//! holder receives therefore reaches `/v1/exec`, so reaching the agent proves nothing. The binding
//! installs a key once per boot, derives it again in any process from a signature only it can make,
//! and presents it with every control request. A preview holder is handed a capability for one
//! port and never sees the key.
//!
//! Until a key is installed the agent behaves as it did before this existed, which keeps sandboxes
//! of deployments without preview unchanged. Neither the key nor its digest is ever logged.

use std::sync::OnceLock;

use alien_core::sandbox_image::CONTROL_KEY_MIN_LEN;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Longest key accepted; keeps a hostile install from making the agent hold a large value.
const CONTROL_KEY_MAX_LEN: usize = 256;

/// What installing a key did.
#[derive(Debug, PartialEq, Eq)]
pub enum Install {
    /// This boot had no key and now has one.
    Installed,
    /// The same key was installed already.
    Unchanged,
    /// Another key is installed, and one boot has one key.
    Conflict,
    /// The key is too short or too long to be one the binding derived.
    Malformed,
}

/// The per-boot control key, held as a digest.
#[derive(Default)]
pub struct ControlKey {
    digest: OnceLock<[u8; 32]>,
}

fn digest(key: &str) -> [u8; 32] {
    Sha256::digest(key.as_bytes()).into()
}

impl ControlKey {
    /// Whether a key has been installed on this boot.
    pub fn is_installed(&self) -> bool {
        self.digest.get().is_some()
    }

    /// Installs `key` unless this boot has one already.
    pub fn install(&self, key: &str) -> Install {
        if !(CONTROL_KEY_MIN_LEN..=CONTROL_KEY_MAX_LEN).contains(&key.len()) {
            return Install::Malformed;
        }
        let wanted = digest(key);
        match self.digest.set(wanted) {
            Ok(()) => Install::Installed,
            Err(_) => match self.digest.get() {
                Some(held) if bool::from(held.ct_eq(&wanted)) => Install::Unchanged,
                _ => Install::Conflict,
            },
        }
    }

    /// Whether a request presenting `presented` may reach a control route. With no key installed
    /// every request may (the behaviour of an agent that was never given one).
    pub fn admits(&self, presented: Option<&str>) -> bool {
        match self.digest.get() {
            None => true,
            Some(held) => match presented {
                Some(key) => bool::from(held.ct_eq(&digest(key))),
                None => false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn an_agent_without_a_key_admits_every_request() {
        let control = ControlKey::default();
        assert!(control.admits(None));
        assert!(control.admits(Some("anything")));
    }

    #[test]
    fn an_installed_key_admits_only_itself() {
        let control = ControlKey::default();
        assert_eq!(control.install(KEY), Install::Installed);
        assert!(control.admits(Some(KEY)));
        assert!(!control.admits(None));
        assert!(!control.admits(Some("")));
        assert!(!control.admits(Some("0123456789abcdef0123456789abcdee")));
    }

    #[test]
    fn a_key_installs_once() {
        let control = ControlKey::default();
        assert_eq!(control.install("short"), Install::Malformed);
        assert_eq!(control.install(KEY), Install::Installed);
        assert_eq!(control.install(KEY), Install::Unchanged);
        assert_eq!(
            control.install("fedcba9876543210fedcba9876543210"),
            Install::Conflict
        );
        assert!(control.admits(Some(KEY)));
    }
}
