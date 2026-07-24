use super::values::{
    append_infrastructure, append_service_accounts, append_services, ChartAnalysis,
};
use super::{yaml_key, yaml_string};
use alien_core::Stack;

pub(super) fn eks_values_example(analysis: &ChartAnalysis) -> String {
    cloud_identity_example(
        analysis,
        "eks.amazonaws.com/role-arn",
        "arn:aws:iam::123456789012:role",
    )
}

pub(super) fn gke_values_example(analysis: &ChartAnalysis) -> String {
    cloud_identity_example(
        analysis,
        "iam.gke.io/gcp-service-account",
        "deployment@project.iam.gserviceaccount.com",
    )
}

pub(super) fn aks_values_example(analysis: &ChartAnalysis) -> String {
    cloud_identity_example(
        analysis,
        "azure.workload.identity/client-id",
        "00000000-0000-0000-0000-000000000000",
    )
}

pub(super) fn onprem_values_example(analysis: &ChartAnalysis) -> String {
    let mut yaml = external_bindings_initialize_example_values();
    append_service_accounts(&mut yaml, analysis);
    yaml.push('\n');
    append_infrastructure(&mut yaml, analysis);
    append_services(&mut yaml, analysis);
    yaml
}

fn cloud_identity_example(
    analysis: &ChartAnalysis,
    annotation: &str,
    identity_prefix: &str,
) -> String {
    let mut yaml = registered_setup_example_values();
    yaml.push_str("serviceAccounts:\n");
    if analysis.service_accounts.is_empty() {
        yaml.push_str("  {}\n");
    } else {
        for name in &analysis.service_accounts {
            let value = if annotation == "eks.amazonaws.com/role-arn" {
                format!("{identity_prefix}/{}-{name}", "deployment")
            } else if annotation == "iam.gke.io/gcp-service-account" {
                identity_prefix.to_string()
            } else {
                identity_prefix.to_string()
            };
            yaml.push_str(&format!(
                "  {}:\n    annotations:\n      {}: {}\n    labels: {{}}\n",
                yaml_key(name),
                yaml_key(annotation),
                yaml_string(&value)
            ));
        }
    }
    append_services(&mut yaml, analysis);
    yaml
}

fn registered_setup_example_values() -> String {
    r#"management:
  token: "dg_replace_me"
  name: "production"
  url: "https://management.example.com"
  deploymentId: "dep_replace_me"
  updates: auto
  telemetry: auto
  healthChecks: "on"

"#
    .to_string()
}

fn external_bindings_initialize_example_values() -> String {
    r#"management:
  token: "dg_replace_me"
  name: "production"
  url: "https://management.example.com"
  deploymentId: null
  updates: auto
  telemetry: auto
  healthChecks: "on"

stackSettings:
  deploymentModel: pull
  network: null
  domains: null
  updates: auto
  telemetry: auto
  heartbeats: "on"

"#
    .to_string()
}

pub(super) fn readme_md(chart_name: &str, stack: &Stack) -> String {
    format!(
        "# {chart_name}\n\nInstall this chart into an existing Kubernetes cluster:\n\n```bash\nhelm install {chart_name} ./{} --namespace production --create-namespace --values values.yaml\n```\n\nThe generated `values.yaml` contains placeholders for management, service-account identity annotations, operator-local infrastructure bindings, and the Kubernetes exposure profile. The chart no longer renders per-app public `Ingress` objects from `services.*.host` or hostless ingress values; public endpoints are runtime-owned through `stackSettings.kubernetes.exposure`.\n\nSee `examples/<target>.yaml` for ready-to-use values matching EKS / GKE / AKS / on-prem.\n",
        stack.id()
    )
}
