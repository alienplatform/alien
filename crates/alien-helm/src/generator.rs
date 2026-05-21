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
    import::EmitContext, ErrorData, Ingress, Platform, ResourceLifecycle, Result, Stack,
    StackSettings, Worker,
};
use alien_error::{Context, IntoAlienError};
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
    files.insert("values.yaml".to_string(), values_yaml(&analysis));
    files.insert("values.schema.json".to_string(), values_schema_json());
    files.insert("templates/_helpers.tpl".to_string(), helpers_tpl());
    files.insert(
        "templates/serviceaccount.yaml".to_string(),
        serviceaccount_tpl(),
    );
    files.insert("templates/role.yaml".to_string(), role_tpl());
    files.insert("templates/rolebinding.yaml".to_string(), rolebinding_tpl());
    files.insert("templates/secret.yaml".to_string(), secret_tpl());
    files.insert("templates/configmap.yaml".to_string(), configmap_tpl());
    files.insert("templates/deployment.yaml".to_string(), deployment_tpl());
    files.insert("templates/service.yaml".to_string(), service_tpl());

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
                service_accounts.insert(function.permissions.clone());
                if function.ingress == Ingress::Public {
                    analysis.services.push(ServiceValue {
                        id: resource_id.clone(),
                    });
                }
            }
            if let Some(build) = entry.config.downcast_ref::<alien_core::Build>() {
                service_accounts.insert(build.permissions.clone());
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

#[derive(Debug)]
struct ServiceValue {
    id: String,
}

fn chart_yaml(chart_name: &str, stack: &Stack) -> String {
    format!(
        "apiVersion: v2\nname: {chart_name}\ndescription: Deployment chart for {stack_id}\ntype: application\nversion: 0.1.0\nappVersion: \"0.1.0\"\n",
        stack_id = stack.id()
    )
}

fn values_yaml(analysis: &ChartAnalysis) -> String {
    let mut yaml = String::new();
    yaml.push_str(
        r#"management:
  token: ""
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
  # Optional 64-character hex key. If empty, Helm generates one at install time.
  encryptionKey: ""
  replicas: 1
  resources:
    requests:
      cpu: 100m
      memory: 128Mi
    limits:
      memory: 512Mi
  api:
    enabled: false
    port: 8080
    service:
      type: ClusterIP

"#,
    );

    append_service_accounts(&mut yaml, analysis);
    yaml.push_str("\nstackSettings: null\n\ninfrastructure: null\n");
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
    yaml
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
                "  {}:\n    type: clusterIp\n    port: 80\n    targetPort: 8080\n    host: \"\"\n    tls:\n      enabled: false\n      secretName: \"\"\n    ingress:\n      className: \"\"\n      annotations: {{}}\n",
                yaml_key(&service.id)
            ));
        }
    }
}

