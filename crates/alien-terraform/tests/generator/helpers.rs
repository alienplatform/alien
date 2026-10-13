//! Shared fixtures + the multi-file snapshot helper.
//!
//! Every test in this directory funnels through `snapshot_module` so the
//! resulting `.snap` file shows every rendered Terraform file with
//! `=== <path> ===` separators. Snapshots stay reviewable as a unit, the
//! way a security team would actually read the module.

use alien_core::{Stack, StackInputDefinition, StackSettings};
use alien_terraform::{
    generate_terraform_module, ModuleFiles, TerraformOptions, TerraformTarget, TfRegistry,
};
use hcl::eval::{Context, Evaluate};
use hcl::expr::{Expression, ObjectKey};
use indexmap::IndexMap;

#[allow(dead_code)]
#[path = "../../src/test_utils.rs"]
pub(super) mod test_utils;

/// A boolean deployer input, the shape `.enabled(input)` gates on.
pub fn gate_input(id: &str, label: &str, description: &str) -> StackInputDefinition {
    StackInputDefinition::deployer_boolean(id, label, description, Some(true))
}

/// HCL re-pads `=` alignment whenever a sibling attribute lands, so assertions
/// have to compare on collapsed whitespace rather than the rendered columns.
pub fn normalized(module: &ModuleFiles) -> String {
    module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `resource "<type>"` headers whose block body carries `gate` (the
/// `count = var.<input> ? 1 : 0` line), read off a `normalized` module. Lets a
/// test assert "every block that must be gated is, and no other" by parsed
/// structure instead of a brittle `starts_with` on the rendered string.
#[allow(dead_code)]
pub fn gated_block_types(main: &str, gate: &str) -> Vec<String> {
    let mut types = Vec::new();
    for (index, _) in main.match_indices("resource \"") {
        let rest = &main[index + "resource \"".len()..];
        let Some((block_type, tail)) = rest.split_once('"') else {
            continue;
        };
        // The block body runs to the next `resource "` header.
        let body_end = tail.find("resource \"").unwrap_or(tail.len());
        if tail[..body_end].contains(gate) {
            types.push(block_type.to_string());
        }
    }
    types
}

/// Every `resource "<type>"` header the module declares, gated or not. Paired
/// with `gated_block_types` so a gate-exclusion test first proves the block it
/// expects to stay ungated is actually rendered.
#[allow(dead_code)]
pub fn declared_block_types(main: &str) -> Vec<String> {
    main.match_indices("resource \"")
        .filter_map(|(index, _)| {
            main[index + "resource \"".len()..]
                .split_once('"')
                .map(|(block_type, _)| block_type.to_string())
        })
        .collect()
}

/// The registration list only changes shape once something in the stack is
/// gated. Everything else must render exactly as it did before this feature.
pub fn assert_ungated_registration_list_is_a_plain_array(main: &str) {
    assert!(
        main.contains("deployment_resources = ["),
        "an ungated stack still renders a plain array:\n{main}"
    );
    assert!(
        !main.contains("concat("),
        "no concat wrapping when nothing is gated:\n{main}"
    );
}

/// Render `stack` for `target` against the built-in registry with the given
/// settings. Panics on render failure so test bodies stay short.
pub fn render(stack: &Stack, target: TerraformTarget, settings: StackSettings) -> ModuleFiles {
    try_render(stack, target, settings).expect("module should render")
}

/// [`render`] without the unwrap, for tests asserting a render refusal.
pub fn try_render(
    stack: &Stack,
    target: TerraformTarget,
    settings: StackSettings,
) -> alien_core::Result<ModuleFiles> {
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
    test_utils::terraform_fmt_check(&files).assert_ok(format!("{context} terraform fmt -check"));
    test_utils::terraform_validate(&files).assert_ok(format!("{context} terraform validate"));
}

/// Run `terraform fmt -check` when provider initialization is intentionally
/// unavailable (for example, a test registry provider with no public binary).
pub fn assert_terraform_formatted(module: &ModuleFiles, context: &str) {
    let files = linter_files(module);
    test_utils::terraform_fmt_check(&files).assert_ok(format!("{context} terraform fmt -check"));
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

/// Evaluate the actual rendered receipt for every combination of installer inputs.
pub fn assert_management_receipt_gates(module: &ModuleFiles, scopes: &[(&str, &str)]) {
    fn find(expression: &Expression) -> Option<&Expression> {
        match expression {
            Expression::Object(fields) => fields.iter().find_map(|(key, value)| {
                let is_receipt = match key {
                    ObjectKey::Identifier(name) => name.as_str() == "installedManagementGrants",
                    ObjectKey::Expression(Expression::String(name)) => {
                        name == "installedManagementGrants"
                    }
                    _ => false,
                };
                if is_receipt {
                    Some(value)
                } else {
                    find(value)
                }
            }),
            Expression::Array(items) => items.iter().find_map(find),
            Expression::FuncCall(call) => call.args.iter().find_map(find),
            Expression::Conditional(condition) => {
                find(&condition.true_expr).or_else(|| find(&condition.false_expr))
            }
            _ => None,
        }
    }
    let receipt = module
        .iter()
        .find_map(|(_, contents)| {
            let body: hcl::Body = hcl::from_str(contents).expect("rendered HCL parses");
            body.blocks()
                .filter(|block| block.identifier() == "locals")
                .flat_map(|block| block.body().attributes())
                .find_map(|attribute| find(attribute.expr()).cloned())
        })
        .expect("management registration receipt exists");
    for mask in 0..(1usize << scopes.len()) {
        let variables = scopes
            .iter()
            .enumerate()
            .map(|(index, (_, input))| {
                (
                    input.to_string(),
                    serde_json::Value::Bool(mask & (1 << index) != 0),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let mut context = Context::new();
        context.declare_var("var", hcl::to_value(variables).expect("input variables"));
        let actual = receipt.evaluate(&context).expect("receipt evaluates");
        let actual = actual.as_object().expect("receipt map");
        for (index, (scope, _)) in scopes.iter().enumerate() {
            let grants = actual
                .get(*scope)
                .expect("scope exists")
                .as_array()
                .expect("grant list");
            assert_eq!(
                !grants.is_empty(),
                mask & (1 << index) != 0,
                "{scope} receipt must reflect its own installer input, mask {mask}"
            );
        }
    }
}
