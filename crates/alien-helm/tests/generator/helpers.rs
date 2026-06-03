//! Shared Helm generator-test fixtures.

use alien_core::{Stack, StackSettings};
use alien_helm::{generate_helm_chart, HelmChart, HelmOptions, HelmRegistry};
use indexmap::IndexMap;

use super::test_utils;

/// Render `stack` into a chart through the built-in registry.
pub fn render(stack: &Stack, settings: StackSettings) -> HelmChart {
    let registry = HelmRegistry::built_in();
    generate_helm_chart(
        stack,
        HelmOptions {
            registry: &registry,
            stack_settings: settings,
            chart_name: stack.id().to_string(),
        },
    )
    .expect("chart should render")
}

/// Snapshot the entire chart as a single string with `=== <path> ===`
/// separators between files. One `.snap` per scenario.
pub fn snapshot_chart(name: &str, chart: &HelmChart) {
    let mut buf = String::new();
    for (path, contents) in &chart.files {
        buf.push_str("=== ");
        buf.push_str(path);
        buf.push_str(" ===\n");
        buf.push_str(contents);
        if !contents.ends_with('\n') {
            buf.push('\n');
        }
        buf.push('\n');
    }
    insta::assert_snapshot!(name, buf);
}

/// Minimal `management` block that satisfies the chart's `required`
/// guardrails on the manager-fetch path. Templates abort without these.
pub const MANAGER_FETCH_VALUES: &str = r#"
management:
  url: "https://manager.example.com"
  name: "test-manager"
  token: "test-sync-token"
  deploymentId: "test-deployment-id"
"#;

/// Patch the chart's default `values.yaml` so the manager-fetch
/// `required` guardrails are satisfied. Used by tests that drive the
/// chart with values.yaml content as the starting point.
pub fn patch_values_with_manager_fetch(values_yaml: &str) -> String {
    values_yaml
        .replace("  token: \"\"", "  token: \"test-sync-token\"")
        .replace("  name: \"\"", "  name: \"test-manager\"")
        .replace("  url: \"\"", "  url: \"https://manager.example.com\"")
}

/// Run `helm lint` + `helm template` + `kubeconform` against the chart
/// for the default values and every generated example values file.
pub fn assert_helm_valid(chart: &HelmChart, context: &str) {
    let files = linter_files(chart);
    test_utils::helm_lint(&files).assert_ok(format!("{context} helm lint"));
    test_utils::helm_template_and_validate(&files, Some(MANAGER_FETCH_VALUES))
        .assert_ok(format!("{context} helm template manager-fetch default values"));

    for (path, values) in files
        .iter()
        .filter(|(path, _)| path.starts_with("examples/") && path.ends_with(".yaml"))
    {
        test_utils::helm_template_and_validate(&files, Some(values))
            .assert_ok(format!("{context} helm template {path}"));
    }
}

fn linter_files(chart: &HelmChart) -> IndexMap<String, String> {
    chart.files.clone()
}
