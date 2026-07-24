use super::{yaml_key, yaml_string};
use crate::emitter::{HelmFragment, InfrastructureValue};
use crate::registry::HelmRegistry;
use alien_core::{
    import::EmitContext, AzureResourceGroupOutputs, Container, ContainerCode, Daemon, DaemonCode,
    ErrorData, KubernetesCluster, KubernetesClusterOutputs, KubernetesClusterOwnership,
    KubernetesClusterProvider, Platform, RemoteStackManagementOutputs, ResourceLifecycle, Result,
    ServiceAccount, ServiceAccountOutputs, Stack, StackSettings, Worker, WorkerCode,
};
use alien_error::{AlienError, Context, IntoAlienError};
use indexmap::IndexMap;
use std::collections::{BTreeMap, BTreeSet};

/// Result of dispatching every stack resource through the
/// `HelmRegistry`. Aggregated values land in `values.yaml`; extra
/// templates land under `templates/`.
#[derive(Debug, Default)]
pub(super) struct ChartAnalysis {
    pub(super) service_accounts: BTreeSet<String>,
    service_account_rbac: BTreeMap<String, Vec<KubernetesRoleRule>>,
    infrastructure: Vec<InfrastructureValue>,
    services: Vec<ServiceValue>,
    pub(super) extra_templates: IndexMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KubernetesRoleRule {
    api_groups: Vec<&'static str>,
    resources: Vec<&'static str>,
    verbs: Vec<&'static str>,
}

impl ChartAnalysis {
    pub(super) fn from_stack(stack: &Stack, registry: &HelmRegistry) -> Result<Self> {
        let mut analysis = Self::default();

        let mut service_accounts = stack
            .permission_profiles()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let service_account_rbac = stack
            .permission_profiles()
            .iter()
            .filter_map(|(name, profile)| {
                let rules = kubernetes_rbac_rules_for_permission_profile(profile);
                (!rules.is_empty()).then(|| (name.clone(), rules))
            })
            .collect::<BTreeMap<_, _>>();

        let names = IndexMap::new();
        let stack_settings = StackSettings::default();

        for (resource_id, entry) in stack.resources() {
            if let Some(function) = entry.config.downcast_ref::<Worker>() {
                fail_if_worker_source_remains(resource_id, function)?;
                service_accounts.insert(function.permissions.clone());
                if !function.public_endpoints.is_empty() {
                    analysis.services.push(ServiceValue {
                        id: resource_id.clone(),
                        component: "worker".to_string(),
                        target_port: 8080,
                    });
                }
            }
            if let Some(container) = entry.config.downcast_ref::<Container>() {
                fail_if_container_source_remains(resource_id, container)?;
                if let Some(endpoint) = container.public_endpoints.first() {
                    analysis.services.push(ServiceValue {
                        id: resource_id.clone(),
                        component: "container".to_string(),
                        target_port: endpoint.port,
                    });
                }
            }
            if let Some(build) = entry.config.downcast_ref::<alien_core::Build>() {
                service_accounts.insert(build.permissions.clone());
            }
            if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
                fail_if_daemon_source_remains(resource_id, daemon)?;
            }

            // Frozen resources contribute operator-local infrastructure bindings; live
            // (workload) resources do not — they ARE the workload.
            if entry.lifecycle != ResourceLifecycle::Frozen {
                continue;
            }

            let resource_type = entry.config.resource_type();
            let Some(emitter) = registry.emitter(&resource_type, Platform::Kubernetes) else {
                continue;
            };

            let ctx = EmitContext {
                stack,
                resource: entry,
                resource_id,
                platform: Platform::Kubernetes,
                stack_settings: &stack_settings,
                names: &names,
            };
            let HelmFragment {
                infrastructure,
                extra_templates,
            } = emitter.emit(&ctx)?;
            if let Some(value) = infrastructure {
                analysis.infrastructure.push(value);
            }
            for (path, contents) in extra_templates {
                analysis.extra_templates.insert(path, contents);
            }
        }

        analysis.service_accounts = service_accounts;
        analysis.service_account_rbac = service_account_rbac;
        Ok(analysis)
    }
}

