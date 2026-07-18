//! Shared fixtures + the multi-file snapshot helper.
//!
//! Every test in this directory funnels through `snapshot_module` so the
//! resulting `.snap` file shows every rendered Terraform file with
//! `=== <path> ===` separators. Snapshots stay reviewable as a unit, the
//! way a security team would actually read the module.

use alien_core::{Stack, StackSettings};
use alien_terraform::{
    generate_terraform_module, ModuleFiles, TerraformOptions, TerraformTarget, TfRegistry,
};
use indexmap::IndexMap;

#[allow(dead_code)]
#[path = "../../src/test_utils.rs"]
mod test_utils;

/// Render `stack` for `target` against the built-in registry with the given
/// settings. Panics on render failure so test bodies stay short.
pub fn render(stack: &Stack, target: TerraformTarget, settings: StackSettings) -> ModuleFiles {
    let registry = TfRegistry::built_in();
    generate_terraform_module(
        stack,
        target,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: settings,
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("module should render")
}

/// Convert the rendered module into the `LinterFiles` shape consumed by
/// `terraform fmt -check` / `terraform validate`.
pub fn linter_files(module: &ModuleFiles) -> IndexMap<String, String> {
    module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(path, contents)| (path.to_string(), contents.to_string()))
        .collect()
}

/// Snapshot the entire module as a single string with `=== <path> ===`
/// separators between files. One `.snap` per scenario, fully reviewable.
pub fn snapshot_module(name: &str, module: &ModuleFiles) {
    let mut buf = String::new();
    for (path, contents) in module.iter() {
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

/// Collapse each line's whitespace runs to single spaces across all rendered
/// files. HCL re-pads `=` alignment when sibling attributes change, so tests
/// must assert normalized literals, never exact padding.
pub fn normalize_module_whitespace(module: &ModuleFiles) -> String {
    module
        .iter()
        .flat_map(|(_, contents)| contents.lines())
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run `terraform fmt -check` + `terraform validate` against the rendered
/// module. Pass the test scenario as `context` for diagnostics.
pub fn assert_terraform_valid(module: &ModuleFiles, context: &str) {
    let files = linter_files(module);
    test_utils::terraform_fmt_check(&files).assert_ok(format!("{context} terraform fmt -check"));
    test_utils::terraform_validate(&files).assert_ok(format!("{context} terraform validate"));
}

/// Run Terraform planning against the generated variable declarations and
/// require a specific diagnostic fragment.
pub fn assert_terraform_variable_plan_invalid_contains(
    module: &ModuleFiles,
    context: &str,
    vars: &[(&str, &str)],
    expected: &str,
) {
    let mut files = IndexMap::new();
    files.insert(
        "variables.tf".to_string(),
        module
            .get("variables.tf")
            .expect("variables.tf should render")
            .to_string(),
    );
    test_utils::terraform_fmt_check(&files).assert_ok(format!("{context} terraform fmt -check"));

    let result = test_utils::terraform_plan_with_vars(&files, vars);
    match &result.status {
        test_utils::LinterStatus::Failed(_) => {
            let diagnostics = format!("{}\n{}", result.stdout, result.stderr);
            assert!(
                diagnostics.contains(expected),
                "terraform plan failed for {context}, but diagnostics did not contain {expected:?}\ncommand: {}\nstdout:\n{}\nstderr:\n{}",
                result.command,
                result.stdout,
                result.stderr
            );
        }
        test_utils::LinterStatus::Passed => {
            panic!("terraform plan unexpectedly passed for {context}");
        }
        test_utils::LinterStatus::Skipped(reason) => {
            panic!("terraform plan was skipped for {context}\nreason: {reason}");
        }
    }
}
