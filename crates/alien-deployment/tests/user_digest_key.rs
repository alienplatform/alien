//! The command line keeps its secrets digest key in the user's config directory,
//! so a second run computes the same digests. Each run is this test binary started again.

use std::process::Command;

const CHILD_CONFIG: &str = "USER_DIGEST_KEY_CHILD_CONFIG";
const PREFIX: &str = "DIGEST:";

#[test]
fn child_prints_digest() {
    let Ok(config) = std::env::var(CHILD_CONFIG) else {
        return;
    };
    std::env::set_var("XDG_CONFIG_HOME", &config);
    std::env::set_var("HOME", &config);
    alien_deployment::persisted_digest_key::use_user_secrets_digest_key();
    let digest = alien_deployment::value_digest("dep:one", "env:DB_PASSWORD", "canary").unwrap();
    println!("{PREFIX}{digest}");
}

fn run(config: &str) -> String {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["child_prints_digest", "--exact", "--nocapture"])
        .env(CHILD_CONFIG, config)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix(PREFIX).map(str::to_string))
        .expect("the child printed no digest")
}

#[test]
fn two_runs_of_the_command_line_compute_the_same_digests() {
    let config = tempfile::tempdir().unwrap();
    let config = config.path().to_str().unwrap();
    let first = run(config);
    assert_eq!(first, run(config));

    let elsewhere = tempfile::tempdir().unwrap();
    assert_ne!(
        first,
        run(elsewhere.path().to_str().unwrap()),
        "another user has another key"
    );
    let config_dir = if cfg!(target_os = "macos") {
        std::path::Path::new(config).join("Library/Application Support")
    } else {
        std::path::Path::new(config).to_path_buf()
    };
    assert!(config_dir.join("alien").join("secrets-digest-key").exists());
}
