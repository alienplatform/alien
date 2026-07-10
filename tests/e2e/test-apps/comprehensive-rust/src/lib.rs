pub mod error;
pub mod handlers;
pub mod models;

pub use error::{Error, ErrorData, Result};
pub use models::AppState;

/// Sanitize a value for use inside a KV key.
///
/// KV keys only allow `a-z A-Z 0-9 - _ : .`; event identifiers like a cron
/// schedule (`* * * * *`) contain spaces and `*`, so every disallowed
/// character is mapped to `_`. Record and lookup sides must both use this so
/// the keys match.
pub fn sanitize_kv_key_part(part: &str) -> String {
    part.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}
