//! Plan generated modules with mocked providers; a mismatch must stop planning.
use alien_core::{SetupManagement, Stack, StackSettings};
use alien_terraform::TerraformTarget;

use super::helpers::{linter_files, render, test_utils};

#[test]
fn manual_setup_management_must_match_the_generated_module() {
    assert_setup_management_plans(SetupManagement::Manual, "manual", "automatic");
}

#[test]
fn automatic_setup_management_must_match_the_generated_module() {
    assert_setup_management_plans(SetupManagement::Automatic, "automatic", "manual");
}

fn assert_setup_management_plans(mode: SetupManagement, name: &str, other: &str) {
    let module = render(
        &Stack::new("setup-management".into()).build(),
        TerraformTarget::Aws,
        StackSettings {
            setup_management: mode,
            ..Default::default()
        },
    );
    let mut files = linter_files(&module);
    files.insert(
        "setup-management.tftest.hcl".into(),
        format!(
            r#"
mock_provider "aws" {{}}
mock_provider "random" {{}}
variables {{
  name = "setup-management"
  token = "test"
  resource_prefix = "setup-management"
}}
run "empty_overlay" {{
  command = plan
  assert {{
    condition = try(local.deployment_settings.setupManagement, "manual") == "{name}"
    error_message = "An empty overlay must preserve the generated setup management."
  }}
}}
run "matching_overlay" {{
  command = plan
  variables {{
    advanced_settings_overlay_json = jsonencode({{ setupManagement = "{name}" }})
  }}
  assert {{
    condition = local.deployment_settings.setupManagement == "{name}"
    error_message = "Matching setup management must reach registration unchanged."
  }}
}}
run "mismatched_overlay" {{
  command = plan
  variables {{
    advanced_settings_overlay_json = jsonencode({{ setupManagement = "{other}" }})
  }}
  expect_failures = [terraform_data.deployment_registration]
}}
"#
        ),
    );
    test_utils::terraform_test(&files).assert_ok(format!("{name} setup management plans"));

    // Assert the actual diagnostic too: expect_failures alone could accept an
    // unrelated precondition failure on the same registration resource.
    files.insert(
        "setup-management.tftest.hcl".into(),
        format!(
            r#"
mock_provider "aws" {{}}
mock_provider "random" {{}}
run "wrong_artifact" {{
  command = plan
  variables {{
    name = "setup-management"
    token = "test"
    resource_prefix = "setup-management"
    advanced_settings_overlay_json = jsonencode({{ setupManagement = "{other}" }})
  }}
}}
"#
        ),
    );
    let result = test_utils::terraform_test(&files);
    assert!(matches!(result.status, test_utils::LinterStatus::Failed(_)));
    let diagnostic = format!("{}\n{}", result.stdout, result.stderr);
    assert!(
        diagnostic.contains(&format!("built for {name} setup")),
        "{diagnostic}"
    );
    let normalized = diagnostic.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        normalized.contains(&format!(
            "settings request {other} setup. Regenerate the setup"
        )),
        "{diagnostic}"
    );
    assert!(
        !diagnostic.contains("Error message refers to sensitive values"),
        "{diagnostic}"
    );
}
