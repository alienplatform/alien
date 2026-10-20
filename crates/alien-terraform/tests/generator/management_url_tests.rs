//! A self-registering module registers the deployment with `management_url`, so
//! Terraform must refuse to plan without a usable endpoint instead of creating the
//! setup infrastructure and failing at registration. A module without registration
//! only records the value, and must keep planning without it.
//!
//! Asserted with real `terraform plan` runs against the generated `variables.tf`.

use super::helpers::{assert_terraform_variable_plan_invalid_contains, test_utils};
use alien_core::{ResourceLifecycle, Stack, StackSettings, Storage};
use alien_terraform::{
    generate_terraform_module, ModuleFiles, TerraformOptions, TerraformRegistration,
    TerraformTarget, TfRegistry,
};
use indexmap::IndexMap;

const MANAGEMENT_URL: &str = "https://manager.example.com";

fn module(registration: Option<TerraformRegistration>) -> ModuleFiles {
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
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("module should render")
}

fn registered_module() -> ModuleFiles {
    module(Some(TerraformRegistration {
        provider_name: "acme_app".to_string(),
        provider_source: "pkg.example.com/acme/app".to_string(),
        provider_version: "1.0.0".to_string(),
        resource_type: "deployment".to_string(),
        release_id: Some("rel-test".to_string()),
        setup_target: "aws".to_string(),
        setup_fingerprint: "test".to_string(),
        setup_fingerprint_version: 1,
    }))
}

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
fn registered_module_requires_a_management_url() {
    let module = registered_module();

    assert_terraform_variable_plan_invalid_contains(
        &module,
        "registered module without management_url",
        &[("token", "example-token")],
        "\"management_url\" is not set",
    );
    for url in ["", "manager.example.com", "https://"] {
        assert_terraform_variable_plan_invalid_contains(
            &module,
            &format!("management_url = {url:?}"),
            &[("token", "example-token"), ("management_url", url)],
            "management_url must be the http(s) management endpoint",
        );
    }
    for url in [MANAGEMENT_URL, "http://manager.alien.localhost:8080/"] {
        assert_variable_plan_passes(
            &module,
            &format!("management_url = {url:?}"),
            &[("token", "example-token"), ("management_url", url)],
        );
    }

    let readme = module.get("README.md").expect("README.md should render");
    let required = readme
        .split("Required:")
        .nth(1)
        .and_then(|rest| rest.split("\n\n").nth(1))
        .expect("README should list required inputs");
    assert!(
        required.contains("- `management_url`:"),
        "the README should list management_url as required:\n{readme}"
    );
    assert_eq!(
        readme.matches("`management_url`").count(),
        1,
        "the README should not also list management_url as optional:\n{readme}"
    );
}

#[test]
fn unregistered_module_plans_without_a_management_url() {
    let module = module(None);

    assert_variable_plan_passes(
        &module,
        "unregistered module without management_url",
        &[("token", "example-token"), ("name", "acme-prod")],
    );
}
