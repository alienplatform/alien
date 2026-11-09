//! `alien stack expand` prints what `expand` returns, so what it returns must be the stack
//! `load` resolves, written the way the deployment engine reads it.

use std::path::Path;

use alien_core::Stack;

#[test]
fn expand_is_the_loaded_stack_as_json_for_every_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut checked = 0;
    for entry in std::fs::read_dir(&root).expect("fixtures directory is readable") {
        let fixture = entry.expect("fixture entry is readable").path();
        if !fixture.is_dir() {
            continue;
        }
        let file = fixture.join("alien.toml");

        let loaded = alien_stack_toml::load(&file)
            .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        let expanded = alien_stack_toml::expand(&file)
            .unwrap_or_else(|error| panic!("{}: {error}", file.display()));

        let reread: Stack = serde_json::from_value(expanded.clone()).unwrap_or_else(|error| {
            panic!("{}: expanded JSON is not a stack: {error}", file.display())
        });
        assert_eq!(reread, loaded, "{}", file.display());
        assert_eq!(
            serde_json::to_value(&reread).expect("a stack serializes"),
            expanded,
            "{}: writing the stack again changes the JSON",
            file.display()
        );
        checked += 1;
    }
    assert!(
        checked >= 60,
        "expected the whole fixture suite, found {checked}"
    );
}

#[test]
fn expand_reports_an_invalid_file_with_its_position() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("alien.toml");
    std::fs::write(
        &file,
        "name = \"acme\"\n[storage.data]\nversioning = \"yes\"\n",
    )
    .expect("write alien.toml");

    let error = alien_stack_toml::expand(&file).expect_err("a string is not a boolean");

    let message = error.to_string();
    assert!(message.contains("alien.toml:3:"), "{message}");
}