fn kubernetes_rbac_rules_for_permission_profile(
    profile: &alien_core::PermissionProfile,
) -> Vec<KubernetesRoleRule> {
    let mut secret_verbs = BTreeSet::new();
    let mut needs_jobs = false;

    for permission in profile.0.values().flatten() {
        match permission.id() {
            "vault/data-read" => {
                secret_verbs.extend(["get", "list", "watch"]);
            }
            "vault/data-write" => {
                secret_verbs.extend([
                    "get", "list", "watch", "create", "update", "patch", "delete",
                ]);
            }
            "build/execute" => {
                needs_jobs = true;
            }
            _ => {}
        }
    }

    let mut rules = Vec::new();
    if !secret_verbs.is_empty() {
        rules.push(KubernetesRoleRule {
            api_groups: vec![""],
            resources: vec!["secrets"],
            verbs: secret_verbs.into_iter().collect(),
        });
    }
    if needs_jobs {
        rules.push(KubernetesRoleRule {
            api_groups: vec!["batch"],
            resources: vec!["jobs"],
            verbs: vec!["get", "list", "watch", "create", "delete"],
        });
    }

    rules
}

fn fail_if_worker_source_remains(resource_id: &str, worker: &Worker) -> Result<()> {
    if matches!(&worker.code, WorkerCode::Source { .. }) {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "Worker '{resource_id}' still has source code before Helm chart generation; build and inject an image first"
            ),
        }));
    }
    Ok(())
}

fn fail_if_container_source_remains(resource_id: &str, container: &Container) -> Result<()> {
    if matches!(&container.code, ContainerCode::Source { .. }) {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "Container '{resource_id}' still has source code before Helm chart generation; build and inject an image first"
            ),
        }));
    }
    Ok(())
}

fn fail_if_daemon_source_remains(resource_id: &str, daemon: &Daemon) -> Result<()> {
    if matches!(&daemon.code, DaemonCode::Source { .. }) {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "Daemon '{resource_id}' still has source code before Helm chart generation; build and inject an image first"
            ),
        }));
    }
    Ok(())
}

#[derive(Debug)]
struct ServiceValue {
    id: String,
    component: String,
    target_port: u16,
}

pub(super) fn chart_yaml(chart_name: &str, stack: &Stack) -> String {
    format!(
        "apiVersion: v2\nname: {chart_name}\ndescription: Deployment chart for {stack_id}\ntype: application\nversion: 0.1.0\nappVersion: \"0.1.0\"\n",
        stack_id = stack.id()
    )
}

