//! Generated modules declare `required_version >= 1.9.0`, so every expression they emit has to
//! evaluate on Terraform 1.9.
//!
//! Terraform before 1.12 evaluates both sides of `&&` and `||`, so `a && f(b[0])` fails whenever
//! `b` has `count = 0`, even though `a` is false. `terraform validate` only parses, and CI's
//! Terraform is newer, so nothing else catches it: these tests plan the registered EKS module with
//! mocked providers on a Terraform 1.9 binary and fail on the first expression that needs
//! short-circuit evaluation.
//!
//! Point `ALIEN_TERRAFORM_1_9_DIR` at a directory holding a Terraform 1.9 `terraform` binary:
//!
//! ```text
//! curl -fsSLO https://releases.hashicorp.com/terraform/1.9.8/terraform_1.9.8_linux_amd64.zip
//! unzip terraform_1.9.8_linux_amd64.zip -d "$HOME/terraform-1.9.8"
//! ALIEN_TERRAFORM_1_9_DIR="$HOME/terraform-1.9.8" cargo test -p alien-terraform --test generator terraform_1_9
//! ```

use super::helpers::{prepare_for_target, prepare_for_target_with, test_pins, test_utils};
use alien_core::{
    Container, ContainerCode, KubernetesClusterOwnership, KubernetesClusterSettings,
    KubernetesSettings, PermissionProfile, Postgres, ResourceLifecycle, ResourceSpec, Stack,
    StackInputDefinition, StackSettings,
};
use alien_terraform::{
    generate_product_terraform_module, TerraformHelmInstall, TerraformOptions,
    TerraformRegistration, TerraformTarget, TfRegistry,
};
use indexmap::IndexMap;
use std::{path::PathBuf, process::Command};

const BIN_DIR_VARIABLE: &str = "ALIEN_TERRAFORM_1_9_DIR";

/// The directory with the Terraform 1.9 binary, or `None` (after saying why) when the variable
/// is unset, so a plain `cargo test` without that binary skips these tests.
fn terraform_1_9_dir() -> Option<PathBuf> {
    let Some(dir) = std::env::var_os(BIN_DIR_VARIABLE) else {
        eprintln!(
            "skipping: {BIN_DIR_VARIABLE} is not set; point it at a directory holding a \
             Terraform 1.9 binary to run this test (see the module docs)"
        );
        return None;
    };
    let dir = PathBuf::from(dir);
    let version = Command::new(dir.join("terraform"))
        .arg("version")
        .output()
        .expect("the Terraform 1.9 binary should run");
    let version = String::from_utf8_lossy(&version.stdout);
    assert!(
        version.starts_with("Terraform v1.9."),
        "{BIN_DIR_VARIABLE} must hold Terraform 1.9.x, found: {version}"
    );
    Some(dir)
}

const DATABASE_GATE: &str = "databaseEnabled";

