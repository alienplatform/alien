//! Top-level Helm chart generator.
//!
//! Drives per-resource [`HelmEmitter`]s through the [`HelmRegistry`] and
//! assembles the chart shell — `Chart.yaml`, the templates, and the
//! values + schema for both bootstrap paths (`manager-fetch path` when
//! `management.deploymentId` is set; external-bindings initialize otherwise).

use crate::{
    emitter::{HelmFragment, InfrastructureValue},
    registry::HelmRegistry,
};
use alien_core::{
    import::EmitContext, Container, ContainerCode, Daemon, DaemonCode, ErrorData, ExposeProtocol,
    Ingress, Platform, RemoteStackManagementOutputs, ResourceLifecycle, Result, ServiceAccount,
    ServiceAccountOutputs, Stack, StackSettings, Worker, WorkerCode,
};
use alien_error::{AlienError, Context, IntoAlienError};
use indexmap::IndexMap;
use std::collections::BTreeSet;

/// Generated Helm chart files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelmChart {
    pub name: String,
    pub files: IndexMap<String, String>,
}

/// Options for Helm chart generation.
pub struct HelmOptions<'a> {
    /// Per-`(ResourceType, Platform)` emitter dispatch. Most callers
    /// pass [`HelmRegistry::built_in()`]; plugin-aware callers extend it
    /// before passing.
    pub registry: &'a HelmRegistry,
    pub stack_settings: StackSettings,
    pub chart_name: String,
}

/// Inputs for rendering the manager-fetch `values.yaml` used after setup import.
pub struct ManagerFetchHelmValuesOptions<'a> {
    pub deployment_id: &'a str,
    pub deployment_name: &'a str,
    pub manager_url: &'a str,
    pub deployment_token: &'a str,
    pub stack: &'a Stack,
    pub stack_state: &'a alien_core::StackState,
    pub stack_settings: &'a StackSettings,
    pub base_platform: Option<Platform>,
    pub region: Option<&'a str>,
    pub gcp_project_id: Option<&'a str>,
    pub azure_location: Option<&'a str>,
}

/// Generate a Helm chart for `stack`.
pub fn generate_helm_chart(stack: &Stack, options: HelmOptions<'_>) -> Result<HelmChart> {
    let chart_name = sanitize_chart_name(&options.chart_name);
    let analysis = ChartAnalysis::from_stack(stack, options.registry)?;

    let stack_json = serde_json::to_string_pretty(stack)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "failed to serialize stack into chart metadata".to_string(),
        })?;
    let stack_settings_json = serde_json::to_string_pretty(&options.stack_settings)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "failed to serialize stack settings into chart metadata".to_string(),
        })?;

    let mut files = IndexMap::new();
    files.insert("Chart.yaml".to_string(), chart_yaml(&chart_name, stack));
    files.insert(
        "values.yaml".to_string(),
        values_yaml(&analysis, &options.stack_settings)?,
    );
    files.insert("values.schema.json".to_string(), values_schema_json());
    files.insert("templates/_helpers.tpl".to_string(), helpers_tpl());
    files.insert(
        "templates/serviceaccount.yaml".to_string(),
        serviceaccount_tpl(),
    );
    files.insert("templates/role.yaml".to_string(), role_tpl());
    files.insert("templates/rolebinding.yaml".to_string(), rolebinding_tpl());
    files.insert("templates/clusterrole.yaml".to_string(), clusterrole_tpl());
    files.insert(
        "templates/clusterrolebinding.yaml".to_string(),
        clusterrolebinding_tpl(),
    );
    files.insert("templates/secret.yaml".to_string(), secret_tpl());
    files.insert("templates/configmap.yaml".to_string(), configmap_tpl());
    files.insert("templates/deployment.yaml".to_string(), deployment_tpl());
    files.insert("templates/pvc.yaml".to_string(), pvc_tpl());
    files.insert("templates/service.yaml".to_string(), service_tpl());
    files.insert("templates/app-service.yaml".to_string(), app_service_tpl());
    files.insert(
        "templates/cluster-bootstrap.yaml".to_string(),
        cluster_bootstrap_tpl(),
    );
    files.insert(
        "templates/poddisruptionbudget.yaml".to_string(),
        poddisruptionbudget_tpl(),
    );
    files.insert(
        "templates/networkpolicy.yaml".to_string(),
        networkpolicy_tpl(),
    );

    // Per-resource extra templates contributed by emitters.
    for (path, contents) in &analysis.extra_templates {
        files.insert(format!("templates/{path}"), contents.clone());
    }

    files.insert(
        "examples/eks.yaml".to_string(),
        eks_values_example(&analysis),
    );
    files.insert(
        "examples/gke.yaml".to_string(),
        gke_values_example(&analysis),
    );
    files.insert(
        "examples/aks.yaml".to_string(),
        aks_values_example(&analysis),
    );
    files.insert(
        "examples/onprem.yaml".to_string(),
        onprem_values_example(&analysis),
    );
    files.insert("README.md".to_string(), readme_md(&chart_name, stack));

    files.insert(
        "files/stack.json".to_string(),
        ensure_trailing_newline(stack_json),
    );
    files.insert(
        "files/stack-settings.json".to_string(),
        ensure_trailing_newline(stack_settings_json),
    );

    Ok(HelmChart {
        name: chart_name,
        files,
    })
}

/// Render one complete manager-fetch values file from imported deployment state.
pub fn render_manager_fetch_values(options: ManagerFetchHelmValuesOptions<'_>) -> Result<String> {
    let registry = HelmRegistry::built_in();
    let analysis = ChartAnalysis::from_stack(options.stack, &registry)?;
    let mut yaml = String::new();

    yaml.push_str("management:\n");
    yaml.push_str(&format!(
        "  token: {}\n",
        yaml_string(options.deployment_token)
    ));
    yaml.push_str(&format!(
        "  name: {}\n",
        yaml_string(options.deployment_name)
    ));
    yaml.push_str(&format!("  url: {}\n", yaml_string(options.manager_url)));
    yaml.push_str(&format!(
        "  deploymentId: {}\n",
        yaml_string(options.deployment_id)
    ));
    yaml.push_str(&format!(
        "  updates: {}\n",
        yaml_string(updates_mode_value(options.stack_settings.updates))
    ));
    yaml.push_str(&format!(
        "  telemetry: {}\n",
        yaml_string(telemetry_mode_value(options.stack_settings.telemetry))
    ));
    yaml.push_str(&format!(
        "  healthChecks: {}\n\n",
        yaml_string(heartbeats_mode_value(options.stack_settings.heartbeats))
    ));

    append_stack_settings(&mut yaml, options.stack_settings)?;
    yaml.push_str("\ninfrastructure: null\n\n");

    match options.base_platform {
        Some(platform) => yaml.push_str(&format!(
            "basePlatform: {}\n",
            yaml_string(platform.as_str())
        )),
        None => yaml.push_str("basePlatform: null\n"),
    }
    yaml.push_str("basePlatformConfig:\n");
    yaml.push_str("  gcp:\n");
    yaml.push_str(&format!(
        "    projectId: {}\n",
        yaml_string(options.gcp_project_id.unwrap_or(""))
    ));
    yaml.push_str(&format!(
        "    region: {}\n",
        yaml_string(options.region.unwrap_or(""))
    ));
    yaml.push_str("  aws:\n");
    yaml.push_str(&format!(
        "    region: {}\n",
        yaml_string(options.region.unwrap_or(""))
    ));
    yaml.push_str("  azure:\n");
    yaml.push_str(&format!(
        "    location: {}\n",
        yaml_string(options.azure_location.or(options.region).unwrap_or(""))
    ));
    yaml.push_str(&format!(
        "serviceAccountPrefix: {}\n",
        yaml_string(&options.stack_state.resource_prefix)
    ));

    append_manager_service_account(&mut yaml, options.stack_state, options.base_platform);
    append_imported_service_accounts(
        &mut yaml,
        &analysis,
        options.stack_state,
        options.base_platform,
    );
    append_cluster_bootstrap(&mut yaml, options.stack_state, options.base_platform);
    append_services(&mut yaml, &analysis);
    yaml.push_str("\npublicUrls: {}\n");

    Ok(yaml)
}

