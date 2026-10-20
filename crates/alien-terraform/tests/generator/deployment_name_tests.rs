//! The `name` variable is the deployment name a self-registering module sends at
//! registration. Registration accepts 2-100 lowercase letters and digits in
//! groups joined by single hyphens, without the reserved `ag-` prefix, so the
//! generated default must be such a name and Terraform must reject any other
//! value at plan time, before the module creates infrastructure.
//!
//! Asserted with real `terraform plan` runs against the generated
//! `variables.tf`, because what matters is what Terraform itself accepts.

use super::helpers::{assert_terraform_variable_plan_invalid_contains, test_utils};
use alien_core::{ResourceLifecycle, Stack, StackSettings, Storage};
use alien_terraform::{
    generate_terraform_module, ModuleFiles, TerraformOptions, TerraformRegistration,
    TerraformTarget, TfRegistry,
};
use indexmap::IndexMap;

const NAME_ERROR: &str = "name must be 2-100 characters of lowercase letters";

fn registered_module(display_name: Option<&str>) -> ModuleFiles {
    let stack = Stack::new("acme-app".to_string())
        .add(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = TfRegistry::built_in();
    generate_terraform_module(
        &stack,
        TerraformTarget::Aws,
        TerraformOptions {
            display_name: display_name.map(str::to_string),
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: Some(TerraformRegistration {
                provider_name: "acme_app".to_string(),
                provider_source: "pkg.example.com/acme/app".to_string(),
                provider_version: "1.0.0".to_string(),
                resource_type: "deployment".to_string(),
                release_id: Some("rel-test".to_string()),
                setup_target: "aws".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
            }),
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("registered module should render")
}

/// The `default` of `variable "name"` as Terraform reads it.
fn name_default(module: &ModuleFiles) -> String {
    let variables: hcl::Body = hcl::from_str(
        module
            .get("variables.tf")
            .expect("variables.tf should render"),
    )
    .expect("variables.tf should parse");
    let name = variables
        .blocks()
        .find(|block| {
            block.identifier() == "variable"
                && block.labels().first().map(|label| label.as_str()) == Some("name")
        })
        .expect("module should declare variable \"name\"");
    match name
        .body()
        .attributes()
        .find(|attribute| attribute.key() == "default")
        .map(|attribute| attribute.expr())
    {
        Some(hcl::Expression::String(default)) => default.clone(),
        other => panic!("variable \"name\" should default to a string, got {other:?}"),
    }
}

/// Plan the generated variable declarations with `vars` and require success.
fn assert_variable_plan_passes(module: &ModuleFiles, context: &str, vars: &[(&str, &str)]) {
    let files = IndexMap::from([(
        "variables.tf".to_string(),
        module
            .get("variables.tf")
            .expect("variables.tf should render")
            .to_string(),
    )]);
    test_utils::terraform_plan_with_vars(&files, vars).assert_ok(context);
}

#[test]
fn registered_module_defaults_name_to_the_display_name_slug() {
    let module = registered_module(Some("Acme Analytics"));

    assert_eq!(name_default(&module), "acme-analytics");
    // `name` is not supplied, so the plan evaluates the default against the
    // module's own validation rule.
    assert_variable_plan_passes(
        &module,
        "registered module with the default name",
        &[
            ("token", "example-token"),
            ("management_url", "https://manager.example.com"),
        ],
    );
    let readme = module.get("README.md").expect("README.md should render");
    assert!(
        readme.contains("Defaults to `acme-analytics`."),
        "the README should document the default deployment name:\n{readme}"
    );
}

#[test]
fn registered_module_truncates_a_long_display_name_on_a_group_boundary() {
    // The 101-character slug would end in "-b"; cutting it to 100 characters
    // must not leave a trailing hyphen.
    let module = registered_module(Some(&format!("{} B", "a".repeat(99))));
    assert_eq!(name_default(&module), "a".repeat(99));
}

#[test]
fn registered_module_falls_back_to_the_stack_id_when_the_display_name_has_no_valid_slug() {
    // Blank, symbol-only, single-character, and reserved-prefix display names.
    for display_name in [None, Some("   "), Some("!!!"), Some("A"), Some("AG Corp")] {
        let module = registered_module(display_name);
        assert_eq!(
            name_default(&module),
            "acme-app",
            "display name {display_name:?} should fall back to the stack id"
        );
    }
}

#[test]
fn name_variable_accepts_only_names_registration_accepts() {
    let module = registered_module(Some("Acme Analytics"));
    let longest = "a".repeat(100);
    for name in ["ab", "acme-prod", "a1-b2-c3", "agent", longest.as_str()] {
        assert_variable_plan_passes(
            &module,
            &format!("name = {name:?}"),
            &[
                ("name", name),
                ("token", "example-token"),
                ("management_url", "https://manager.example.com"),
            ],
        );
    }

    let too_long = "a".repeat(101);
    for name in [
        "Acme Analytics",
        "Acme",
        "a",
        "acme_prod",
        "-acme",
        "acme-",
        "acme--prod",
        "ag-acme",
        too_long.as_str(),
    ] {
        assert_terraform_variable_plan_invalid_contains(
            &module,
            &format!("name = {name:?}"),
            &[
                ("name", name),
                ("token", "example-token"),
                ("management_url", "https://manager.example.com"),
            ],
            NAME_ERROR,
        );
    }
}
