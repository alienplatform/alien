//! Terraform module `README.md` generation.

use super::providers::{has_dynamic_aws_network_settings, has_dynamic_gcp_network_settings};
use super::variables::terraform_stack_input_variable_name;
use super::{TerraformHelmInstall, TerraformRegistration};
use crate::target::TerraformTarget;
use alien_core::{Stack, StackInputDefinition, StackSettings};

pub(super) fn readme_md(
    stack: &Stack,
    target: TerraformTarget,
    registration: Option<&TerraformRegistration>,
    display_name: Option<&str>,
    stack_settings: &StackSettings,
    helm_install: Option<&TerraformHelmInstall>,
    stack_inputs: &[StackInputDefinition],
) -> String {
    let required_env = if registration.is_some() {
        "export TF_VAR_token=\"...\"".to_string()
    } else {
        format!(
            "export TF_VAR_name=\"{}\"\nexport TF_VAR_token=\"...\"",
            stack.id()
        )
    };
    let registration_note = registration
        .map(|_| {
            "Terraform registers the deployment after the setup resources are ready. The registration step consumes `local.deployment_management_config`, `local.deployment_settings`, and `local.deployment_resources`; keep those values intact if your organization wraps this module.\n".to_string()
        })
        .unwrap_or_else(|| {
            "This module exposes `deployment_management_config`, `deployment_stack_settings`, and `deployment_resources` outputs for registration flows managed outside Terraform.\n".to_string()
        });

    let display_name = display_name.unwrap_or_else(|| stack.id());
    let mut input_sections = vec![readme_required_inputs(registration.is_some())];
    input_sections.push(readme_common_inputs());
    if matches!(target.cloud_platform(), alien_core::Platform::Aws) {
        input_sections.push(readme_aws_inputs());
    }
    if matches!(target.cloud_platform(), alien_core::Platform::Gcp) {
        input_sections.push(readme_gcp_inputs());
    }
    if matches!(target.cloud_platform(), alien_core::Platform::Azure) {
        input_sections.push(readme_azure_inputs(target));
    }
    if has_dynamic_aws_network_settings(stack_settings.network.as_ref())
        || has_dynamic_gcp_network_settings(stack_settings.network.as_ref())
    {
        input_sections.push(readme_network_inputs(target));
    }
    if target.is_kubernetes() {
        input_sections.push(readme_kubernetes_inputs(
            target,
            registration.is_some(),
            helm_install,
        ));
    }
    if !stack_inputs.is_empty() {
        input_sections.push(readme_stack_inputs(stack_inputs));
    }
    let kubernetes_operations = target
        .is_kubernetes()
        .then(|| readme_kubernetes_operations(target))
        .unwrap_or_default();
    let inputs = input_sections.join("\n\n");
    format!(
        "# Deployment setup - {display_name}\n\n\
Target: `{target}`.\n\n\
This module creates setup-owned infrastructure, grants the management access needed after setup, and prepares deployment registration metadata. Review the generated `.tf` files before applying; each resource file maps to one setup resource.\n\n\
## Inputs\n\n\
{inputs}\n\n\
## Run\n\n\
Use your organization's normal backend and approval workflow. A typical local review looks like:\n\n\
```bash\n{required_env}\nterraform init\nterraform validate\nterraform plan -out=tfplan\nterraform apply tfplan\n```\n\n\
## Registration\n\n\
{registration_note}\n\
## Outputs\n\n\
- `deployment_management_config`: management endpoint and credential-boundary metadata.\n\
- `deployment_stack_settings`: deployment settings JSON assembled from typed variables, package defaults, and advanced-setting overlays.\n\
- `deployment_resources`: setup-owned resource metadata handed to the deployment runtime.\n\
- `deployment_id` and `deployment_token`: emitted only when Terraform performs registration.\
{kubernetes_operations}",
        display_name = display_name,
        target = target.name(),
        inputs = inputs,
        required_env = required_env,
        registration_note = registration_note,
        kubernetes_operations = kubernetes_operations
    )
}

fn readme_required_inputs(has_registration: bool) -> String {
    let name = if has_registration {
        "- `name`: optional display name. Defaults to the package name."
    } else {
        "- `name`: deployment name to include in the registration metadata."
    };
    format!("Required:\n\n- `token`: install token from the setup page.\n{name}")
}

fn readme_common_inputs() -> String {
    "Common optional settings:\n\n- `resource_prefix`: stable physical-name prefix. Leave empty to generate one.\n- `management_url`: optional management endpoint used by pull-style runtimes.\n- `deployment_model`: `push` or `pull`.\n- `updates_mode`: `auto` or `approval-required`.\n- `telemetry_mode`: `off`, `auto`, or `approval-required`.\n- `heartbeats_mode`: `off` or `on`.\n- `advanced_settings_json`: complete advanced deployment settings JSON. Most installs should keep the generated default.\n- `advanced_settings_overlay_json`: partial advanced settings merged over package defaults, preserving generated values such as compute selections.".to_string()
}

fn readme_stack_inputs(inputs: &[StackInputDefinition]) -> String {
    let mut lines = vec!["Application inputs:".to_string()];
    for input in inputs {
        let required = if input.required {
            "required"
        } else {
            "optional"
        };
        lines.push(format!(
            "- `{}`: {} ({required}). {}",
            terraform_stack_input_variable_name(input),
            input.label,
            input.description
        ));
    }
    lines.join("\n")
}