/// Result of dispatching every stack resource through the
/// `HelmRegistry`. Aggregated values land in `values.yaml`; extra
/// templates land under `templates/`.
#[derive(Debug, Default)]
struct ChartAnalysis {
    service_accounts: BTreeSet<String>,
    infrastructure: Vec<InfrastructureValue>,
    services: Vec<ServiceValue>,
    extra_templates: IndexMap<String, String>,
}

impl ChartAnalysis {
    fn from_stack(stack: &Stack, registry: &HelmRegistry) -> Result<Self> {
        let mut analysis = Self::default();

        let mut service_accounts = stack
            .permission_profiles()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();

        let names = IndexMap::new();
        let stack_settings = StackSettings::default();

        for (resource_id, entry) in stack.resources() {
            if let Some(function) = entry.config.downcast_ref::<Worker>() {
                fail_if_worker_source_remains(resource_id, function)?;
                service_accounts.insert(function.permissions.clone());
                if function.ingress == Ingress::Public {
                    analysis.services.push(ServiceValue {
                        id: resource_id.clone(),
                        component: "worker".to_string(),
                        target_port: 8080,
                    });
                }
            }
            if let Some(container) = entry.config.downcast_ref::<Container>() {
                fail_if_container_source_remains(resource_id, container)?;
                if let Some(port) = container
                    .ports
                    .iter()
                    .find(|port| port.expose == Some(ExposeProtocol::Http))
                {
                    analysis.services.push(ServiceValue {
                        id: resource_id.clone(),
                        component: "container".to_string(),
                        target_port: port.port,
                    });
                }
            }
            if let Some(build) = entry.config.downcast_ref::<alien_core::Build>() {
                service_accounts.insert(build.permissions.clone());
            }
            if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
                fail_if_daemon_source_remains(resource_id, daemon)?;
            }

            // Frozen resources contribute agent-local infrastructure bindings; live
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
        Ok(analysis)
    }
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

fn chart_yaml(chart_name: &str, stack: &Stack) -> String {
    format!(
        "apiVersion: v2\nname: {chart_name}\ndescription: Deployment chart for {stack_id}\ntype: application\nversion: 0.1.0\nappVersion: \"0.1.0\"\n",
        stack_id = stack.id()
    )
}

fn values_yaml(analysis: &ChartAnalysis, stack_settings: &StackSettings) -> Result<String> {
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
    repository: ghcr.io/alienplatform/alien-agent
    tag: latest
    pullPolicy: IfNotPresent
  imagePullSecrets: []
  podLabels: {}
  podAnnotations: {}
  automountServiceAccountToken: true
  encryption:
    # Leave empty to let the chart generate a stable random 64-hex-char key
    # on first install (preserved across upgrades via `lookup`). To pin the
    # key explicitly, set it here (must be 64 hex chars = 256-bit AES). To
    # source it from an external secret store, set `existingSecret.name`.
    key: ""
    existingSecret:
      name: ""
      key: encryption-key
  # Agent self-update inputs the agent passes to the Helm-runner Job it
  # spawns on `agent_target.helm`. Set chartRef + chartVersion to the OCI
  # ref + version used at install time — the agent re-uses them in
  # `helm upgrade --reuse-values`. Leave blank if you don't want to enable
  # in-cluster agent upgrades for this install.
  upgrade:
    chartRef: ""
    chartVersion: ""
    helmRunnerImage: "alpine/helm:3.18.4"
    # Extra flags appended to the `helm upgrade` command the agent's
    # helm-runner Job runs (e.g. `--plain-http` for local-dev OCI
    # registries served over HTTP). Production should leave empty.
    extraArgs: ""
  replicas: 1
  # Helm's --atomic --wait gives up after this many seconds if /readyz
  # hasn't returned 200 — the revision is then rolled back automatically.
  progressDeadlineSeconds: 120
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
    # /livez is process liveness; /readyz turns 200 only after the agent
    # completes at least one /v1/sync round-trip with the manager — the
    # gate Helm's --atomic --wait relies on so a freshly-rolled agent
    # is not considered ready until it has actually reached the manager.
    liveness:
      enabled: true
      path: /livez
      initialDelaySeconds: 10
      periodSeconds: 10
      timeoutSeconds: 2
      failureThreshold: 3
    readiness:
      enabled: true
      path: /readyz
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
    mountPath: /var/lib/alien-agent
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
    yaml.push_str("\ninfrastructure: null\n\nbasePlatform: null\nbasePlatformConfig:\n  gcp:\n    projectId: \"\"\n    region: \"\"\n  aws:\n    region: \"\"\n  azure:\n    location: \"\"\nserviceAccountPrefix: \"\"\nmanagerServiceAccount:\n  annotations: {}\n  labels: {}\n\n# Agent self-update. When the agent receives agent_target.helm on /v1/sync\n# it creates a short-lived Helm-runner Job that runs `helm upgrade --atomic`.\n# The Job runs as `alien-agent-upgrader`; we keep the SA optional so charts\n# that don't want self-update can disable it.\nupgrader:\n  enabled: true\n");
    append_services(&mut yaml, analysis);
    yaml.push_str("\npublicUrls: {}\n");

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

fn append_stack_settings(yaml: &mut String, stack_settings: &StackSettings) -> Result<()> {
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

fn append_service_accounts(yaml: &mut String, analysis: &ChartAnalysis) {
    yaml.push_str("serviceAccounts:\n");
    if analysis.service_accounts.is_empty() {
        yaml.push_str("  {}\n");
    } else {
        for name in &analysis.service_accounts {
            yaml.push_str(&format!(
                "  {}:\n    annotations: {{}}\n    labels: {{}}\n",
                yaml_key(name)
            ));
        }
    }
}

fn append_imported_service_accounts(
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
    }
}

fn append_manager_service_account(
    yaml: &mut String,
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) {
    yaml.push_str("managerServiceAccount:\n");
    match remote_stack_management_identity(stack_state) {
        Some(identity) => {
            yaml.push_str("  annotations:\n");
            yaml.push_str(&format!(
                "    {}: {}\n",
                yaml_key(identity_annotation_key(base_platform)),
                yaml_string(identity)
            ));
        }
        None => yaml.push_str("  annotations: {}\n"),
    }
    yaml.push_str("  labels: {}\n");
}

fn append_cluster_bootstrap(
    yaml: &mut String,
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) {
    let eks_managed = base_platform == Some(Platform::Aws)
        && stack_state.resources.values().any(|resource| {
            resource
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<alien_core::KubernetesClusterOutputs>())
                .is_some_and(|outputs| {
                    outputs.provider == alien_core::KubernetesClusterProvider::Eks
                        && outputs.ownership == alien_core::KubernetesClusterOwnership::Managed
                })
        });

    yaml.push_str("clusterBootstrap:\n");
    yaml.push_str("  metricsServer:\n");
    yaml.push_str(&format!("    enabled: {}\n", eks_managed));
    yaml.push_str("    image: registry.k8s.io/metrics-server/metrics-server:v0.8.1\n");
    yaml.push_str("  storageClass:\n");
    yaml.push_str("    default:\n");
    yaml.push_str(&format!("      enabled: {}\n", eks_managed));
    yaml.push_str("      name: \"gp3\"\n");
    yaml.push_str("      provisioner: \"ebs.csi.aws.com\"\n");
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

fn remote_stack_management_identity(stack_state: &alien_core::StackState) -> Option<&str> {
    stack_state.resources.values().find_map(|resource| {
        resource
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
            .map(|outputs| outputs.management_resource_id.as_str())
    })
}

fn identity_annotation_key(base_platform: Option<Platform>) -> &'static str {
    match base_platform {
        Some(Platform::Gcp) => "iam.gke.io/gcp-service-account",
        Some(Platform::Azure) => "azure.workload.identity/client-id",
        _ => "eks.amazonaws.com/role-arn",
    }
}

fn updates_mode_value(mode: alien_core::UpdatesMode) -> &'static str {
    match mode {
        alien_core::UpdatesMode::Auto => "auto",
        alien_core::UpdatesMode::ApprovalRequired => "approval-required",
    }
}