pub(super) fn values_yaml(
    analysis: &ChartAnalysis,
    stack_settings: &StackSettings,
) -> Result<String> {
    let mut yaml = String::new();
    yaml.push_str(
        r#"management:
  token: ""
  existingSecret:
    name: ""
    tokenKey: sync-token
  name: ""
  url: ""
  deploymentId: "dep_replace_me"
  updates: auto
  telemetry: auto
  healthChecks: "on"

runtime:
  image:
    repository: registry.example.com/deployment/operator
    tag: latest
    pullPolicy: IfNotPresent
  imagePullSecrets: []
  podLabels: {}
  podAnnotations: {}
  automountServiceAccountToken: true
  encryption:
    # Set this explicitly, or reference an existing Secret below.
    key: "replace-me-with-a-stable-64-character-encryption-secret"
    existingSecret:
      name: ""
      key: encryption-key
  replicas: 1
  resources:
    requests:
      cpu: 100m
      memory: 128Mi
    limits:
      memory: 512Mi
  api:
    enabled: false
    bindHost: 0.0.0.0
    port: 8080
    service:
      type: ClusterIP
  probes:
    liveness:
      enabled: true
      path: /health
      initialDelaySeconds: 10
      periodSeconds: 10
      timeoutSeconds: 2
      failureThreshold: 3
    readiness:
      enabled: true
      path: /health
      initialDelaySeconds: 5
      periodSeconds: 10
      timeoutSeconds: 2
      failureThreshold: 3
  security:
    podSecurityContext:
      runAsNonRoot: true
      runAsUser: 10001
      runAsGroup: 10001
      fsGroup: 10001
      seccompProfile:
        type: RuntimeDefault
    containerSecurityContext:
      allowPrivilegeEscalation: false
      readOnlyRootFilesystem: true
      capabilities:
        drop:
          - ALL
  tmp:
    enabled: true
    sizeLimit: 256Mi
  data:
    mountPath: /var/lib/deployment-operator
    persistence:
      enabled: false
      existingClaim: ""
      storageClassName: ""
      accessModes:
        - ReadWriteOnce
      size: 1Gi
  scheduling:
    nodeSelector: {}
    tolerations: []
    affinity: {}
    topologySpreadConstraints: []
    priorityClassName: ""
    runtimeClassName: ""
  pdb:
    enabled: false
    minAvailable: 1
  networkPolicy:
    enabled: false
    ingress:
      enabled: true
    egress:
      enabled: true
  cleanup:
    onUninstall:
      enabled: true
      deletePersistentVolumeClaims: false
      image:
        repository: alpine/k8s
        tag: "1.32.0"
        pullPolicy: IfNotPresent

logCollector:
  enabled: false
  token: "replace-me-with-a-stable-in-cluster-collector-token"
  image:
    repository: fluent/fluent-bit
    tag: "3.2"
    pullPolicy: IfNotPresent
  resources:
    requests:
      cpu: 50m
      memory: 64Mi
    limits:
      memory: 256Mi
  scope:
    deploymentLabelKey: "alien.dev/deployment"
    deploymentLabelValue: ""

heartbeat:
  collection:
    nodes:
      enabled: true

clusterBootstrap:
  metricsServer:
    enabled: false
    image: registry.k8s.io/metrics-server/metrics-server:v0.8.1
  storageClass:
    default:
      enabled: false
      name: ""
      provisioner: ""
      parameters: {}
  ingress:
    eksAutoMode:
      enabled: false
      name: alb
      controller: eks.amazonaws.com/alb
      scheme: internet-facing
      subnetIds: []
    azureApplicationGatewayForContainers:
      enabled: false
      applicationLoadBalancer:
        name: ""
        namespace: ""
        associationSubnetId: ""
  compute:
    eksAutoMode:
      arm64NodePool:
        enabled: false
        name: general-purpose-arm64
        nodeClassName: default
        capacityType: on-demand
        instanceCategories:
          - c
          - m
          - r
        minInstanceGeneration: "5"
        limits:
          cpu: "1000"
          memory: 1000Gi

"#,
    );

    append_service_accounts(&mut yaml, analysis);
    append_stack_settings(&mut yaml, stack_settings)?;
    yaml.push_str("\ninfrastructure: null\n\nbasePlatform: null\nbasePlatformConfig:\n  gcp:\n    projectId: \"\"\n    region: \"\"\n  aws:\n    region: \"\"\n  azure:\n    location: \"\"\n    subscriptionId: \"\"\n    tenantId: \"\"\nserviceAccountPrefix: \"\"\nmanagerServiceAccount:\n  annotations: {}\n  labels: {}\n");
    append_services(&mut yaml, analysis);
    yaml.push_str("\npublicEndpoints: {}\n");

    yaml.push_str(
        r#"
persistentStorage:
  storageClassName: ""

ephemeralStorage:
  nodeSelector: {}
"#,
    );
    Ok(yaml)
}

pub(super) fn append_stack_settings(
    yaml: &mut String,
    stack_settings: &StackSettings,
) -> Result<()> {
    if stack_settings == &StackSettings::default() {
        yaml.push_str("\nstackSettings: null\n");
        return Ok(());
    }

    let serialized = serde_yaml::to_string(stack_settings)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "failed to serialize stack settings into chart values".to_string(),
        })?;
    let serialized = serialized
        .strip_prefix("---\n")
        .unwrap_or(&serialized)
        .trim_end();

    if serialized == "{}" || serialized.is_empty() {
        yaml.push_str("\nstackSettings: null\n");
        return Ok(());
    }

    yaml.push_str("\nstackSettings:\n");
    for line in serialized.lines() {
        yaml.push_str("  ");
        yaml.push_str(line);
        yaml.push('\n');
    }

    Ok(())
}

pub(super) fn append_service_accounts(yaml: &mut String, analysis: &ChartAnalysis) {
    yaml.push_str("serviceAccounts:\n");
    if analysis.service_accounts.is_empty() {
        yaml.push_str("  {}\n");
    } else {
        for name in &analysis.service_accounts {
            yaml.push_str(&format!(
                "  {}:\n    annotations: {{}}\n    labels: {{}}\n",
                yaml_key(name)
            ));
            append_service_account_rbac(yaml, analysis.service_account_rbac.get(name));
        }
    }
}