fn values_schema_json() -> String {
    r#"{
  "$schema": "https://json-schema.org/draft-07/schema#",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "management": {
      "type": "object",
      "additionalProperties": false,
      "required": ["token", "updates", "telemetry", "healthChecks"],
      "properties": {
        "token": { "type": "string" },
        "name": { "type": "string" },
        "url": { "type": "string" },
        "deploymentId": { "type": ["string", "null"] },
        "updates": { "type": "string", "enum": ["auto", "approval-required"] },
        "telemetry": { "type": "string", "enum": ["auto", "approval-required", "off"] },
        "healthChecks": { "type": "string", "enum": ["on", "off"] }
      }
    },
    "runtime": { "type": "object" },
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
    "services": { "type": "object" },
    "publicUrls": { "type": "object", "additionalProperties": { "type": "string" } },
    "persistentStorage": { "type": "object" },
    "ephemeralStorage": { "type": "object" }
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
"#
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
{{- printf "%s-%s" .Release.Name (include "alien.name" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "alien.labels" -}}
app.kubernetes.io/name: {{ include "alien.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
{{- end -}}

{{- define "alien.managerServiceAccountName" -}}
{{ include "alien.fullname" . }}-manager
{{- end -}}

{{- define "alien.serviceAccountName" -}}
{{- printf "%s-%s" (include "alien.fullname" .root) .name | trunc 63 | trimSuffix "-" -}}
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
"#
    .to_string()
}

fn role_tpl() -> String {
    r#"apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "alien.fullname" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["configmaps", "secrets", "services", "pods", "pods/log", "persistentvolumeclaims"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["apps"]
    resources: ["deployments", "statefulsets"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["batch"]
    resources: ["jobs"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["ingresses", "networkpolicies"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
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
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "alien.fullname" . }}
"#
    .to_string()
}

fn secret_tpl() -> String {
    r#"apiVersion: v1
kind: Secret
metadata:
  name: {{ include "alien.fullname" . }}
  labels:
    {{- include "alien.labels" . | nindent 4 }}
type: Opaque
stringData:
  sync-token: {{ .Values.management.token | quote }}
  {{- $existingSecret := lookup "v1" "Secret" .Release.Namespace (include "alien.fullname" .) }}
  {{- if .Values.runtime.encryptionKey }}
  encryption-key: {{ .Values.runtime.encryptionKey | quote }}
  {{- else if and $existingSecret (index $existingSecret.data "encryption-key") }}
  encryption-key: {{ index $existingSecret.data "encryption-key" | b64dec | quote }}
  {{- else }}
  encryption-key: {{ randAlphaNum 64 | sha256sum | quote }}
  {{- end }}
  {{- if .Values.infrastructure }}
  external-bindings.json: {{ toJson .Values.infrastructure | quote }}
  {{- end }}
"#
    .to_string()
}

fn configmap_tpl() -> String {
    r#"{{- $defaultStackSettings := dict "deploymentModel" "pull" "updates" .Values.management.updates "telemetry" .Values.management.telemetry "heartbeats" .Values.management.healthChecks -}}
{{- $publicUrls := dict -}}
{{- range $id, $service := .Values.services -}}
{{- if $service.host -}}
{{- $_ := set $publicUrls $id (printf "https://%s" $service.host) -}}
{{- end -}}
{{- end -}}
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
  public-urls.json: {{ toJson (default $publicUrls .Values.publicUrls) | quote }}
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
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "alien.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
  template:
    metadata:
      labels:
        {{- include "alien.labels" . | nindent 8 }}
    spec:
      serviceAccountName: {{ include "alien.managerServiceAccountName" . }}
      containers:
        - name: agent
          image: "{{ .Values.runtime.image.repository }}:{{ .Values.runtime.image.tag }}"
          imagePullPolicy: {{ .Values.runtime.image.pullPolicy }}
          env:
            - name: PLATFORM
              value: kubernetes
            - name: SYNC_URL
              value: {{ .Values.management.url | quote }}
            - name: AGENT_NAME
              value: {{ .Values.management.name | quote }}
            {{- if .Values.management.deploymentId }}
            - name: DEPLOYMENT_ID
              value: {{ .Values.management.deploymentId | quote }}
            {{- end }}
            - name: KUBERNETES_NAMESPACE
              value: {{ .Release.Namespace | quote }}
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
          ports:
            - name: otlp
              containerPort: {{ .Values.runtime.api.port }}
          volumeMounts:
            - name: config
              mountPath: /etc/alien/config
              readOnly: true
            - name: secrets
              mountPath: /etc/alien/secrets
              readOnly: true
          resources:
            {{- toYaml .Values.runtime.resources | nindent 12 }}
      volumes:
        - name: config
          configMap:
            name: {{ include "alien.fullname" . }}
        - name: secrets
          secret:
            secretName: {{ include "alien.fullname" . }}
            defaultMode: 384
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
      targetPort: http
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
        "# {chart_name}\n\nInstall this chart into an existing Kubernetes cluster:\n\n```bash\nhelm install {chart_name} ./{} --namespace production --create-namespace --values values.yaml\n```\n\nThe generated `values.yaml` contains placeholders for management, service-account identity annotations, agent-local infrastructure bindings, and public service exposure. See `examples/<target>.yaml` for ready-to-use values matching EKS / GKE / AKS / on-prem.\n",
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
        crate::test_utils::helm_template_and_validate(&files, None)
            .assert_ok("helm template manager-fetch path");
        crate::test_utils::helm_template_and_validate(&files, Some(&files["examples/onprem.yaml"]))
            .assert_ok("helm template external-bindings initialize path");
    }
}
