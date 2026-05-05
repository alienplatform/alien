//! Shared Helm generator-test fixtures.

use alien_core::{Stack, StackSettings};
use alien_helm::{generate_helm_chart, HelmChart, HelmOptions, HelmRegistry};
use indexmap::IndexMap;

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

/// Run `helm lint` + `helm template` + `kubeconform` against the chart
/// for both bootstrap paths (manager-fetch + external-bindings initialize).
pub fn assert_helm_valid(chart: &HelmChart, context: &str) {
    let files = linter_files(chart);
    alien_test_kit::linters::helm_lint(&files).assert_ok(format!("{context} helm lint"));
    alien_test_kit::linters::helm_template_and_validate(&files, None)
        .assert_ok(format!("{context} helm template manager-fetch"));
    if let Some(local_values) = files.get("examples/onprem.yaml") {
        alien_test_kit::linters::helm_template_and_validate(&files, Some(local_values)).assert_ok(
            format!("{context} helm template external-bindings initialize"),
        );
    }
}

fn linter_files(chart: &HelmChart) -> IndexMap<String, String> {
    chart.files.clone()
}