pub(super) fn append_registered_service_accounts(
    yaml: &mut String,
    analysis: &ChartAnalysis,
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) {
    yaml.push_str("serviceAccounts:\n");
    if analysis.service_accounts.is_empty() {
        yaml.push_str("  {}\n");
        return;
    }

    for name in &analysis.service_accounts {
        yaml.push_str(&format!("  {}:\n", yaml_key(name)));
        match service_account_identity_for_profile(stack_state, name) {
            Some(identity) => {
                yaml.push_str("    annotations:\n");
                yaml.push_str(&format!(
                    "      {}: {}\n",
                    yaml_key(identity_annotation_key(base_platform)),
                    yaml_string(identity)
                ));
            }
            None => yaml.push_str("    annotations: {}\n"),
        }
        yaml.push_str("    labels: {}\n");
        append_service_account_rbac(yaml, analysis.service_account_rbac.get(name));
    }
}

fn append_service_account_rbac(yaml: &mut String, rules: Option<&Vec<KubernetesRoleRule>>) {
    let Some(rules) = rules.filter(|rules| !rules.is_empty()) else {
        return;
    };

    yaml.push_str("    rbac:\n");
    yaml.push_str("      rules:\n");
    for rule in rules {
        yaml.push_str("        - apiGroups: ");
        append_yaml_inline_string_list(yaml, &rule.api_groups);
        yaml.push('\n');
        yaml.push_str("          resources: ");
        append_yaml_inline_string_list(yaml, &rule.resources);
        yaml.push('\n');
        yaml.push_str("          verbs: ");
        append_yaml_inline_string_list(yaml, &rule.verbs);
        yaml.push('\n');
    }
}

fn append_yaml_inline_string_list(yaml: &mut String, values: &[&str]) {
    yaml.push('[');
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            yaml.push_str(", ");
        }
        yaml.push_str(&yaml_string(value));
    }
    yaml.push(']');
}

pub(super) fn append_manager_service_account(
    yaml: &mut String,
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) -> Result<()> {
    yaml.push_str("managerServiceAccount:\n");
    match remote_stack_management_identity(stack_state, base_platform)? {
        Some(identity) => {
            yaml.push_str("  annotations:\n");
            yaml.push_str(&format!(
                "    {}: {}\n",
                yaml_key(identity_annotation_key(base_platform)),
                yaml_string(&identity)
            ));
        }
        None => yaml.push_str("  annotations: {}\n"),
    }
    if base_platform == Some(Platform::Azure) {
        yaml.push_str("  labels:\n");
        yaml.push_str("    azure.workload.identity/use: 'true'\n");
    } else {
        yaml.push_str("  labels: {}\n");
    }
    Ok(())
}

pub(super) fn append_runtime_cloud_identity(yaml: &mut String, base_platform: Option<Platform>) {
    if base_platform != Some(Platform::Azure) {
        return;
    }

    yaml.push_str("runtime:\n");
    yaml.push_str("  podLabels:\n");
    yaml.push_str("    azure.workload.identity/use: 'true'\n");
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AzureBasePlatformConfig {
    pub(super) subscription_id: String,
    pub(super) tenant_id: Option<String>,
}

pub(super) fn azure_base_platform_config(
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) -> Result<Option<AzureBasePlatformConfig>> {
    if base_platform != Some(Platform::Azure) {
        return Ok(None);
    }

    let subscription_id = stack_state
        .resources
        .values()
        .find_map(|resource| {
            resource
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
                .and_then(|outputs| {
                    azure_subscription_id_from_resource_id(&outputs.management_resource_id)
                })
        })
        .or_else(|| {
            stack_state.resources.values().find_map(|resource| {
                resource
                    .outputs
                    .as_ref()
                    .and_then(|outputs| outputs.downcast_ref::<AzureResourceGroupOutputs>())
                    .and_then(|outputs| {
                        azure_subscription_id_from_resource_id(&outputs.resource_id)
                    })
            })
        });

    let tenant_id = azure_remote_stack_management_access_config(stack_state)?
        .and_then(|access_config| access_config.tenant_id);

    Ok(
        subscription_id.map(|subscription_id| AzureBasePlatformConfig {
            subscription_id,
            tenant_id,
        }),
    )
}

fn azure_subscription_id_from_resource_id(resource_id: &str) -> Option<String> {
    let mut parts = resource_id.split('/').filter(|part| !part.is_empty());
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("subscriptions") {
            return parts.next().map(str::to_string);
        }
    }
    None
}