fn readme_aws_inputs() -> String {
    "AWS settings:\n\n- `aws_region`: AWS region used by the provider.\n- `managing_role_arn`: management identity allowed to assume setup-created roles.\n- `managing_account_id`: account that hosts application container images. Empty disables scoped cross-account image-pull grants.".to_string()
}

fn readme_gcp_inputs() -> String {
    "GCP settings:\n\n- `gcp_project`: target GCP project ID.\n- `gcp_region`: target GCP region.\n- `managing_service_account_email`: management service account allowed to impersonate setup-created identities.\n- `gcp_manage_custom_roles`: whether this module creates project custom roles.\n- `gcp_custom_role_prefix`: custom role ID prefix when roles are managed outside this module.".to_string()
}

fn readme_azure_inputs(target: TerraformTarget) -> String {
    let tenant = if target == TerraformTarget::Aks {
        "\n- `azure_tenant_id`: tenant ID for target AKS Kubernetes API identities."
    } else {
        ""
    };
    format!(
        "Azure settings:\n\n- `azure_subscription_id`: target subscription ID.\n- `azure_location`: Azure location.\n- `azure_resource_group_name`: target resource group name.{tenant}\n- `azure_managing_tenant_id`, `azure_oidc_issuer`, `azure_oidc_subject`: management identity trust settings when this setup grants Azure management access."
    )
}

fn readme_network_inputs(target: TerraformTarget) -> String {
    match target.cloud_platform() {
        alien_core::Platform::Aws => "Network settings:\n\n- `network_mode`: `create-new`, `use-existing`, or `use-default`.\n- `vpc_cidr`, `availability_zones`: used with `create-new`.\n- `vpc_id`, `public_subnet_ids`, `private_subnet_ids`, `security_group_ids`: required with `use-existing`.".to_string(),
        alien_core::Platform::Gcp => "Network settings:\n\n- `network_mode`: `create-new`, `use-existing`, or `use-default`.\n- `network_cidr`, `availability_zones`: used with `create-new`.\n- `network_name`, `subnet_name`, `network_region`: required with `use-existing`.".to_string(),
        _ => String::new(),
    }
}

fn readme_kubernetes_inputs(
    target: TerraformTarget,
    has_registration: bool,
    helm_install: Option<&TerraformHelmInstall>,
) -> String {
    let cluster_name = match target {
        TerraformTarget::Eks => "\n- `eks_cluster_name`: existing EKS cluster name when `kubernetes_cluster_mode = \"existing\"`.",
        TerraformTarget::Gke => "\n- `gke_cluster_name`, `gke_cluster_location`: existing GKE cluster when `kubernetes_cluster_mode = \"existing\"`.",
        TerraformTarget::Aks => "\n- `aks_cluster_name`, `aks_cluster_resource_group_name`: existing AKS cluster when `kubernetes_cluster_mode = \"existing\"`.",
        _ => "",
    };
    let helm = if has_registration && helm_install.is_some() {
        "\n- `helm_install_enabled`: set to `false` to use Terraform only for infrastructure and install the Helm chart separately.\n- `helm_release_name`, `helm_chart`: Helm release and chart reference used when Terraform installs the Operator chart. On `terraform destroy`, Terraform uninstalls this Helm release before removing the setup registration."
    } else {
        ""
    };
    let exposure = if target == TerraformTarget::Eks {
        "\n- `custom_domain_name`, `custom_domain_certificate_arn`: optional EKS public route hostname and ACM certificate ARN. Leave empty to use the generated load balancer hostname."
    } else {
        ""
    };
    format!(
        "Kubernetes settings:\n\n- `kubernetes_cluster_mode`: `create` or `existing`.\n- `kubernetes_namespace`: namespace for runtime resources.{cluster_name}{exposure}{helm}"
    )
}

fn readme_kubernetes_operations(target: TerraformTarget) -> String {
    match target {
        TerraformTarget::Eks => format!(
            "{}{}",
            "\n\n## Kubernetes Operations\n\nBefore inspecting the cluster, verify that your AWS CLI points at the target account, not the management account:\n\n```bash\nAWS_PROFILE=<target-profile> aws sts get-caller-identity\nterraform output kubernetes_update_kubeconfig_command\nAWS_PROFILE=<target-profile> aws eks update-kubeconfig --region $(terraform output -raw deployment_region) --name $(terraform output -raw kubernetes_kube_context) --alias $(terraform output -raw kubernetes_kube_context)\nkubectl --context $(terraform output -raw kubernetes_kube_context) -n $(terraform output -raw kubernetes_namespace) get pods,pvc,svc,ingress,events\n```\n\nTreat live `kubectl patch` changes as diagnostics only. Durable fixes belong in the generated package, Helm values, or deployment configuration.",
            readme_kubernetes_destroy_order(),
        ),
        TerraformTarget::Gke | TerraformTarget::Aks => readme_kubernetes_destroy_order().to_string(),
        _ => String::new(),
    }
}

fn readme_kubernetes_destroy_order() -> &'static str {
    "\n\n## Destroy Order\n\nIf `helm_install_enabled = true`, `terraform destroy` uninstalls the Operator Helm release first. The chart's pre-delete cleanup job removes runtime Kubernetes objects, then Terraform removes the setup registration and infrastructure.\n\nIf `helm_install_enabled = false`, uninstall the Helm release yourself and confirm the cleanup job completed before running `terraform destroy`. Terraform cannot clean runtime Kubernetes objects for a Helm release it did not install."
}