/// A workload on EKS, with a database behind a deployer input when `gated_database`. The
/// preflights add the cluster and the network from the deployment's settings.
fn source_stack(gated_database: bool) -> Stack {
    let mut builder = Stack::new("eks-1-9".to_string()).permission("app", PermissionProfile::new());
    if gated_database {
        builder = builder.inputs(vec![StackInputDefinition::deployer_boolean(
            DATABASE_GATE,
            "Enable the database",
            "Whether to create the database.",
            Some(true),
        )]);
    }
    builder = builder.add(
        Container::new("app".to_string())
            .code(ContainerCode::Image {
                image: "example.com/app:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "0.5".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .permissions("app".to_string())
            .build(),
        ResourceLifecycle::Live,
    );
    if gated_database {
        builder = builder.add_enabled_when(
            Postgres::new("db".to_string()).build(),
            ResourceLifecycle::Frozen,
            DATABASE_GATE,
        );
    }
    builder.build()
}

/// The module a registered EKS install ships: registration, provider-rendered Helm install and
/// the Remote Operator lifecycle locals.
fn registered_eks_module(stack: &Stack) -> alien_terraform::ModuleFiles {
    let registry = TfRegistry::built_in();
    generate_product_terraform_module(
        stack,
        TerraformTarget::Eks,
        TerraformOptions {
            setup_identity: None,
            pins: test_pins(),
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: Some(TerraformRegistration {
                provider_name: "acme_app".to_string(),
                provider_source: "pkg.example.com/acme/app".to_string(),
                provider_version: "1.0.0".to_string(),
                resource_type: "deployment".to_string(),
                release_id: Some("rel-test".to_string()),
                setup_target: "kubernetes".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
            }),
            helm_install: Some(TerraformHelmInstall {
                chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
                release_name: "acme-operator".to_string(),
            }),
            supported_aws_regions: Vec::new(),
        },
        false,
    )
    .expect("module should render")
}

/// Remove the block that starts at the first line beginning with `opening`, through the first
/// following line that is just the closing brace at the same indentation.
fn remove_block(text: &str, opening: &str) -> String {
    let mut out = Vec::new();
    let mut closing = None;
    for line in text.lines() {
        match &closing {
            Some(close) if line == close => closing = None,
            Some(_) => {}
            None if line.trim_start().starts_with(opening) => {
                let indent = &line[..line.len() - line.trim_start().len()];
                closing = Some(format!("{indent}}}"));
            }
            None => out.push(line),
        }
    }
    out.join("\n") + "\n"
}

/// Terraform cannot install the registration provider of a test module, so replace it with a
/// local value that has the attributes the module reads. Everything else stays as generated.
fn with_registration_provider_stubbed(
    module: &alien_terraform::ModuleFiles,
) -> IndexMap<String, String> {
    let mut files: IndexMap<String, String> = module
        .iter()
        .map(|(name, body)| (name.to_string(), body.to_string()))
        .collect();
    files.shift_remove("registration.tf");
    let versions = files.get_mut("versions.tf").expect("versions.tf");
    *versions = remove_block(versions, "acme_app = {");
    let providers = files.get_mut("providers.tf").expect("providers.tf");
    *providers = remove_block(providers, "provider \"acme_app\"");
    let helm = files.get_mut("helm.tf").expect("helm.tf");
    *helm = helm
        .replace("    acme_app_deployment.this,\n", "")
        .replace("acme_app_deployment.this.helm_values", "\"{}\"");
    let outputs = files.get_mut("outputs.tf").expect("outputs.tf");
    *outputs = outputs.replace("acme_app_deployment.this.deployment_id", "\"stub\"");
    files
}

/// The ids the manager expects a setup to register: the stack it prepares for the deployment's
/// settings (the cluster and network mutations decide what it carries), filtered by the same
/// predicate it validates a registration with. A declined gated resource is dropped first, as the
/// manager does before it prepares the stack.
async fn manager_expected_ids(
    ownership: KubernetesClusterOwnership,
    database: Option<bool>,
) -> Vec<String> {
    let mut source = source_stack(database.is_some());
    if database == Some(false) {
        source.resources.shift_remove("db");
    }
    let settings = StackSettings {
        kubernetes: Some(KubernetesSettings {
            cluster: Some(KubernetesClusterSettings {
                ownership,
                namespace: None,
                cloud: None,
            }),
            exposure: None,
        }),
        ..StackSettings::default()
    };
    let stack = prepare_for_target_with(source, TerraformTarget::Eks, settings).await;
    let mut ids: Vec<String> = stack
        .resources()
        .filter(|(_, entry)| {
            alien_core::emits_package_scaffolding(
                entry,
                StackSettings::default().setup_management,
                alien_core::Platform::Aws,
            )
        })
        .map(|(id, _)| id.to_string())
        .collect();
    ids.sort();
    ids
}

fn id_set(ids: &[String]) -> String {
    let quoted: Vec<String> = ids.iter().map(|id| format!("{id:?}")).collect();
    format!("toset([{}])", quoted.join(", "))
}

const MOCK_PROVIDERS: &str = r#"mock_provider "aws" {
  mock_data "aws_eks_cluster" {
    defaults = {
      endpoint              = "https://example.eks.amazonaws.com"
      certificate_authority = [{ data = "ZmFrZQ==" }]
      identity              = [{ oidc = [{ issuer = "https://oidc.eks.us-east-1.amazonaws.com/id/ABCDEF" }] }]
      arn                   = "arn:aws:eks:us-east-1:111122223333:cluster/c"
    }
  }
  mock_data "aws_caller_identity" {
    defaults = { account_id = "111122223333", arn = "arn:aws:iam::111122223333:user/x" }
  }
  mock_data "aws_partition" {
    defaults = { partition = "aws" }
  }
  mock_data "aws_region" {
    defaults = { name = "us-east-1" }
  }
  mock_data "aws_availability_zones" {
    defaults = { names = ["us-east-1a", "us-east-1b", "us-east-1c"], zone_ids = ["use1-az1", "use1-az2", "use1-az3"] }
  }
}
mock_provider "kubernetes" {}
mock_provider "helm" {}
mock_provider "tls" {}
mock_provider "random" {}

variables {
  token                   = "t"
  management_url          = "https://manager.example.com"
  kubernetes_cluster_mode = "existing"
  eks_cluster_name        = "c"
}
"#;

/// A plan run that asserts the registration lists exactly `expected`, with `variables` set.
fn registration_run(name: &str, variables: &str, expected: &[String], what: &str) -> String {
    format!(
        r#"
run "{name}" {{
  command = plan
  variables {{
{variables}
  }}
  assert {{
    condition     = toset([for r in local.deployment_resources : r.id]) == {ids}
    error_message = "{what}"
  }}
}}
"#,
        ids = id_set(expected)
    )
}

fn plan_on_terraform_1_9(module: &alien_terraform::ModuleFiles, runs: &str, bin_dir: &PathBuf) {
    let mut files = with_registration_provider_stubbed(module);
    files.insert(
        "plan.tftest.hcl".to_string(),
        format!("{MOCK_PROVIDERS}{runs}"),
    );
    test_utils::terraform_test_with_binary_in(&files, bin_dir)
        .assert_ok("registered EKS module on Terraform 1.9");
}

#[tokio::test]
async fn registered_eks_module_evaluates_on_terraform_1_9() {
    let Some(bin_dir) = terraform_1_9_dir() else {
        return;
    };
    let existing_ids = manager_expected_ids(KubernetesClusterOwnership::Existing, None).await;
    let created_ids = manager_expected_ids(KubernetesClusterOwnership::Managed, None).await;
    assert!(
        existing_ids.contains(&"kubernetes".to_string())
            && !existing_ids.contains(&"default-network".to_string()),
        "the manager prepares no network for an existing cluster: {existing_ids:?}"
    );
    let mut with_network = existing_ids.clone();
    with_network.push("default-network".to_string());
    with_network.sort();
    assert_eq!(
        created_ids, with_network,
        "a created cluster adds exactly the network"
    );

    let stack = prepare_for_target(source_stack(false), TerraformTarget::Eks).await;
    let module = registered_eks_module(&stack);
    let mut runs = String::new();
    // An existing cluster creates no network and registers exactly what the manager expects. The
    // Remote Operator lifecycle inspection is off, so its data sources have count 0.
    runs.push_str(
        r#"
run "existing_cluster" {
  command = plan
  assert {
    condition     = length(aws_vpc.default_network) == 0 && length(aws_nat_gateway.default_network) == 0 && length(aws_eip.default_network_nat) == 0
    error_message = "an existing cluster must not get a default network"
  }
  assert {
    condition     = toset([for r in local.deployment_resources : r.id]) == EXISTING
    error_message = "the registration must list exactly the resources the manager expects for an existing cluster"
  }
}

run "created_cluster" {
  command = plan
  variables {
    kubernetes_cluster_mode = "create"
  }
  assert {
    condition     = length(aws_vpc.default_network) == 1
    error_message = "a created cluster keeps its default network"
  }
  assert {
    condition     = toset([for r in local.deployment_resources : r.id]) == CREATED
    error_message = "the registration must list exactly the resources the manager expects for a created cluster"
  }
}

# The same module with the Remote Operator lifecycle inspection on: its data sources have one
# instance and the locals read them.
run "remote_operator_inspection" {
  command = plan
  variables {
    remote_operator_enabled             = true
    remote_operator_sync_token          = "tok"
    remote_operator_sync_token_revision = 3
    remote_operator_encryption_key      = "0000000000000000000000000000000000000000000000000000000000000000"
  }
  override_data {
    target = data.kubernetes_resources.remote_operator_namespace[0]
    values = { objects = [{ metadata = { name = "production" } }] }
  }
  override_data {
    target = data.kubernetes_resources.remote_operator_identity_records[0]
    values = { objects = [] }
  }
  override_data {
    target = data.kubernetes_resources.remote_operator_credentials[0]
    values = { objects = [{ metadata = { annotations = { "alien.dev/sync-token-revision" = "3" } } }] }
  }
  assert {
    condition     = local.remote_operator_namespace_exists && local.remote_operator_persisted_sync_token_revision == 3
    error_message = "the lifecycle locals must read the inspected namespace and credentials"
  }
}
"#
        .replace("EXISTING", &id_set(&existing_ids))
        .replace("CREATED", &id_set(&created_ids))
        .as_str(),
    );
    plan_on_terraform_1_9(&module, &runs, &bin_dir);
}

/// A database behind a deployer input decides whether the manager still prepares a network for
/// an existing cluster: it drops a declined frozen resource before it prepares the stack.
#[tokio::test]
async fn a_gated_database_decides_the_registered_network_on_terraform_1_9() {
    let Some(bin_dir) = terraform_1_9_dir() else {
        return;
    };
    let declined = manager_expected_ids(KubernetesClusterOwnership::Existing, Some(false)).await;
    let accepted = manager_expected_ids(KubernetesClusterOwnership::Existing, Some(true)).await;
    assert!(
        !declined.contains(&"default-network".to_string()),
        "a declined database leaves an existing cluster without a network: {declined:?}"
    );
    assert!(
        accepted.contains(&"default-network".to_string()) && accepted.contains(&"db".to_string()),
        "the manager prepares a network for an accepted database: {accepted:?}"
    );

    let stack = prepare_for_target(source_stack(true), TerraformTarget::Eks).await;
    let module = registered_eks_module(&stack);
    // The built-in registry has no database emitter, so the module registers every id the manager
    // expects except the database itself.
    let without_database =
        |ids: &[String]| -> Vec<String> { ids.iter().filter(|id| *id != "db").cloned().collect() };
    let mut runs = registration_run(
        "database_declined",
        "    input_database_enabled = false",
        &without_database(&declined),
        "a declined database leaves an existing cluster without a network",
    );
    runs.push_str(&registration_run(
        "database_accepted",
        "    input_database_enabled = true",
        &without_database(&accepted),
        "an accepted database keeps the network the manager prepares for it",
    ));
    plan_on_terraform_1_9(&module, &runs, &bin_dir);
}
