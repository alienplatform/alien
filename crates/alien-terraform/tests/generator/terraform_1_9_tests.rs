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

use super::helpers::{test_pins, test_utils};
use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, KubernetesCluster,
    KubernetesClusterOwnership, KubernetesClusterProvider, KubernetesHeartbeatMode, Network,
    NetworkSettings, Platform, ResourceLifecycle, Stack, StackSettings, StackState,
};
use alien_preflights::{mutations::NetworkMutation, StackMutation};
use alien_terraform::{
    generate_product_terraform_module, TerraformHelmInstall, TerraformOptions,
    TerraformRegistration, TerraformTarget, TfRegistry,
};
use indexmap::IndexMap;
use std::{path::PathBuf, process::Command};

const BIN_DIR_VARIABLE: &str = "ALIEN_TERRAFORM_1_9_DIR";

fn terraform_1_9_dir() -> PathBuf {
    let dir = std::env::var_os(BIN_DIR_VARIABLE).unwrap_or_else(|| {
        panic!("{BIN_DIR_VARIABLE} must name a directory with a Terraform 1.9 binary; see the module docs")
    });
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
    dir
}

fn eks_stack(ownership: KubernetesClusterOwnership, with_network: bool) -> Stack {
    let mut stack = Stack::new("eks-1-9".to_string());
    if with_network {
        stack = stack.add(
            Network::new("default-network".to_string())
                .settings(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        );
    }
    stack
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(ownership)
                .namespace("production".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build()
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

/// The ids the manager expects a setup to register for `stack` under the given cluster
/// ownership: the stack it prepares (the network mutation decides whether it carries a network)
/// filtered by the same predicate it validates a registration with.
async fn manager_expected_ids(ownership: KubernetesClusterOwnership) -> Vec<String> {
    let stack = eks_stack(ownership, false);
    let stack_state = StackState::new(Platform::Kubernetes);
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .base_platform(Platform::Aws)
        .build();
    let stack = if NetworkMutation.should_run(&stack, &stack_state, &config) {
        NetworkMutation
            .mutate(stack, &stack_state, &config)
            .await
            .expect("the network mutation should run")
    } else {
        stack
    };
    let mut ids: Vec<String> = stack
        .resources()
        .filter(|(_, entry)| {
            alien_core::emits_package_scaffolding(
                entry,
                StackSettings::default().setup_management,
                Platform::Aws,
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

#[tokio::test]
async fn registered_eks_module_evaluates_on_terraform_1_9() {
    let existing_ids = manager_expected_ids(KubernetesClusterOwnership::Existing).await;
    let created_ids = manager_expected_ids(KubernetesClusterOwnership::Managed).await;
    assert_eq!(
        existing_ids,
        ["kubernetes"],
        "the manager prepares no network for an existing cluster"
    );
    assert_eq!(created_ids, ["default-network", "kubernetes"]);

    let module = registered_eks_module(&eks_stack(KubernetesClusterOwnership::Managed, true));
    let mut files = with_registration_provider_stubbed(&module);
    files.insert(
        "plan.tftest.hcl".to_string(),
        format!(
            r#"mock_provider "aws" {{
  mock_data "aws_eks_cluster" {{
    defaults = {{
      endpoint              = "https://example.eks.amazonaws.com"
      certificate_authority = [{{ data = "ZmFrZQ==" }}]
      identity              = [{{ oidc = [{{ issuer = "https://oidc.eks.us-east-1.amazonaws.com/id/ABCDEF" }}] }}]
      arn                   = "arn:aws:eks:us-east-1:111122223333:cluster/c"
    }}
  }}
  mock_data "aws_caller_identity" {{
    defaults = {{ account_id = "111122223333", arn = "arn:aws:iam::111122223333:user/x" }}
  }}
  mock_data "aws_partition" {{
    defaults = {{ partition = "aws" }}
  }}
  mock_data "aws_region" {{
    defaults = {{ name = "us-east-1" }}
  }}
  mock_data "aws_availability_zones" {{
    defaults = {{ names = ["us-east-1a", "us-east-1b", "us-east-1c"], zone_ids = ["use1-az1", "use1-az2", "use1-az3"] }}
  }}
}}
mock_provider "kubernetes" {{}}
mock_provider "helm" {{}}
mock_provider "tls" {{}}
mock_provider "random" {{}}

variables {{
  token                   = "t"
  management_url          = "https://manager.example.com"
  kubernetes_cluster_mode = "existing"
  eks_cluster_name        = "c"
}}

# An existing cluster creates no network and registers exactly what the manager expects. The
# Remote Operator lifecycle inspection is off, so its data sources have count 0.
run "existing_cluster" {{
  command = plan
  assert {{
    condition     = length(aws_vpc.default_network) == 0 && length(aws_nat_gateway.default_network) == 0 && length(aws_eip.default_network_nat) == 0
    error_message = "an existing cluster must not get a default network"
  }}
  assert {{
    condition     = toset([for r in local.deployment_resources : r.id]) == {existing}
    error_message = "the registration must list exactly the resources the manager expects for an existing cluster"
  }}
}}

run "created_cluster" {{
  command = plan
  variables {{
    kubernetes_cluster_mode = "create"
  }}
  assert {{
    condition     = length(aws_vpc.default_network) == 1
    error_message = "a created cluster keeps its default network"
  }}
  assert {{
    condition     = toset([for r in local.deployment_resources : r.id]) == {created}
    error_message = "the registration must list exactly the resources the manager expects for a created cluster"
  }}
}}

# The same module with the Remote Operator lifecycle inspection on: its data sources have one
# instance and the locals read them.
run "remote_operator_inspection" {{
  command = plan
  variables {{
    remote_operator_enabled             = true
    remote_operator_sync_token          = "tok"
    remote_operator_sync_token_revision = 3
    remote_operator_encryption_key      = "0000000000000000000000000000000000000000000000000000000000000000"
  }}
  override_data {{
    target = data.kubernetes_resources.remote_operator_namespace[0]
    values = {{ objects = [{{ metadata = {{ name = "production" }} }}] }}
  }}
  override_data {{
    target = data.kubernetes_resources.remote_operator_identity_records[0]
    values = {{ objects = [] }}
  }}
  override_data {{
    target = data.kubernetes_resources.remote_operator_credentials[0]
    values = {{ objects = [{{ metadata = {{ annotations = {{ "alien.dev/sync-token-revision" = "3" }} }} }}] }}
  }}
  assert {{
    condition     = local.remote_operator_namespace_exists && local.remote_operator_persisted_sync_token_revision == 3
    error_message = "the lifecycle locals must read the inspected namespace and credentials"
  }}
}}
"#,
            existing = id_set(&existing_ids),
            created = id_set(&created_ids),
        ),
    );
    test_utils::terraform_test_with_binary_in(&files, &terraform_1_9_dir())
        .assert_ok("registered EKS module on Terraform 1.9");
}