pub(super) fn append_cluster_bootstrap(
    yaml: &mut String,
    stack: &Stack,
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) {
    let eks_managed =
        base_platform == Some(Platform::Aws) && managed_eks_cluster_present(stack, stack_state);

    yaml.push_str("clusterBootstrap:\n");
    yaml.push_str("  metricsServer:\n");
    yaml.push_str(&format!("    enabled: {}\n", eks_managed));
    yaml.push_str("    image: registry.k8s.io/metrics-server/metrics-server:v0.8.1\n");
    yaml.push_str("  storageClass:\n");
    yaml.push_str("    default:\n");
    yaml.push_str(&format!("      enabled: {}\n", eks_managed));
    yaml.push_str("      name: \"gp3\"\n");
    yaml.push_str("      provisioner: \"ebs.csi.eks.amazonaws.com\"\n");
    yaml.push_str("      parameters:\n");
    yaml.push_str("        type: \"gp3\"\n");
    yaml.push_str("        fsType: \"ext4\"\n");
    yaml.push_str("        encrypted: \"true\"\n");
    yaml.push_str("  ingress:\n");
    yaml.push_str("    eksAutoMode:\n");
    yaml.push_str(&format!("      enabled: {}\n", eks_managed));
    yaml.push_str("      name: alb\n");
    yaml.push_str("      controller: eks.amazonaws.com/alb\n");
    yaml.push_str("      scheme: internet-facing\n");
    yaml.push_str("      subnetIds: []\n");
    yaml.push_str("    azureApplicationGatewayForContainers:\n");
    match azure_application_gateway_for_containers_bootstrap(stack_state) {
        Some(bootstrap) => {
            yaml.push_str("      enabled: true\n");
            yaml.push_str("      applicationLoadBalancer:\n");
            yaml.push_str(&format!(
                "        name: {}\n",
                yaml_string(&bootstrap.alb_name)
            ));
            yaml.push_str(&format!(
                "        namespace: {}\n",
                yaml_string(&bootstrap.alb_namespace)
            ));
            yaml.push_str(&format!(
                "        associationSubnetId: {}\n",
                yaml_string(&bootstrap.association_subnet_id)
            ));
        }
        None => {
            yaml.push_str("      enabled: false\n");
            yaml.push_str("      applicationLoadBalancer:\n");
            yaml.push_str("        name: \"\"\n");
            yaml.push_str("        namespace: \"\"\n");
            yaml.push_str("        associationSubnetId: \"\"\n");
        }
    }
    yaml.push_str("  compute:\n");
    yaml.push_str("    eksAutoMode:\n");
    yaml.push_str("      arm64NodePool:\n");
    yaml.push_str(&format!("        enabled: {}\n", eks_managed));
    yaml.push_str("        name: general-purpose-arm64\n");
    yaml.push_str("        nodeClassName: default\n");
    yaml.push_str("        capacityType: on-demand\n");
    yaml.push_str("        instanceCategories:\n");
    yaml.push_str("          - c\n");
    yaml.push_str("          - m\n");
    yaml.push_str("          - r\n");
    yaml.push_str("        minInstanceGeneration: \"5\"\n");
    yaml.push_str("        limits:\n");
    yaml.push_str("          cpu: \"1000\"\n");
    yaml.push_str("          memory: 1000Gi\n");
}

fn managed_eks_cluster_present(stack: &Stack, stack_state: &alien_core::StackState) -> bool {
    stack_state.resources.values().any(|resource| {
        resource
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<KubernetesClusterOutputs>())
            .is_some_and(is_managed_eks_cluster_outputs)
            || resource
                .config
                .downcast_ref::<KubernetesCluster>()
                .is_some_and(is_managed_eks_cluster_config)
    }) || stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<KubernetesCluster>()
            .is_some_and(is_managed_eks_cluster_config)
    })
}

fn is_managed_eks_cluster_outputs(outputs: &KubernetesClusterOutputs) -> bool {
    outputs.provider == KubernetesClusterProvider::Eks
        && outputs.ownership == KubernetesClusterOwnership::Managed
}

fn is_managed_eks_cluster_config(cluster: &KubernetesCluster) -> bool {
    cluster.provider == KubernetesClusterProvider::Eks
        && cluster.ownership == KubernetesClusterOwnership::Managed
}

fn azure_application_gateway_for_containers_bootstrap(
    stack_state: &alien_core::StackState,
) -> Option<&alien_core::import::data::AzureApplicationGatewayForContainersBootstrap> {
    stack_state.resources.values().find_map(|resource| {
        resource
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<alien_core::KubernetesClusterOutputs>())
            .and_then(|outputs| outputs.azure_application_gateway_for_containers.as_ref())
    })
}

