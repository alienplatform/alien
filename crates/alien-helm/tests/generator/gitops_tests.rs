//! `gitops.enabled` makes a render fail instead of generating credentials.
//!
//! Without cluster access (Argo CD, `helm template`) `lookup` is empty, so a
//! generated encryption key or collector token differs on every render. The
//! GitOps profile refuses those renders; each test renders the real chart.

use super::{helpers::render, test_utils};
use alien_core::{ResourceLifecycle, Stack, StackSettings, Storage};

fn chart_files() -> test_utils::LinterFiles {
    let stack = Stack::new("gitops-app".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    render(&stack, StackSettings::default()).files
}

const EXISTING: &str = "gitops:\n  enabled: true\nmanagement:\n  existingSecret:\n    name: alien-management\nruntime:\n  encryption:\n    existingSecret:\n      name: alien-encryption\n";

#[test]
fn profile_render_with_existing_secrets_generates_nothing() {
    let files = chart_files();
    let rendered = test_utils::helm_template(&files, Some(EXISTING));
    rendered.assert_ok("gitops with existing secrets");
    assert!(
        !rendered.stdout.contains("encryption-key:"),
        "the chart must not render an encryption key Secret entry:\n{}",
        rendered.stdout
    );
    // Rendering twice gives the same manifests: nothing random in the output.
    let again = test_utils::helm_template(&files, Some(EXISTING));
    assert_eq!(rendered.stdout, again.stdout);
}

#[test]
fn profile_refuses_a_generated_encryption_key() {
    let values =
        "gitops:\n  enabled: true\nmanagement:\n  existingSecret:\n    name: alien-management\n";
    let rendered = test_utils::helm_template(&chart_files(), Some(values));
    assert!(!rendered.is_ok(), "{}", rendered.stdout);
    assert!(
        rendered
            .stderr
            .contains("runtime.encryption.existingSecret.name"),
        "{}",
        rendered.stderr
    );
}

#[test]
fn profile_refuses_a_management_token_in_values() {
    let values = "gitops:\n  enabled: true\nmanagement:\n  token: test-credential\nruntime:\n  encryption:\n    existingSecret:\n      name: alien-encryption\n";
    let rendered = test_utils::helm_template(&chart_files(), Some(values));
    assert!(!rendered.is_ok(), "{}", rendered.stdout);
    assert!(
        rendered.stderr.contains("management.existingSecret.name"),
        "{}",
        rendered.stderr
    );
}

#[test]
fn profile_refuses_a_generated_collector_token() {
    let values = format!("{EXISTING}logCollector:\n  enabled: true\n  mode: nodeAgent\n");
    let rendered = test_utils::helm_template(&chart_files(), Some(&values));
    assert!(!rendered.is_ok(), "{}", rendered.stdout);
    assert!(
        rendered.stderr.contains("logCollector.token"),
        "{}",
        rendered.stderr
    );
    let pod_api = format!("{EXISTING}logCollector:\n  enabled: true\n  mode: podApi\n");
    test_utils::helm_template(&chart_files(), Some(&pod_api)).assert_ok("podApi collector");
}

#[test]
fn without_the_profile_a_render_still_generates_credentials() {
    // The default (a Helm CLI install) is unchanged.
    let values = "management:\n  token: test-credential\n";
    test_utils::helm_template(&chart_files(), Some(values)).assert_ok("helm cli install");
}
