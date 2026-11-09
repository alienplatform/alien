//! The process-wide key for digests of secret values.
//!
//! Digests of secrets are stored where the control plane can read them, so they
//! are keyed: a plain hash would let whoever stores one test guesses for a weak
//! secret. The embedding process sets the key once at startup.

use std::sync::OnceLock;

static KEY: OnceLock<[u8; 32]> = OnceLock::new();

/// Sets the key and returns whether this call set it. A key that is already
/// set is left alone.
pub fn set_if_unset(key: [u8; 32]) -> bool {
    KEY.set(key).is_ok()
}

/// The key, if the process has set one.
pub fn get() -> Option<[u8; 32]> {
    KEY.get().copied()
}
