//! Plan the shared account with each contributor enabled and disabled.
use super::helpers::{assert_terraform_valid, gate_input, linter_files, render, test_utils};
use alien_core::{
    AzureResourceGroup, AzureStorageAccount, LifecycleRule, ResourceLifecycle, Stack,
    StackSettings, Storage,
};
use alien_terraform::TerraformTarget;
use std::{fs, process::Command};

#[test]
fn shared_account_capabilities_follow_enabled_frozen_and_live_containers() {
    let stack = Stack::new("shared-storage".to_string())
        .inputs(vec![
            gate_input("publicFiles", "Public files", "Create public files"),
            gate_input(
                "versionedFiles",
                "Versioned files",
                "Create versioned files",
            ),
        ])
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureStorageAccount::new("default-storage-account".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("private".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Storage::new("public".to_string())
                .public_read(true)
                .cors_allowed_origins(vec![
                    "https://public.example.test".to_string(),
                    "https://shared.example.test".to_string(),
                ])
                .build(),
            ResourceLifecycle::Frozen,
            "publicFiles",
        )
        .add_enabled_when(
            Storage::new("versioned".to_string())
                .versioning(true)
                .cors_allowed_origins(vec![
                    "https://versioned.example.test".to_string(),
                    "https://shared.example.test".to_string(),
                ])
                .build(),
            ResourceLifecycle::Live,
            "versionedFiles",
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    assert_terraform_valid(&module, "shared account capabilities");
    let mut files = linter_files(&module);
    let mut tests = String::from(
        r#"
mock_provider "azurerm" {}
variables {
  name = "shared-storage"
  token = "test-token"
  azure_subscription_id = "00000000-0000-0000-0000-000000000000"
  azure_resource_group_name = "example-rg"
}
"#,
    );
    for public in [false, true] {
        for versioned in [false, true] {
            tests.push_str(&format!(r#"
run "public_{public}_versioned_{versioned}" {{
  command = plan
  variables {{
    input_public_files = {public}
    input_versioned_files = {versioned}
  }}
  assert {{
    condition = azurerm_storage_account.default_storage_account.allow_nested_items_to_be_public == {public}
    error_message = "The public capability must follow enabled public containers."
  }}
  assert {{
    condition = azurerm_storage_account.default_storage_account.blob_properties[0].versioning_enabled == {versioned}
    error_message = "The setup-owned account must include enabled Live versioning intent."
  }}
  assert {{
    condition = azurerm_storage_container.private.container_access_type == "private" && length(azurerm_storage_container.public) == ({public} ? 1 : 0)
    error_message = "Enabling parent public capability must leave private siblings private."
  }}
  assert {{
    condition = length(azurerm_storage_account.default_storage_account.blob_properties[0].cors_rule) == (({public} || {versioned}) ? 1 : 0)
    error_message = "CORS must be absent with no enabled contributors and otherwise have one shared rule."
  }}
  assert {{
    condition = (!{public} && !{versioned}) ? length(azurerm_storage_account.default_storage_account.blob_properties[0].cors_rule) == 0 : toset(try(azurerm_storage_account.default_storage_account.blob_properties[0].cors_rule[0].allowed_origins, [])) == setunion(toset({public} ? ["https://public.example.test", "https://shared.example.test"] : []), toset({versioned} ? ["https://versioned.example.test", "https://shared.example.test"] : []))
    error_message = "Only enabled contributors may supply origins, including Live containers."
  }}
  assert {{
    condition = alltrue([for rule in azurerm_storage_account.default_storage_account.blob_properties[0].cors_rule : toset(rule.allowed_methods) == toset(["GET", "HEAD"]) && toset(rule.exposed_headers) == toset(["ETag", "Content-Range", "Accept-Ranges"]) && rule.max_age_in_seconds == 3600 && rule.allowed_headers == tolist(["*"])])
    error_message = "Browser range reads need GET/HEAD and the response headers, without enabling writes."
  }}
  assert {{
    condition = !contains([for resource in local.deployment_resources : resource.id], "versioned")
    error_message = "Setup must not create or import the Live container."
  }}
}}
"#));
        }
    }
    files.insert("tests/account.tftest.hcl".to_string(), tests);
    test_utils::terraform_test(&files).assert_ok("shared account enabled contributor plans");
}

#[test]
fn shared_cors_limits_are_checked_after_gating_and_deduplication() {
    for (case, origins, valid) in [
        (
            "duplicates",
            vec!["https://shared.example.test".to_string(); 100],
            true,
        ),
        (
            "long_origin",
            vec![format!("https://{}.example.test", "x".repeat(256))],
            false,
        ),
        (
            "account_budget",
            (0..10)
                .map(|i| format!("https://{i}{}.example.test", "x".repeat(210)))
                .collect(),
            false,
        ),
        (
            "comma",
            vec!["https://one.example.test,https://two.example.test".to_string()],
            false,
        ),
    ] {
        let stack = Stack::new("cors-limits".to_string())
            .inputs(vec![gate_input("files", "Files", "Enable files")])
            .add(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureStorageAccount::new("default-storage-account".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_enabled_when(
                Storage::new("files".to_string())
                    .cors_allowed_origins(origins)
                    .build(),
                ResourceLifecycle::Live,
                "files",
            )
            .build();
        let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
        let mut files = linter_files(&module);
        let expected_failure = if valid {
            ""
        } else {
            "expect_failures = [azurerm_storage_account.default_storage_account]"
        };
        files.insert("tests/limits.tftest.hcl".to_string(), format!(r#"
mock_provider "azurerm" {{}}
variables {{
  name = "cors-limits"
  token = "test-token"
  azure_subscription_id = "00000000-0000-0000-0000-000000000000"
  azure_resource_group_name = "example-rg"
}}
run "disabled" {{
  command = plan
  variables {{ input_files = false }}
  assert {{
    condition = length(azurerm_storage_account.default_storage_account.blob_properties[0].cors_rule) == 0
    error_message = "A disabled contributor must not configure account CORS."
  }}
}}
run "enabled" {{
  command = plan
  variables {{ input_files = true }}
  {expected_failure}
}}
"#));
        test_utils::terraform_test(&files).assert_ok(format!("shared CORS limits: {case}"));
    }
}

fn lifecycle_stack() -> Stack {
    Stack::new("lifecycle".to_string())
        .inputs(vec![gate_input("files", "Files", "Enable files")])
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureStorageAccount::new("default-storage-account".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("archive".to_string())
                .lifecycle_rules(vec![LifecycleRule {
                    days: 30,
                    prefix: None,
                }])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Storage::new("scratch".to_string())
                .lifecycle_rules(vec![LifecycleRule {
                    days: 1,
                    prefix: Some("tmp/".to_string()),
                }])
                .build(),
            ResourceLifecycle::Live,
            "files",
        )
        .build()
}

#[test]
fn lifecycle_policy_is_shared_scoped_and_live_expiry_requires_opt_in() {
    let module = render(
        &lifecycle_stack(),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    assert_terraform_valid(&module, "singleton lifecycle policy");
    let mut files = linter_files(&module);
    let mut tests = String::from(
        r#"
mock_provider "azurerm" {}
variables {
  name = "lifecycle"
  resource_prefix = "lifecycle-test"
  token = "test-token"
  azure_subscription_id = "00000000-0000-0000-0000-000000000000"
  azure_resource_group_name = "example-rg"
}
"#,
    );
    for gate in [false, true] {
        for live in [false, true] {
            for enabled in [false, true] {
                tests.push_str(&format!(r#"
run "gate_{gate}_live_{live}_enabled_{enabled}" {{
  command = plan
  variables {{
    input_files = {gate}
    azure_storage_live_lifecycle_enabled = {live}
    azure_storage_lifecycle_enabled = {enabled}
  }}
  assert {{
    condition = length(azurerm_storage_management_policy.default_storage_account.rule) == 2
    error_message = "One policy must keep both container rules, including disabled rules."
  }}
  assert {{
    condition = alltrue([for r in azurerm_storage_management_policy.default_storage_account.rule : r.name == "archive-rule-1" ? r.enabled == {enabled} && toset(r.filters[0].prefix_match) == toset(["${{local.resource_prefix}}-archive/"]) && r.actions[0].base_blob[0].delete_after_days_since_modification_greater_than == 30 : r.name == "scratch-rule-1" && r.enabled == ({gate} && {live} && {enabled}) && toset(r.filters[0].prefix_match) == toset(["${{local.resource_prefix}}-scratch/tmp/"]) && r.actions[0].base_blob[0].delete_after_days_since_modification_greater_than == 1])
    error_message = "Expiry must remain container-scoped and require both contributor and Live activation gates."
  }}
}}
"#));
            }
        }
    }
    files.insert("tests/lifecycle.tftest.hcl".to_string(), tests);
    test_utils::terraform_test(&files).assert_ok("singleton lifecycle activation plans");
}

#[test]
fn native_azure_provider_imports_policy_without_deleting_aliases() {
    let module = render(
        &lifecycle_stack(),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let body: hcl::Body = hcl::from_str(module.get("default_storage_account.tf").unwrap()).unwrap();
    let mut policy = hcl::Body::builder();
    for block in body.blocks() {
        if matches!(block.identifier.as_str(), "removed" | "import")
            || (block.identifier.as_str() == "resource"
                && block.labels.first()
                    == Some(&hcl::structure::BlockLabel::String(
                        "azurerm_storage_management_policy".to_string(),
                    )))
        {
            policy = policy.add_block(block.clone());
        }
    }
    let root = tempfile::tempdir().unwrap();
    let mut versions: hcl::Body = hcl::from_str(module.get("versions.tf").unwrap()).unwrap();
    for block in versions.blocks_mut() {
        block.body.0.retain(|item| matches!(item, hcl::Structure::Attribute(attribute) if attribute.key.as_str() == "required_version"));
    }
    fs::write(
        root.path().join("versions.tf"),
        hcl::to_string(&versions).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("generated-policy.txt"),
        hcl::to_string(&policy.build())
            .unwrap()
            .replace(
                "azurerm_storage_account.default_storage_account.id",
                "local.account_id",
            )
            .replace(
                "azurerm_resource_group.default_resource_group",
                "terraform_data.retained",
            ),
    )
    .unwrap();
    let output = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/azure-storage-policy-migration.py"
        ))
        .arg(root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
