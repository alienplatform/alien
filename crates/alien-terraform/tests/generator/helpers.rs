//! Shared fixtures + the multi-file snapshot helper.
//!
//! Every test in this directory funnels through `snapshot_module` so the
//! resulting `.snap` file shows every rendered Terraform file with
//! `=== <path> ===` separators. Snapshots stay reviewable as a unit, the
//! way a security team would actually read the module.

use alien_core::{
    AwsManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    ManagementConfig, Stack, StackInputDefinition, StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;
use alien_terraform::{
    generate_terraform_module, ModuleFiles, TerraformOptions, TerraformTarget, TfRegistry,
};
use indexmap::IndexMap;

#[allow(dead_code)]
#[path = "../../src/test_utils.rs"]
pub(super) mod test_utils;

/// The stack as `alien render --target aws` prepares it: template preflights, then mutations
/// against a configuration without a deployment.
pub async fn prepare_for_render(stack: Stack) -> Stack {
    prepare_for_target(stack, TerraformTarget::Aws).await
}

/// The stack as `alien render --format terraform --target <target>` prepares it, on the target's
/// deployment platform: Kubernetes, on its cloud, for a managed Kubernetes target.
pub async fn prepare_for_target(mut stack: Stack, target: TerraformTarget) -> Stack {
    let platform = target.deployment_platform();
    alien_core::remove_off_platform_gated_resources(&mut stack, platform, None);
    let runner = PreflightRunner::new();
    runner
        .run_template_preflights(&stack, platform)
        .await
        .expect("template preflights pass");
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "rendered-from-template-parameter".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .maybe_base_platform(target.base_platform())
        .build();
    runner
        .apply_mutations(stack, &StackState::new(platform), &config)
        .await
        .expect("template mutations apply")
}

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
    render_with_registry(stack, target, settings, &TfRegistry::built_in())
}