fn telemetry_mode_value(mode: alien_core::TelemetryMode) -> &'static str {
    match mode {
        alien_core::TelemetryMode::Off => "off",
        alien_core::TelemetryMode::Auto => "auto",
        alien_core::TelemetryMode::ApprovalRequired => "approval-required",
    }
}

fn heartbeats_mode_value(mode: alien_core::HeartbeatsMode) -> &'static str {
    match mode {
        alien_core::HeartbeatsMode::Off => "off",
        alien_core::HeartbeatsMode::On => "on",
    }
}

fn append_infrastructure(yaml: &mut String, analysis: &ChartAnalysis) {
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

fn append_services(yaml: &mut String, analysis: &ChartAnalysis) {
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

fn values_schema_json() -> String {
    r##"{
  "$schema": "https://json-schema.org/draft-07/schema#",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "nameOverride": { "type": "string" },
    "fullnameOverride": { "type": "string" },
    "management": {
      "type": "object",
      "additionalProperties": false,
      "required": ["token", "updates", "telemetry", "healthChecks"],
      "properties": {
        "token": { "type": "string" },
        "existingSecret": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "name": { "type": "string" },
            "tokenKey": { "type": "string", "minLength": 1 }
          }
        },
        "name": { "type": "string" },
        "url": { "type": "string" },
        "deploymentId": { "type": ["string", "null"] },
        "updates": { "type": "string", "enum": ["auto", "approval-required"] },
        "telemetry": { "type": "string", "enum": ["auto", "approval-required", "off"] },
        "healthChecks": { "type": "string", "enum": ["on", "off"] }
      }
    },
    "runtime": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "image": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "repository": { "type": "string", "minLength": 1 },
            "tag": { "type": "string", "minLength": 1 },
            "pullPolicy": { "type": "string", "enum": ["Always", "IfNotPresent", "Never"] }
          }
        },
        "imagePullSecrets": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["name"],
            "properties": { "name": { "type": "string", "minLength": 1 } }
          }
        },
        "podLabels": { "type": "object", "additionalProperties": { "type": "string" } },
        "podAnnotations": { "type": "object", "additionalProperties": { "type": "string" } },
        "automountServiceAccountToken": { "type": "boolean" },
        "encryption": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "key": { "type": "string" },
            "existingSecret": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "name": { "type": "string" },
                "key": { "type": "string", "minLength": 1 }
              }
            }
          }
        },
        "upgrade": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "chartRef": { "type": "string" },
            "chartVersion": { "type": "string" },
            "helmRunnerImage": { "type": "string" },
            "extraArgs": { "type": "string" }
          }
        },
        "replicas": { "type": "integer", "minimum": 1 },
        "progressDeadlineSeconds": { "type": "integer", "minimum": 1 },
        "resources": { "type": "object" },
        "api": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "bindHost": { "type": "string" },
            "port": { "type": "integer", "minimum": 1, "maximum": 65535 },
            "service": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "type": { "type": "string", "enum": ["ClusterIP", "NodePort", "LoadBalancer"] }
              }
            }
          }
        },
        "probes": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "liveness": { "$ref": "#/definitions/httpProbe" },
            "readiness": { "$ref": "#/definitions/httpProbe" }
          }
        },
        "security": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "podSecurityContext": { "type": "object" },
            "containerSecurityContext": { "type": "object" }
          }
        },
        "tmp": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "sizeLimit": { "type": "string" }
          }
        },
        "data": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "mountPath": { "type": "string", "minLength": 1 },
            "persistence": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "existingClaim": { "type": "string" },
                "storageClassName": { "type": "string" },
                "accessModes": { "type": "array", "items": { "type": "string" } },
                "size": { "type": "string" }
              }
            }
          }
        },
        "scheduling": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "nodeSelector": { "type": "object", "additionalProperties": { "type": "string" } },
            "tolerations": { "type": "array" },
            "affinity": { "type": "object" },
            "topologySpreadConstraints": { "type": "array" },
            "priorityClassName": { "type": "string" },
            "runtimeClassName": { "type": "string" }
          }
        },
        "pdb": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "minAvailable": { "type": ["integer", "string"] },
            "maxUnavailable": { "type": ["integer", "string"] }
          }
        },
        "networkPolicy": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "ingress": {
              "type": "object",
              "additionalProperties": false,
              "properties": { "enabled": { "type": "boolean" } }
            },
            "egress": {
              "type": "object",
              "additionalProperties": false,
              "properties": { "enabled": { "type": "boolean" } }
            }
          }
        }
      }
    },
    "managerServiceAccount": {
      "type": "object",
      "properties": {
        "annotations": { "type": "object", "additionalProperties": { "type": "string" } },
        "labels": { "type": "object", "additionalProperties": { "type": "string" } }
      }
    },
    "serviceAccounts": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "properties": {
          "annotations": { "type": "object", "additionalProperties": { "type": "string" } },
          "labels": { "type": "object", "additionalProperties": { "type": "string" } }
        }
      }
    },
    "stackSettings": {
      "type": ["object", "null"],
      "properties": {
        "deploymentModel": { "type": "string", "enum": ["pull", "Pull"] },
        "updates": { "type": "string" },
        "telemetry": { "type": "string" },
        "heartbeats": { "type": "string" }
      },
      "additionalProperties": true
    },
    "infrastructure": { "type": ["object", "null"] },
    "basePlatform": { "type": ["string", "null"], "enum": ["aws", "gcp", "azure", null] },
    "basePlatformConfig": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "gcp": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "projectId": { "type": "string" },
            "region": { "type": "string" }
          }
        },
        "aws": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "region": { "type": "string" }
          }
        },
        "azure": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "location": { "type": "string" }
          }
        }
      }
    },
    "heartbeat": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "collection": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "nodes": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" }
              }
            }
          }
        }
      }
    },
    "clusterBootstrap": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "metricsServer": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "image": { "type": "string" }
          }
        },
        "storageClass": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "default": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "name": { "type": "string" },
                "provisioner": { "type": "string" },
                "parameters": { "type": "object", "additionalProperties": { "type": "string" } }
              }
            }
          }
        },
        "ingress": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "eksAutoMode": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "name": { "type": "string" },
                "controller": { "type": "string" },
                "scheme": { "type": "string" },
                "subnetIds": {
                  "type": "array",
                  "items": { "type": "string" }
                }
              }
            }
          }
        },
        "compute": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "eksAutoMode": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "arm64NodePool": {
                  "type": "object",
                  "additionalProperties": false,
                  "properties": {
                    "enabled": { "type": "boolean" },
                    "name": { "type": "string" },
                    "nodeClassName": { "type": "string" },
                    "capacityType": { "type": "string" },
                    "instanceCategories": {
                      "type": "array",
                      "items": { "type": "string" }
                    },
                    "minInstanceGeneration": { "type": "string" },
                    "limits": {
                      "type": "object",
                      "additionalProperties": false,
                      "properties": {
                        "cpu": { "type": "string" },
                        "memory": { "type": "string" }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    },
    "serviceAccountPrefix": { "type": "string" },
    "upgrader": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "enabled": { "type": "boolean" }
      }
    },
    "services": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "type": { "type": "string", "enum": ["clusterIp", "loadBalancer"] },
          "port": { "type": "integer", "minimum": 1, "maximum": 65535 },
          "targetPort": { "type": "integer", "minimum": 1, "maximum": 65535 },
          "component": { "type": "string" }
        }
      }
    },
    "publicUrls": { "type": "object", "additionalProperties": { "type": "string" } },
    "persistentStorage": { "type": "object" },
    "ephemeralStorage": { "type": "object" }
  },
  "definitions": {
    "httpProbe": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "enabled": { "type": "boolean" },
        "path": { "type": "string", "minLength": 1 },
        "initialDelaySeconds": { "type": "integer", "minimum": 0 },
        "periodSeconds": { "type": "integer", "minimum": 1 },
        "timeoutSeconds": { "type": "integer", "minimum": 1 },
        "failureThreshold": { "type": "integer", "minimum": 1 }
      }
    }
  },
  "oneOf": [
    {
      "title": "manager-fetch path",
      "required": ["management"],
      "properties": {
        "management": {
          "required": ["token", "deploymentId"],
          "properties": {
            "deploymentId": { "type": "string", "minLength": 1 }
          }
        },
        "infrastructure": { "type": "null" }
      }
    },
    {
      "title": "external-bindings initialize path",
      "required": ["management", "infrastructure"],
      "properties": {
        "management": {
          "properties": {
            "deploymentId": { "type": "null" }
          }
        },
        "stackSettings": { "type": ["object", "null"] },
        "infrastructure": { "type": "object" }
      }
    }
  ]
}
"##
    .to_string()
}

