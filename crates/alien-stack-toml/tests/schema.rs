//! The checked-in JSON schema (for Taplo / Even Better TOML completion) must match the
//! document types. Regenerate it with `ALIEN_UPDATE_SCHEMA=1 cargo test -p alien-stack-toml --test schema`.

use std::path::Path;

#[test]
fn checked_in_schema_is_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/alien.toml.json");
    let generated = serde_json::to_string_pretty(&alien_stack_toml::json_schema()).unwrap() + "\n";
    if std::env::var_os("ALIEN_UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, &generated).unwrap();
    }
    let checked_in = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        checked_in == generated,
        "{} is stale; regenerate it with ALIEN_UPDATE_SCHEMA=1 cargo test -p alien-stack-toml --test schema",
        path.display()
    );
}

#[test]
fn daemon_keys_are_the_daemon_settings_in_the_schema() {
    let schema = alien_stack_toml::json_schema();
    let mut in_schema: Vec<&str> = schema["definitions"]["Daemon"]["properties"]
        .as_object()
        .expect("the schema describes the daemon's settings")
        .keys()
        .map(String::as_str)
        .collect();
    in_schema.sort_unstable();
    let mut listed = alien_stack_toml::DAEMON_KEYS.to_vec();
    listed.sort_unstable();
    assert_eq!(listed, in_schema);
}