fn service_account_identity_for_profile<'a>(
    stack_state: &'a alien_core::StackState,
    profile: &str,
) -> Option<&'a str> {
    stack_state
        .resources
        .iter()
        .find_map(|(resource_id, resource)| {
            let outputs = resource
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<ServiceAccountOutputs>())?;
            let service_account = resource.config.downcast_ref::<ServiceAccount>()?;
            let account_profile =
                alien_core::permission_profile_from_service_account_id(service_account.id());
            (account_profile == profile || resource_id == profile)
                .then_some(outputs.identity.as_str())
        })
}

fn remote_stack_management_identity(
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) -> Result<Option<String>> {
    let Some(outputs) = stack_state.resources.values().find_map(|resource| {
        resource
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
    }) else {
        return Ok(None);
    };

    if base_platform == Some(Platform::Azure) {
        let Some(access_config) = azure_remote_stack_management_access_config(stack_state)? else {
            return Ok(None);
        };
        return Ok(Some(access_config.uami_client_id));
    }

    Ok(Some(outputs.management_resource_id.clone()))
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AzureRemoteStackManagementAccessConfig {
    uami_client_id: String,
    tenant_id: Option<String>,
}

fn azure_remote_stack_management_access_config(
    stack_state: &alien_core::StackState,
) -> Result<Option<AzureRemoteStackManagementAccessConfig>> {
    let Some(outputs) = stack_state.resources.values().find_map(|resource| {
        resource
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
    }) else {
        return Ok(None);
    };

    let access_config: AzureRemoteStackManagementAccessConfig =
        serde_json::from_str(&outputs.access_configuration)
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: "Failed to parse Azure management access configuration".to_string(),
            })?;

    if access_config.uami_client_id.is_empty() {
        return Err(AlienError::new(ErrorData::GenericError {
            message: "Azure management access configuration is missing uamiClientId".to_string(),
        }));
    }

    Ok(Some(access_config))
}

fn identity_annotation_key(base_platform: Option<Platform>) -> &'static str {
    match base_platform {
        Some(Platform::Gcp) => "iam.gke.io/gcp-service-account",
        Some(Platform::Azure) => "azure.workload.identity/client-id",
        _ => "eks.amazonaws.com/role-arn",
    }
}

pub(super) fn updates_mode_value(mode: alien_core::UpdatesMode) -> &'static str {
    match mode {
        alien_core::UpdatesMode::Auto => "auto",
        alien_core::UpdatesMode::ApprovalRequired => "approval-required",
    }
}

pub(super) fn telemetry_mode_value(mode: alien_core::TelemetryMode) -> &'static str {
    match mode {
        alien_core::TelemetryMode::Off => "off",
        alien_core::TelemetryMode::Auto => "auto",
        alien_core::TelemetryMode::ApprovalRequired => "approval-required",
    }
}

pub(super) fn heartbeats_mode_value(mode: alien_core::HeartbeatsMode) -> &'static str {
    match mode {
        alien_core::HeartbeatsMode::Off => "off",
        alien_core::HeartbeatsMode::On => "on",
    }
}

pub(super) fn append_infrastructure(yaml: &mut String, analysis: &ChartAnalysis) {
    yaml.push_str("infrastructure:\n");
    if analysis.infrastructure.is_empty() {
        yaml.push_str("  {}\n");
    } else {
        for resource in &analysis.infrastructure {
            yaml.push_str(&format!(
                "  {}:\n    type: {}\n    service: {}\n",
                yaml_key(&resource.id),
                resource.binding_type,
                resource.service
            ));
            for (key, value) in &resource.fields {
                let value = if value == "null" {
                    "null".to_string()
                } else {
                    yaml_string(value)
                };
                yaml.push_str(&format!("    {key}: {value}\n"));
            }
        }
    }
}

pub(super) fn append_services(yaml: &mut String, analysis: &ChartAnalysis) {
    yaml.push_str("\nservices:\n");
    if analysis.services.is_empty() {
        yaml.push_str("  {}\n");
    } else {
        for service in &analysis.services {
            yaml.push_str(&format!(
                "  {}:\n    type: clusterIp\n    port: 80\n    targetPort: {}\n    component: {}\n",
                yaml_key(&service.id),
                service.target_port,
                yaml_string(&service.component)
            ));
        }
    }
}