fn helpers_tpl() -> String {
    r#"{{- define "alien.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "alien.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "alien.labels" -}}
app.kubernetes.io/name: {{ include "alien.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
{{- end -}}

{{- define "alien.managerServiceAccountName" -}}
{{- $prefix := default (include "alien.fullname" .) .Values.serviceAccountPrefix -}}
{{- $raw := printf "%s-manager-sa" $prefix | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
  ServiceAccount used by the Helm-runner Job the agent creates when it
  acts on agent_target.helm. Held as a least-privilege boundary; bound
  to the existing Role so the Job can mutate the Deployment + release
  Secrets.
*/}}
{{- define "alien.upgraderServiceAccountName" -}}
{{- $prefix := default (include "alien.fullname" .) .Values.serviceAccountPrefix -}}
{{- $raw := printf "%s-upgrader-sa" $prefix | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "alien.serviceAccountName" -}}
{{- $prefix := default (include "alien.fullname" .root) .root.Values.serviceAccountPrefix -}}
{{- $raw := printf "%s-%s-sa" $prefix .name | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "alien.resourceName" -}}
{{- $raw := .name | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "alien.managementSecretName" -}}
{{- default (include "alien.fullname" .) .Values.management.existingSecret.name -}}
{{- end -}}

{{- define "alien.managementSecretTokenKey" -}}
{{- default "sync-token" .Values.management.existingSecret.tokenKey -}}
{{- end -}}

{{- define "alien.encryptionSecretName" -}}
{{- default (include "alien.fullname" .) .Values.runtime.encryption.existingSecret.name -}}
{{- end -}}

{{- define "alien.encryptionSecretKey" -}}
{{- default "encryption-key" .Values.runtime.encryption.existingSecret.key -}}
{{- end -}}

{{- define "alien.heartbeatNodeClusterRoleName" -}}
{{- printf "%s-heartbeat-nodes" (include "alien.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- /* Name of the ClusterRole that grants the agent self-update Job permission
       to manage the chart-owned cluster-scoped resources (currently just the
       heartbeat ClusterRole+Binding). Only created when both `upgrader.enabled`
       and the heartbeat node-collection feature are on. */ -}}
{{- define "alien.upgraderClusterRoleName" -}}
{{- printf "%s-upgrader" (include "alien.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
"#
    .to_string()
}

fn serviceaccount_tpl() -> String {
    r#"apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "alien.managerServiceAccountName" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
    {{- with .Values.managerServiceAccount.labels }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
  {{- with .Values.managerServiceAccount.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
---
{{- range $name, $account := .Values.serviceAccounts }}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "alien.serviceAccountName" (dict "root" $ "name" $name) }}
  labels:
    {{- include "alien.labels" $ | nindent 4 }}
    {{- with $account.labels }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
  {{- with $account.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
---
{{- end }}
{{- if .Values.upgrader.enabled }}
# alien-agent-upgrader is the ServiceAccount used by the Helm-runner Job
# the agent creates when it acts on agent_target.helm. It exists as a
# least-privilege boundary for the Job — the agent pod itself uses
# `alien-agent-manager-sa` and only needs to create Jobs + stage
# ConfigMaps/Secrets. Operators are not restricted by this — the
# protection against bad helm upgrades is the chart's `required` values,
# not RBAC.
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "alien.upgraderServiceAccountName" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
---
{{- end }}
"#
    .to_string()
}

fn role_tpl() -> String {
    r#"{{- $stackSettings := default dict .Values.stackSettings -}}
{{- $exposure := dig "kubernetes" "exposure" dict $stackSettings -}}
{{- $exposureMode := dig "mode" "" $exposure -}}
{{- $route := dig "route" dict $exposure -}}
{{- $routeApi := dig "routeApi" "" $route -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "alien.fullname" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["configmaps", "secrets", "services", "pods", "pods/log", "persistentvolumeclaims"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # ServiceAccounts + RBAC objects need to live here for `helm upgrade
  # --reuse-values` to inspect (and patch) the release's existing
  # resources during agent self-update. Without this, the upgrader SA
  # — which is bound to this Role — can't `get` the SAs the chart
  # already created and helm 4xx's out.
  - apiGroups: [""]
    resources: ["serviceaccounts"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources: ["roles", "rolebindings"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: [""]
    resources: ["events"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["apps"]
    resources: ["deployments", "statefulsets", "daemonsets", "replicasets"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["metrics.k8s.io"]
    resources: ["pods"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["batch"]
    resources: ["jobs"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["networkpolicies"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  {{- if and (ne $exposureMode "disabled") (eq $routeApi "ingress") }}
  - apiGroups: ["networking.k8s.io"]
    resources: ["ingresses"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  {{- end }}
  {{- if and (ne $exposureMode "disabled") (eq $routeApi "gateway") }}
  - apiGroups: ["gateway.networking.k8s.io"]
    resources: ["gateways", "httproutes"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["networking.gke.io"]
    resources: ["healthcheckpolicies"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  {{- end }}
"#
    .to_string()
}

fn rolebinding_tpl() -> String {
    r#"apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ include "alien.fullname" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "alien.managerServiceAccountName" . }}
  {{- /* Execution ServiceAccounts (workers/daemons/containers) need RBAC to
         read their own vault secrets (e.g. the injected ALIEN_COMMANDS_TOKEN).
         Without this binding, the worker pod gets 403 reading any secret
         provisioned by the KubernetesVaultController. */}}
  {{- range $name, $account := .Values.serviceAccounts }}
  - kind: ServiceAccount
    name: {{ include "alien.serviceAccountName" (dict "root" $ "name" $name) }}
  {{- end }}
  {{- if .Values.upgrader.enabled }}
  - kind: ServiceAccount
    name: {{ include "alien.upgraderServiceAccountName" . }}
  {{- end }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "alien.fullname" . }}
"#
    .to_string()
}

fn clusterrole_tpl() -> String {
    r#"{{- $nodeCollectionEnabled := dig "collection" "nodes" "enabled" true (default dict .Values.heartbeat) -}}
{{- if $nodeCollectionEnabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {{ include "alien.heartbeatNodeClusterRoleName" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["nodes"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["metrics.k8s.io"]
    resources: ["nodes"]
    verbs: ["get", "list", "watch"]
{{- if .Values.upgrader.enabled }}
---
# Narrow cluster-scoped RBAC for the agent self-update helm-runner Job.
# The chart creates exactly one cluster-scoped resource type pair —
# the heartbeat ClusterRole + ClusterRoleBinding above — and the
# upgrader SA needs to be able to `get/update/patch/delete` them
# during `helm upgrade --reuse-values`. `resourceNames` scopes this
# to ONLY the chart's own cluster objects; no enumeration of other
# tenants' cluster resources. Verbs are deliberately minimal (no
# `list`/`watch`, no `create` — chart install is what creates them
# the first time, run by the customer's helm operator). If a future
# chart version introduces new cluster-scoped resources, add their
# names to `resourceNames` (and a `create` verb on a separate rule
# without `resourceNames` if the upgrader needs to add new ones).
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {{ include "alien.upgraderClusterRoleName" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
rules:
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources: ["clusterroles", "clusterrolebindings"]
    resourceNames:
      # The heartbeat-nodes cluster pair — the existing reason the
      # upgrader needs cluster-scope access at all.
      - {{ include "alien.heartbeatNodeClusterRoleName" . | quote }}
      # And the upgrader cluster pair itself — helm now tracks these as
      # chart-owned, so every `helm upgrade --reuse-values` does a `get`
      # on them to compute the diff. Without self-reference, the first
      # upgrade trips on `clusterroles "<name>-upgrader" is forbidden`.
      - {{ include "alien.upgraderClusterRoleName" . | quote }}
    verbs: ["get", "update", "patch", "delete"]
{{- end }}
{{- end }}
"#
    .to_string()
}

fn clusterrolebinding_tpl() -> String {
    r#"{{- $nodeCollectionEnabled := dig "collection" "nodes" "enabled" true (default dict .Values.heartbeat) -}}
{{- if $nodeCollectionEnabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{ include "alien.heartbeatNodeClusterRoleName" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "alien.managerServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{ include "alien.heartbeatNodeClusterRoleName" . }}
{{- if .Values.upgrader.enabled }}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{ include "alien.upgraderClusterRoleName" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "alien.upgraderServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{ include "alien.upgraderClusterRoleName" . }}
{{- end }}
{{- end }}
"#
    .to_string()
}

fn secret_tpl() -> String {
    // Encryption key resolution order:
    //   1. user-provided `runtime.encryption.key`
    //   2. existing in-cluster Secret's encryption-key (preserves the key
    //      across `helm upgrade` so previously-encrypted data stays readable)
    //   3. freshly generated via `randBytes 32` — crypto/rand-backed in
    //      sprig 3.2+; if your Helm bundles an older sprig, set the key
    //      explicitly via `runtime.encryption.key` or
    //      `runtime.encryption.existingSecret.name`.
    //
    // `lookup` returns nil during `helm template` (no cluster access), so
    // a `helm template | kubectl apply -f -` workflow would generate a
    // fresh key on each render — install via `helm install/upgrade` to
    // keep the key stable, or always set `runtime.encryption.key`.
    r#"{{- $createManagementSecret := not .Values.management.existingSecret.name -}}
{{- $createEncryptionSecret := not .Values.runtime.encryption.existingSecret.name -}}
{{- $encryptionKey := "" -}}
{{- if $createEncryptionSecret -}}
  {{- $providedKey := .Values.runtime.encryption.key | default "" | trim -}}
  {{- $existingKey := "" -}}
  {{- $existingSecret := lookup "v1" "Secret" .Release.Namespace (include "alien.fullname" .) -}}
  {{- if and $existingSecret $existingSecret.data -}}
    {{- with index $existingSecret.data "encryption-key" -}}
      {{- $existingKey = b64dec . -}}
    {{- end -}}
  {{- end -}}
  {{- if $providedKey -}}
    {{- $encryptionKey = $providedKey -}}
  {{- else if $existingKey -}}
    {{- $encryptionKey = $existingKey -}}
  {{- else -}}
    {{- /* sprig randBytes returns base64; b64dec to raw bytes then hex */ -}}
    {{- $encryptionKey = printf "%x" (b64dec (randBytes 32)) -}}
  {{- end -}}
{{- end -}}
{{- if or $createManagementSecret $createEncryptionSecret .Values.infrastructure }}
apiVersion: v1
kind: Secret
metadata:
  name: {{ include "alien.fullname" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
type: Opaque
stringData:
  {{- if $createManagementSecret }}
  sync-token: {{ required "management.token or management.existingSecret.name is required — pass the full values document" .Values.management.token | quote }}
  {{- end }}
  {{- if $createEncryptionSecret }}
  encryption-key: {{ $encryptionKey | quote }}
  {{- end }}
  {{- if .Values.infrastructure }}
  external-bindings.json: {{ toJson .Values.infrastructure | quote }}
  {{- end }}
{{- end }}
"#
    .to_string()
}

fn configmap_tpl() -> String {
    r#"{{- $defaultStackSettings := dict "deploymentModel" "pull" "updates" .Values.management.updates "telemetry" .Values.management.telemetry "heartbeats" .Values.management.healthChecks -}}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ include "alien.fullname" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
data:
  stack.json: |-
{{ .Files.Get "files/stack.json" | indent 4 }}
  stack-settings.json: {{ toJson (default $defaultStackSettings .Values.stackSettings) | quote }}
  services.json: {{ toJson .Values.services | quote }}
  public-urls.json: {{ toJson (default dict .Values.publicUrls) | quote }}
"#
    .to_string()
}

fn deployment_tpl() -> String {
    r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "alien.fullname" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
spec:
  replicas: {{ .Values.runtime.replicas }}
  # Recreate guarantees exactly one agent runs at any time, so the
  # InstanceLock is never contended — even during a self-update.
  strategy:
    type: Recreate
  progressDeadlineSeconds: {{ .Values.runtime.progressDeadlineSeconds | default 120 }}
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "alien.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
  template:
    metadata:
      labels:
        {{- include "alien.labels" . | nindent 8 }}
        {{- with .Values.runtime.podLabels }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      {{- with .Values.runtime.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "alien.managerServiceAccountName" . }}
      automountServiceAccountToken: {{ .Values.runtime.automountServiceAccountToken }}
      securityContext:
        {{- toYaml .Values.runtime.security.podSecurityContext | nindent 8 }}
      {{- with .Values.runtime.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.topologySpreadConstraints }}
      topologySpreadConstraints:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- if .Values.runtime.scheduling.priorityClassName }}
      priorityClassName: {{ .Values.runtime.scheduling.priorityClassName | quote }}
      {{- end }}
      {{- if .Values.runtime.scheduling.runtimeClassName }}
      runtimeClassName: {{ .Values.runtime.scheduling.runtimeClassName | quote }}
      {{- end }}
      containers:
        - name: agent
          image: "{{ .Values.runtime.image.repository }}:{{ .Values.runtime.image.tag }}"
          imagePullPolicy: {{ .Values.runtime.image.pullPolicy }}
          securityContext:
            {{- toYaml .Values.runtime.security.containerSecurityContext | nindent 12 }}
          env:
            - name: PLATFORM
              value: kubernetes
            {{- if .Values.basePlatform }}
            - name: BASE_PLATFORM
              value: {{ .Values.basePlatform | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "gcp") .Values.basePlatformConfig.gcp.projectId }}
            - name: GCP_PROJECT_ID
              value: {{ .Values.basePlatformConfig.gcp.projectId | quote }}
            - name: GOOGLE_CLOUD_PROJECT
              value: {{ .Values.basePlatformConfig.gcp.projectId | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "gcp") .Values.basePlatformConfig.gcp.region }}
            - name: GCP_REGION
              value: {{ .Values.basePlatformConfig.gcp.region | quote }}
            {{- end }}
            # `required` chart guardrail: any helm upgrade that does not
            # carry the full values document fails to render (Helm aborts
            # before touching the release). This is the protection against
            # bare `helm upgrade` silently resetting the agent's manager
            # config — manager-triggered, operator-triggered, or otherwise.
            - name: SYNC_URL
              value: {{ required "management.url is required — pass the full values document" .Values.management.url | quote }}
            - name: AGENT_NAME
              value: {{ required "management.name is required — pass the full values document" .Values.management.name | quote }}
            {{- if .Values.management.deploymentId }}
            - name: DEPLOYMENT_ID
              value: {{ .Values.management.deploymentId | quote }}
            {{- end }}
            - name: KUBERNETES_NAMESPACE
              value: {{ .Release.Namespace | quote }}
            - name: KUBERNETES_HELM_RELEASE
              value: {{ .Release.Name | quote }}
            - name: ALIEN_AGENT_UPGRADER_SA
              value: {{ include "alien.upgraderServiceAccountName" . | quote }}
            # Reported back on /v1/sync so the dashboard can surface the
            # registry an admin will pull a new tag from when pinning a
            # target agent version.
            - name: ALIEN_AGENT_IMAGE_REPOSITORY
              value: {{ .Values.runtime.image.repository | quote }}
            {{- if .Values.runtime.upgrade.chartRef }}
            # Used by the agent when it receives `agent_target.helm` and
            # spawns a Helm-runner Job to apply the new version. The Job
            # runs `helm upgrade --reuse-values` against this chart so only
            # the manager-supplied `values` override (e.g. image.tag) flips.
            - name: ALIEN_AGENT_CHART_REF
              value: {{ .Values.runtime.upgrade.chartRef | quote }}
            {{- end }}
            {{- if .Values.runtime.upgrade.chartVersion }}
            - name: ALIEN_AGENT_CHART_VERSION
              value: {{ .Values.runtime.upgrade.chartVersion | quote }}
            {{- end }}
            {{- if .Values.runtime.upgrade.helmRunnerImage }}
            - name: ALIEN_AGENT_HELM_RUNNER_IMAGE
              value: {{ .Values.runtime.upgrade.helmRunnerImage | quote }}
            {{- end }}
            {{- if .Values.runtime.upgrade.extraArgs }}
            # Extra flags spliced into the `helm upgrade` command the
            # agent's helm-runner Job runs. Use sparingly — exists for
            # local-dev/insecure OCI registries (`--plain-http`) and
            # similar one-off escape hatches; production should leave empty.
            - name: ALIEN_AGENT_HELM_EXTRA_ARGS
              value: {{ .Values.runtime.upgrade.extraArgs | quote }}
            {{- end }}
            {{- if .Values.serviceAccountPrefix }}
            # Pin the deployment's resource_prefix to the same value used for
            # ServiceAccount naming, so Helm-created SAs and vault secret names
            # stay aligned across agent restarts (pull-model storage is ephemeral
            # and the agent would otherwise regenerate a random prefix each time).
            - name: ALIEN_RESOURCE_PREFIX
              value: {{ .Values.serviceAccountPrefix | quote }}
            {{- end }}
            - name: SYNC_TOKEN_FILE
              value: /etc/alien/secrets/sync-token
            - name: AGENT_ENCRYPTION_KEY_FILE
              value: /etc/alien/secrets/encryption-key
            - name: STACK_SETTINGS_FILE
              value: /etc/alien/config/stack-settings.json
            - name: PUBLIC_URLS_FILE
              value: /etc/alien/config/public-urls.json
            {{- if .Values.infrastructure }}
            - name: EXTERNAL_BINDINGS_FILE
              value: /etc/alien/secrets/external-bindings.json
            {{- end }}
            - name: SYNC_INTERVAL
              value: "30"
            - name: OTLP_PORT
              value: {{ .Values.runtime.api.port | quote }}
            - name: OTLP_HOST
              value: {{ .Values.runtime.api.bindHost | quote }}
          ports:
            - name: otlp
              containerPort: {{ .Values.runtime.api.port }}
          {{- if .Values.runtime.probes.liveness.enabled }}
          livenessProbe:
            httpGet:
              path: {{ .Values.runtime.probes.liveness.path | quote }}
              port: otlp
            initialDelaySeconds: {{ .Values.runtime.probes.liveness.initialDelaySeconds }}
            periodSeconds: {{ .Values.runtime.probes.liveness.periodSeconds }}
            timeoutSeconds: {{ .Values.runtime.probes.liveness.timeoutSeconds }}
            failureThreshold: {{ .Values.runtime.probes.liveness.failureThreshold }}
          {{- end }}
          {{- if .Values.runtime.probes.readiness.enabled }}
          readinessProbe:
            httpGet:
              path: {{ .Values.runtime.probes.readiness.path | quote }}
              port: otlp
            initialDelaySeconds: {{ .Values.runtime.probes.readiness.initialDelaySeconds }}
            periodSeconds: {{ .Values.runtime.probes.readiness.periodSeconds }}
            timeoutSeconds: {{ .Values.runtime.probes.readiness.timeoutSeconds }}
            failureThreshold: {{ .Values.runtime.probes.readiness.failureThreshold }}
          {{- end }}
          volumeMounts:
            - name: config
              mountPath: /etc/alien/config
              readOnly: true
            - name: management-token
              mountPath: /etc/alien/secrets/sync-token
              subPath: sync-token
              readOnly: true
            - name: encryption-key
              mountPath: /etc/alien/secrets/encryption-key
              subPath: {{ include "alien.encryptionSecretKey" . }}
              readOnly: true
            {{- if .Values.infrastructure }}
            - name: external-bindings
              mountPath: /etc/alien/secrets/external-bindings.json
              subPath: external-bindings.json
              readOnly: true
            {{- end }}
            {{- if .Values.runtime.tmp.enabled }}
            - name: tmp
              mountPath: /tmp
            {{- end }}
            - name: runtime-data
              mountPath: {{ .Values.runtime.data.mountPath | quote }}
          resources:
            {{- toYaml .Values.runtime.resources | nindent 12 }}
      volumes:
        - name: config
          configMap:
            name: {{ include "alien.fullname" . }}
        - name: management-token
          secret:
            secretName: {{ include "alien.managementSecretName" . }}
            items:
              - key: {{ include "alien.managementSecretTokenKey" . }}
                path: sync-token
            defaultMode: 384
        - name: encryption-key
          secret:
            secretName: {{ include "alien.encryptionSecretName" . }}
            defaultMode: 384
        {{- if .Values.infrastructure }}
        - name: external-bindings
          secret:
            secretName: {{ include "alien.fullname" . }}
            items:
              - key: external-bindings.json
                path: external-bindings.json
            defaultMode: 384
        {{- end }}
        {{- if .Values.runtime.tmp.enabled }}
        - name: tmp
          emptyDir:
            sizeLimit: {{ .Values.runtime.tmp.sizeLimit | quote }}
        {{- end }}
        - name: runtime-data
          {{- if .Values.runtime.data.persistence.enabled }}
          persistentVolumeClaim:
            claimName: {{ default (printf "%s-runtime-data" (include "alien.fullname" .)) .Values.runtime.data.persistence.existingClaim }}
          {{- else }}
          emptyDir: {}
          {{- end }}
"#
    .to_string()
}

fn service_tpl() -> String {
    r#"{{- if .Values.runtime.api.enabled }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "alien.fullname" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
spec:
  type: {{ .Values.runtime.api.service.type }}
  selector:
    app.kubernetes.io/name: {{ include "alien.name" . }}
    app.kubernetes.io/instance: {{ .Release.Name }}
  ports:
    - name: http
      port: {{ .Values.runtime.api.port }}
      targetPort: otlp
{{- end }}
"#
    .to_string()
}

fn pvc_tpl() -> String {
    r#"{{- if and .Values.runtime.data.persistence.enabled (not .Values.runtime.data.persistence.existingClaim) }}
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: {{ printf "%s-runtime-data" (include "alien.fullname" .) }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
spec:
  accessModes:
    {{- toYaml .Values.runtime.data.persistence.accessModes | nindent 4 }}
  {{- if .Values.runtime.data.persistence.storageClassName }}
  storageClassName: {{ .Values.runtime.data.persistence.storageClassName | quote }}
  {{- end }}
  resources:
    requests:
      storage: {{ .Values.runtime.data.persistence.size | quote }}
{{- end }}
"#
    .to_string()
}

fn poddisruptionbudget_tpl() -> String {
    r#"{{- if .Values.runtime.pdb.enabled }}
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: {{ include "alien.fullname" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
spec:
  {{- if hasKey .Values.runtime.pdb "maxUnavailable" }}
  maxUnavailable: {{ .Values.runtime.pdb.maxUnavailable }}
  {{- else }}
  minAvailable: {{ .Values.runtime.pdb.minAvailable }}
  {{- end }}
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "alien.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}
"#
    .to_string()
}

fn networkpolicy_tpl() -> String {
    r#"{{- if .Values.runtime.networkPolicy.enabled }}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ include "alien.fullname" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: {{ include "alien.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
  policyTypes:
    {{- if .Values.runtime.networkPolicy.ingress.enabled }}
    - Ingress
    {{- end }}
    {{- if .Values.runtime.networkPolicy.egress.enabled }}
    - Egress
    {{- end }}
  {{- if .Values.runtime.networkPolicy.ingress.enabled }}
  ingress:
    - {}
  {{- end }}
  {{- if .Values.runtime.networkPolicy.egress.enabled }}
  egress:
    - {}
  {{- end }}
{{- end }}
"#
    .to_string()
}

fn app_service_tpl() -> String {
    r#"{{- range $id, $service := .Values.services }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "alien.resourceName" (dict "root" $ "name" $id) }}
  labels:
    {{- include "alien.labels" $ | nindent 4 }}
    alien.dev/resource-id: {{ $id | quote }}
spec:
  type: {{ if eq $service.type "loadBalancer" }}LoadBalancer{{ else }}ClusterIP{{ end }}
  selector:
    app: {{ include "alien.resourceName" (dict "root" $ "name" $id) }}
    managed-by: alien
    component: {{ $service.component | quote }}
  ports:
    - name: http
      port: {{ default 80 $service.port }}
      targetPort: {{ default 8080 $service.targetPort }}
---
{{- end }}
"#
    .to_string()
}

fn cluster_bootstrap_tpl() -> String {
    r#"{{- $bootstrap := default dict .Values.clusterBootstrap -}}
{{- $storage := dig "storageClass" "default" dict $bootstrap -}}
{{- if dig "enabled" false $storage }}
{{- $storageName := required "clusterBootstrap.storageClass.default.name is required when enabled" $storage.name -}}
{{- $provisioner := required "clusterBootstrap.storageClass.default.provisioner is required when enabled" $storage.provisioner -}}
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: {{ $storageName | quote }}
  annotations:
    storageclass.kubernetes.io/is-default-class: "true"
  labels:
    {{- include "alien.labels" . | nindent 4 }}
provisioner: {{ $provisioner | quote }}
{{ with $storage.parameters }}
parameters:
  {{ range $key, $value := . }}
  {{ $key }}: {{ $value | quote }}
  {{ end }}
{{ end }}
reclaimPolicy: Delete
volumeBindingMode: WaitForFirstConsumer
allowVolumeExpansion: true
{{ end }}
{{- $eksAlb := dig "ingress" "eksAutoMode" dict $bootstrap -}}
{{- if dig "enabled" false $eksAlb }}
{{- $ingressClassName := required "clusterBootstrap.ingress.eksAutoMode.name is required when enabled" $eksAlb.name -}}
{{- $controller := required "clusterBootstrap.ingress.eksAutoMode.controller is required when enabled" $eksAlb.controller -}}
---
apiVersion: eks.amazonaws.com/v1
kind: IngressClassParams
metadata:
  name: {{ $ingressClassName | quote }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
spec:
  scheme: {{ default "internet-facing" $eksAlb.scheme | quote }}
  {{ with $eksAlb.subnetIds }}
  subnets:
    ids:
      {{ range . }}
      - {{ . | quote }}
      {{ end }}
  {{ end }}
---
apiVersion: networking.k8s.io/v1
kind: IngressClass
metadata:
  name: {{ $ingressClassName | quote }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
spec:
  controller: {{ $controller | quote }}
  parameters:
    apiGroup: eks.amazonaws.com
    kind: IngressClassParams
    name: {{ $ingressClassName | quote }}
{{ end }}
{{- $eksArm64NodePool := dig "compute" "eksAutoMode" "arm64NodePool" dict $bootstrap -}}
{{- if dig "enabled" false $eksArm64NodePool }}
{{- $nodePoolName := required "clusterBootstrap.compute.eksAutoMode.arm64NodePool.name is required when enabled" $eksArm64NodePool.name -}}
{{- $nodeClassName := required "clusterBootstrap.compute.eksAutoMode.arm64NodePool.nodeClassName is required when enabled" $eksArm64NodePool.nodeClassName -}}
---
apiVersion: karpenter.sh/v1
kind: NodePool
metadata:
  name: {{ $nodePoolName | quote }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
spec:
  template:
    spec:
      nodeClassRef:
        group: eks.amazonaws.com
        kind: NodeClass
        name: {{ $nodeClassName | quote }}
      requirements:
        - key: karpenter.sh/capacity-type
          operator: In
          values:
            - {{ default "on-demand" $eksArm64NodePool.capacityType | quote }}
        - key: kubernetes.io/arch
          operator: In
          values:
            - "arm64"
        - key: eks.amazonaws.com/instance-category
          operator: In
          values:
            {{ range (default (list "c" "m" "r") $eksArm64NodePool.instanceCategories) }}
            - {{ . | quote }}
            {{ end }}
        - key: eks.amazonaws.com/instance-generation
          operator: Gt
          values:
            - {{ default "5" $eksArm64NodePool.minInstanceGeneration | quote }}
  {{ with $eksArm64NodePool.limits }}
  limits:
    {{ with .cpu }}
    cpu: {{ . | quote }}
    {{ end }}
    {{ with .memory }}
    memory: {{ . | quote }}
    {{ end }}
  {{ end }}
{{ end }}
{{- $metrics := dig "metricsServer" dict $bootstrap -}}
{{- if dig "enabled" false $metrics }}
---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: metrics-server
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "alien.labels" . | nindent 4 }}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: system:aggregated-metrics-reader
  labels:
    k8s-app: metrics-server
    rbac.authorization.k8s.io/aggregate-to-admin: "true"
    rbac.authorization.k8s.io/aggregate-to-edit: "true"
    rbac.authorization.k8s.io/aggregate-to-view: "true"
    {{- include "alien.labels" . | nindent 4 }}
rules:
  - apiGroups: ["metrics.k8s.io"]
    resources: ["pods", "nodes"]
    verbs: ["get", "list", "watch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: system:metrics-server
  labels:
    k8s-app: metrics-server
    {{- include "alien.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["nodes/metrics"]
    verbs: ["get"]
  - apiGroups: [""]
    resources: ["pods", "nodes"]
    verbs: ["get", "list", "watch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: metrics-server-auth-reader
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "alien.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: extension-apiserver-authentication-reader
subjects:
  - kind: ServiceAccount
    name: metrics-server
    namespace: kube-system
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: metrics-server:system:auth-delegator
  labels:
    k8s-app: metrics-server
    {{- include "alien.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: system:auth-delegator
subjects:
  - kind: ServiceAccount
    name: metrics-server
    namespace: kube-system
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: system:metrics-server
  labels:
    k8s-app: metrics-server
    {{- include "alien.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: system:metrics-server
subjects:
  - kind: ServiceAccount
    name: metrics-server
    namespace: kube-system
---
apiVersion: v1
kind: Service
metadata:
  name: metrics-server
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "alien.labels" . | nindent 4 }}
spec:
  selector:
    k8s-app: metrics-server
  ports:
    - name: https
      port: 443
      protocol: TCP
      targetPort: https
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: metrics-server
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "alien.labels" . | nindent 4 }}
spec:
  selector:
    matchLabels:
      k8s-app: metrics-server
  template:
    metadata:
      labels:
        k8s-app: metrics-server
    spec:
      serviceAccountName: metrics-server
      containers:
        - name: metrics-server
          image: {{ default "registry.k8s.io/metrics-server/metrics-server:v0.8.1" $metrics.image | quote }}
          imagePullPolicy: IfNotPresent
          args:
            - --cert-dir=/tmp
            - --secure-port=10250
            - --kubelet-preferred-address-types=InternalIP,ExternalIP,Hostname
            - --kubelet-use-node-status-port
            - --metric-resolution=15s
          ports:
            - name: https
              containerPort: 10250
              protocol: TCP
          livenessProbe:
            httpGet:
              path: /livez
              port: https
              scheme: HTTPS
            initialDelaySeconds: 10
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /readyz
              port: https
              scheme: HTTPS
            initialDelaySeconds: 20
            periodSeconds: 10
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            runAsNonRoot: true
            runAsUser: 1000
          volumeMounts:
            - name: tmp
              mountPath: /tmp
      volumes:
        - name: tmp
          emptyDir: {}
---
apiVersion: apiregistration.k8s.io/v1
kind: APIService
metadata:
  name: v1beta1.metrics.k8s.io
  labels:
    k8s-app: metrics-server
    {{- include "alien.labels" . | nindent 4 }}
spec:
  service:
    name: metrics-server
    namespace: kube-system
  group: metrics.k8s.io
  version: v1beta1
  insecureSkipTLSVerify: true
  groupPriorityMinimum: 100
  versionPriority: 100
{{- end }}
"#
    .to_string()
}

fn eks_values_example(analysis: &ChartAnalysis) -> String {
    cloud_identity_example(
        analysis,
        "eks.amazonaws.com/role-arn",
        "arn:aws:iam::123456789012:role",
    )
}

fn gke_values_example(analysis: &ChartAnalysis) -> String {
    cloud_identity_example(
        analysis,
        "iam.gke.io/gcp-service-account",
        "deployment@project.iam.gserviceaccount.com",
    )
}

fn aks_values_example(analysis: &ChartAnalysis) -> String {
    cloud_identity_example(
        analysis,
        "azure.workload.identity/client-id",
        "00000000-0000-0000-0000-000000000000",
    )
}

fn onprem_values_example(analysis: &ChartAnalysis) -> String {
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
    let mut yaml = manager_fetch_example_values();
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

fn manager_fetch_example_values() -> String {
    r#"management:
  token: "dg_replace_me"
  name: "production"
  url: "https://manager.example.com"
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
  url: "https://manager.example.com"
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

fn readme_md(chart_name: &str, stack: &Stack) -> String {
    format!(
        "# {chart_name}\n\nInstall this chart into an existing Kubernetes cluster:\n\n```bash\nhelm install {chart_name} ./{} --namespace production --create-namespace --values values.yaml\n```\n\nThe generated `values.yaml` contains placeholders for management, service-account identity annotations, agent-local infrastructure bindings, and the Kubernetes exposure profile. The chart no longer renders per-app public `Ingress` objects from `services.*.host` or hostless ingress values; public endpoints are runtime-owned through `stackSettings.kubernetes.exposure`.\n\nSee `examples/<target>.yaml` for ready-to-use values matching EKS / GKE / AKS / on-prem.\n",
        stack.id()
    )
}

fn sanitize_chart_name(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            ch.to_ascii_lowercase()
        } else if !last_dash {
            last_dash = true;
            '-'
        } else {
            continue;
        };
        out.push(next);
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "alien-deployment".to_string()
    } else {
        out.chars()
            .take(63)
            .collect::<String>()
            .trim_matches('-')
            .to_string()
    }
}

fn yaml_key(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        value.to_string()
    } else {
        yaml_string(value)
    }
}

fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn ensure_trailing_newline(mut value: String) -> String {
    if !value.ends_with('\n') {
        value.push('\n');
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        PermissionProfile, Queue, ResourceLifecycle, Storage, WorkerCode, WorkerTrigger,
    };

    fn sample_stack() -> Stack {
        let storage = Storage::new("assets".to_string()).versioning(true).build();
        let queue = Queue::new("jobs".to_string()).build();
        let function = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "example.com/api:1".to_string(),
            })
            .permissions("runtime".to_string())
            .ingress(Ingress::Public)
            .link(&storage)
            .trigger(WorkerTrigger::queue(&queue))
            .build();

        Stack::new("sample-stack".to_string())
            .permission(
                "runtime",
                PermissionProfile::new().global(["storage/data-read"]),
            )
            .add(storage, ResourceLifecycle::Frozen)
            .add(queue, ResourceLifecycle::Frozen)
            .add(function, ResourceLifecycle::Live)
            .build()
    }

    #[test]
    fn generated_chart_lints_and_templates() {
        let registry = HelmRegistry::built_in();
        let chart = generate_helm_chart(
            &sample_stack(),
            HelmOptions {
                registry: &registry,
                stack_settings: StackSettings::default(),
                chart_name: "sample-stack".to_string(),
            },
        )
        .expect("chart should render");

        let files = chart.files.clone();
        crate::test_utils::helm_lint(&files).assert_ok("helm chart");
        // Manager-fetch path: a token + deploymentId are required (the chart
        // refuses to install without them — guards against half-configured
        // values).
        let manager_fetch_values = r#"
management:
  url: "https://manager.example.com"
  name: "test-manager"
  token: "test-sync-token"
  deploymentId: "test-deployment-id"
"#;
        crate::test_utils::helm_template_and_validate(&files, Some(manager_fetch_values))
            .assert_ok("helm template manager-fetch path");
        crate::test_utils::helm_template_and_validate(&files, Some(&files["examples/onprem.yaml"]))
            .assert_ok("helm template external-bindings initialize path");
    }

    #[test]
    fn fullname_defaults_to_release_name() {
        let helpers = helpers_tpl();

        assert!(helpers.contains("{{- .Release.Name | trunc 63 | trimSuffix \"-\" -}}"));
        assert!(!helpers.contains("printf \"%s-%s\" .Release.Name"));
    }
}
