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

/// Render `stack` for `target` against the built-in registry with the given
/// settings. Panics on render failure so test bodies stay short.
pub fn render(stack: &Stack, target: TerraformTarget, settings: StackSettings) -> ModuleFiles {
    let registry = TfRegistry::built_in();
    generate_terraform_module(
        stack,
        target,
        TerraformOptions {
            registry: &registry,
            stack_settings: settings,
            registration: None,
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

/// Run `terraform fmt -check` + `terraform validate` against the rendered
/// module. Pass the test scenario as `context` for diagnostics.
pub fn assert_terraform_valid(module: &ModuleFiles, context: &str) {
    let files = linter_files(module);
    alien_test_kit::linters::terraform_fmt_check(&files)
        .assert_ok(format!("{context} terraform fmt -check"));
    alien_test_kit::linters::terraform_validate(&files)
        .assert_ok(format!("{context} terraform validate"));
}
