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
    Ingress, Platform, ResourceLifecycle, Result, Stack, StackSettings, Worker, WorkerCode,
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

"#,
    );

    append_service_accounts(&mut yaml, analysis);
    append_stack_settings(&mut yaml, stack_settings)?;
    yaml.push_str("\ninfrastructure: null\n\nbasePlatform: null\nserviceAccountPrefix: \"\"\nmanagerServiceAccount:\n  annotations: {}\n  labels: {}\n");
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
        "replicas": { "type": "integer", "minimum": 1 },
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
    "serviceAccountPrefix": { "type": "string" },
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
{{- end }}
"#
    .to_string()
}

fn secret_tpl() -> String {
    r#"{{- $createManagementSecret := not .Values.management.existingSecret.name -}}
{{- $createEncryptionSecret := not .Values.runtime.encryption.existingSecret.name -}}
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
  sync-token: {{ .Values.management.token | quote }}
  {{- end }}
  {{- if $createEncryptionSecret }}
  encryption-key: {{ required "runtime.encryption.key or runtime.encryption.existingSecret.name is required" .Values.runtime.encryption.key | quote }}
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
        crate::test_utils::helm_template_and_validate(&files, None)
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
