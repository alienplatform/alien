//! The JSON schemas `alien.toml` is read and edited with, generated from alien-core.
//!
//! `schema/canonical.json` is what the resolver walks; `schema/alien.toml.json` is the editor
//! schema. Both are checked in. These tests regenerate them in memory and fail, with the
//! command to run, when alien-core (or the sugar the editor schema describes) has moved on.

mod canonical;

use std::path::PathBuf;

pub fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema")
}

/// Compares `generated` with the checked-in file, or rewrites the file when `UPDATE_SCHEMA=1`.
pub fn check_generated(file: &str, generated: &str) {
    let path = schema_dir().join(file);
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, generated).expect("schema file is writable");
        return;
    }
    let checked_in = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        checked_in == generated,
        "schema/{file} is out of date. Run `UPDATE_SCHEMA=1 cargo test -p alien-stack-toml --test schema` and commit the result."
    );
}
