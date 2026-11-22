//! `test-gcp-internet-egress` compiles in the test overrides of the Google Cloud internet
//! switch, and `test-sandbox-agent-image` lets a live test layer an agent built from the tree
//! instead of the released one. A shipped build must never carry either, so no manifest in the
//! workspace may enable them outside `[dev-dependencies]` (where Cargo never builds them into a
//! binary or a library a binary uses). This reads every manifest and fails on any other place a
//! feature is named.

use std::path::{Path, PathBuf};

/// Each test-only feature and the manifest that defines it.
const FEATURES: &[(&str, &str)] = &[
    ("test-gcp-internet-egress", "crates/alien-core/Cargo.toml"),
    ("test-sandbox-agent-image", "crates/alien-build/Cargo.toml"),
];
const FEATURE: &str = FEATURES[0].0;

fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut found = vec![root.join("Cargo.toml")];
    let mut stack = vec![root.join("crates")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable directory") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "target") {
                    continue;
                }
                stack.push(path);
            } else if path.file_name().is_some_and(|name| name == "Cargo.toml") {
                found.push(path);
            }
        }
    }
    found
}

/// Every `(manifest, section, line)` that names `feature` outside a dev-dependency table, save the
/// feature's own definition in `owner`.
fn misuses_of(feature: &str, owner: &str, manifest: &Path, contents: &str) -> Vec<String> {
    let own = manifest.ends_with(owner);
    let mut section = String::new();
    let mut found = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed.to_string();
            continue;
        }
        let code = trimmed.split('#').next().unwrap_or("");
        if !code.contains(feature) {
            continue;
        }
        let dev = section.ends_with("dev-dependencies]");
        let definition = own && section == "[features]" && code.starts_with(feature);
        if !dev && !definition {
            found.push(format!("{} {section}: {trimmed}", manifest.display()));
        }
    }
    found
}

fn misuses(manifest: &Path, contents: &str) -> Vec<String> {
    misuses_of(FEATURES[0].0, FEATURES[0].1, manifest, contents)
}

#[test]
fn the_test_only_features_are_enabled_only_by_dev_dependencies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (feature, owner) in FEATURES {
        let mut all = Vec::new();
        let mut enabled_by_a_test = false;
        for manifest in manifests(&root) {
            let contents = std::fs::read_to_string(&manifest).expect("readable manifest");
            enabled_by_a_test |= contents.contains(feature) && !manifest.ends_with(owner);
            all.extend(misuses_of(feature, owner, &manifest, &contents));
        }
        assert!(
            all.is_empty(),
            "{feature} enabled outside [dev-dependencies]: {all:#?}"
        );
        // The scan read real manifests: the feature is used, by dev-dependencies.
        assert!(enabled_by_a_test, "{feature} is enabled by no test");
    }
}

#[test]
fn the_scan_finds_a_normal_dependency_and_a_default_feature() {
    let manifest = Path::new("crates/example/Cargo.toml");
    let bad = format!(
        "[features]\ndefault = [\"alien-core/{FEATURE}\"]\n\n[dependencies]\nalien-core = {{ workspace = true, features = [\"{FEATURE}\"] }}\n\n[dev-dependencies]\nalien-core = {{ workspace = true, features = [\"{FEATURE}\"] }}\n"
    );
    assert_eq!(misuses(manifest, &bad).len(), 2);
    let target = format!(
        "[target.'cfg(unix)'.dependencies]\nalien-core = {{ workspace = true, features = [\"{FEATURE}\"] }}\n[target.'cfg(unix)'.dev-dependencies]\nalien-core = {{ workspace = true, features = [\"{FEATURE}\"] }}\n"
    );
    assert_eq!(misuses(manifest, &target).len(), 1);
}