/// [`try_render`] against a registry with extension emitters registered.
pub fn render_with_registry(
    stack: &Stack,
    target: TerraformTarget,
    settings: StackSettings,
    registry: &TfRegistry,
) -> alien_core::Result<ModuleFiles> {
    let module = generate_terraform_module(
        stack,
        target,
        TerraformOptions {
            display_name: None,
            registry,
            stack_settings: settings,
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )?;
    if target.cloud_platform() == alien_core::Platform::Aws {
        assert_leaves_runtime_role_policies_alone(&module);
    }
    Ok(module)
}

/// The runtime writes its own inline policies on these roles, under names Terraform never
/// declares. Terraform leaves them alone only while nothing in the module claims a role's whole
/// set of inline or attached policies.
pub fn assert_leaves_runtime_role_policies_alone(module: &ModuleFiles) {
    for (path, contents) in module.iter().filter(|(path, _)| path.ends_with(".tf")) {
        let body = hcl::parse(contents).unwrap_or_else(|error| panic!("{path} parses: {error}"));
        for block in body
            .blocks()
            .filter(|block| block.identifier() == "resource")
        {
            let kind = block.labels()[0].as_str();
            assert!(
                !matches!(
                    kind,
                    "aws_iam_role_policies_exclusive" | "aws_iam_role_policy_attachments_exclusive"
                ),
                "{path}: {kind} would remove the runtime's policies"
            );
            if kind == "aws_iam_role" {
                let label = block.labels()[1].as_str();
                assert!(
                    !block
                        .body()
                        .blocks()
                        .any(|nested| nested.identifier() == "inline_policy"),
                    "{path}: aws_iam_role.{label} declares its inline policies exclusively"
                );
                assert!(
                    !block
                        .body()
                        .attributes()
                        .any(|attribute| attribute.key() == "managed_policy_arns"),
                    "{path}: aws_iam_role.{label} declares its attached policies exclusively"
                );
            }
        }
    }
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

/// The document every former inline grant keeps once its permissions move to managed policies.
pub const INERT_POLICY: &str =
    r#"{"Version":"2012-10-17","Statement":[{"Effect":"Deny","NotAction":"*","Resource":"*"}]}"#;

/// The documents that give an AWS role its setup permissions, as `(address, argument of the
/// jsonencode it is written as)`: its inline policies and the managed policies attached to it.
/// Inert stubs grant nothing, so they are left out.
pub fn role_policy_documents(
    module: &ModuleFiles,
    role_label: &str,
) -> Vec<(String, hcl::Expression)> {
    let blocks: Vec<hcl::Block> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .flat_map(|(path, contents)| {
            hcl::parse(contents)
                .unwrap_or_else(|error| panic!("{path} parses: {error}"))
                .into_blocks()
                .filter(|block| block.identifier() == "resource")
        })
        .collect();
    let attribute = |block: &hcl::Block, key: &str| {
        block
            .body()
            .attributes()
            .find(|attribute| attribute.key() == key)
            .map(|attribute| attribute.expr().to_string())
    };
    let names_role = |expression: Option<String>, attribute_name: &str| {
        expression.is_some_and(|expression| {
            expression == format!("aws_iam_role.{role_label}.{attribute_name}")
                || expression == format!("aws_iam_role.{role_label}[0].{attribute_name}")
        })
    };
    let jsonencoded = |block: &hcl::Block, address: &str| {
        let policy = block
            .body()
            .attributes()
            .find(|attribute| attribute.key() == "policy")
            .unwrap_or_else(|| panic!("{address} has a policy"))
            .expr()
            .clone();
        match policy {
            hcl::Expression::FuncCall(call) if call.name.name.as_str() == "jsonencode" => {
                Some(call.args[0].clone())
            }
            hcl::Expression::String(text) => {
                assert_eq!(
                    text, INERT_POLICY,
                    "{address}: a literal policy is the inert stub"
                );
                None
            }
            other => panic!("{address}: unexpected policy {other}"),
        }
    };
    let address = |block: &hcl::Block| {
        format!(
            "{}.{}",
            block.labels()[0].as_str(),
            block.labels()[1].as_str()
        )
    };
    let mut documents = Vec::new();
    for block in &blocks {
        let kind = block.labels()[0].as_str();
        if kind == "aws_iam_role_policy" && names_role(attribute(block, "role"), "id") {
            let address = address(block);
            if let Some(document) = jsonencoded(block, &address) {
                documents.push((address, document));
            }
        }
        if kind == "aws_iam_role_policy_attachment" && names_role(attribute(block, "role"), "name")
        {
            let arn = attribute(block, "policy_arn").expect("an attachment names its policy");
            let Some(label) = arn
                .strip_prefix("aws_iam_policy.")
                .and_then(|rest| rest.strip_suffix(".arn"))
                .map(|label| label.trim_end_matches("[0]"))
            else {
                // AWS-managed policies are attached by literal ARN.
                continue;
            };
            let policy = blocks
                .iter()
                .find(|candidate| {
                    candidate.labels()[0].as_str() == "aws_iam_policy"
                        && candidate.labels()[1].as_str() == label
                })
                .unwrap_or_else(|| panic!("aws_iam_policy.{label} is in the module"));
            let address = address(policy);
            let document = jsonencoded(policy, &address)
                .unwrap_or_else(|| panic!("{address}: a managed policy is never a stub"));
            documents.push((address, document));
        }
    }
    documents
}

/// Each (effect, action, resource, condition) the evaluated documents allow or deny, one entry
/// per action and resource, so documents that merge or split statements differently compare
/// equal exactly when they grant and deny the same.
pub fn statement_grants(documents: &[serde_json::Value]) -> std::collections::BTreeSet<String> {
    let as_list = |value: &serde_json::Value| match value {
        serde_json::Value::Null => Vec::new(),
        serde_json::Value::Array(items) => items.clone(),
        other => vec![other.clone()],
    };
    let mut grants = std::collections::BTreeSet::new();
    for document in documents {
        for statement in as_list(&document["Statement"]) {
            // An exclusion list means one thing as a whole, so it stays one entry.
            let whole = |values: &serde_json::Value| {
                let mut values = as_list(values);
                values.sort_by_key(|value| value.to_string());
                vec![serde_json::Value::Array(values)]
            };
            let (action_key, actions) = match statement.get("NotAction") {
                Some(actions) => ("NotAction", whole(actions)),
                None => ("Action", as_list(&statement["Action"])),
            };
            let (resource_key, resources) = match statement.get("NotResource") {
                Some(resources) => ("NotResource", whole(resources)),
                None => ("Resource", as_list(&statement["Resource"])),
            };
            for action in &actions {
                for resource in &resources {
                    grants.insert(
                        serde_json::json!({
                            "Effect": statement["Effect"],
                            action_key: action,
                            resource_key: resource,
                            "Condition": statement.get("Condition"),
                        })
                        .to_string(),
                    );
                }
            }
        }
    }
    grants
}
