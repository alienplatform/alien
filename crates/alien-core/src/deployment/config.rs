//! Deployment configuration and OTLP observability settings.

use crate::{ExternalBindings, ManagementConfig, StackSettings};
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    is_false, ComputeBackend, DeletionPolicy, DomainMetadata, EnvironmentVariableType,
    EnvironmentVariablesSnapshot,
};

/// Deployment configuration
///
/// Configuration for how to perform the deployment.
/// Note: Credentials (ClientConfig) are passed separately to step() function.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentConfig {
    /// This invocation is an explicit setup run using the installer's credentials.
    /// Not serialized: a release or stored configuration cannot request an identity refresh.
    #[serde(skip)]
    #[cfg_attr(feature = "openapi", schema(ignore))]
    #[builder(default)]
    pub setup_run: bool,
    /// Identifies this setup run, set together with `setup_run`. Every setup run gets a new
    /// one, so a controller can tell a result recorded earlier in this run from one recorded in
    /// an earlier run.
    #[serde(skip)]
    #[cfg_attr(feature = "openapi", schema(ignore))]
    pub setup_run_id: Option<String>,
    /// Human-readable deployment name for cloud console metadata.
    ///
    /// This is separate from the physical resource prefix in StackState. It is
    /// used only for display text such as IAM role descriptions, service
    /// account descriptions, and custom role titles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_name: Option<String>,
    /// User-customizable deployment settings (network, deployment model, approvals).
    /// Provided by customer via CloudFormation, Terraform, CLI, or Helm.
    #[serde(default)]
    pub stack_settings: StackSettings,
    /// Platform service account/role that will manage the infrastructure remotely.
    /// Derived from Manager's ServiceAccount, not user-specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management_config: Option<ManagementConfig>,
    /// Registered installation identity for target federation setup.
    /// Callers derive this from registration, separately from display metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gcp_federation_scope: Option<crate::GcpFederationScope>,
    /// Environment variables snapshot
    pub environment_variables: EnvironmentVariablesSnapshot,
    /// Deployer-provided stack input values, keyed by input id. A resource
    /// gated with `.enabled(input)` and a Live lifecycle follows these
    /// values; a missing key falls back to the input's declared boolean
    /// default. Suppliers must not place secret-kind input values here:
    /// this map serializes and debug-prints unredacted.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    #[builder(default)]
    pub input_values: HashMap<String, serde_json::Value>,
    /// Values the manager holds for this deployment instead of the sender.
    /// For each slot the sender gives the kind and a keyed digest, never the
    /// value. The manager loads the value from its own store, refuses to run
    /// when a digest differs, and merges the value into `environment_variables`
    /// or `input_values` before the deployment steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub value_slots: Vec<ValueSlotRef>,
    /// IDs of applicable secret inputs stored for this exact deployment target.
    /// Trusted presence metadata only: never values, gate answers, or authority.
    /// Absent on legacy targets; an explicit empty list means no stored secrets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(nullable = false))]
    pub stored_secret_input_ids: Option<Vec<String>>,
    /// Allow frozen resource changes during updates
    /// When true, skips the frozen resources compatibility check.
    /// This requires running with elevated cloud credentials.
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_frozen_changes: bool,
    /// Compute backend for Container and Worker resources.
    /// When None, the platform default is used for cloud platforms.
    /// Contains cluster IDs and management tokens for container orchestration.
    /// Worker runtime credentials are provided through cloud identity and vault-backed secrets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_backend: Option<ComputeBackend>,
    /// External bindings for pre-existing services.
    /// Required for Kubernetes platform (all infrastructure resources).
    /// Optional for cloud platforms (override specific resources).
    #[serde(default)]
    pub external_bindings: ExternalBindings,
    /// Cloud platform that owns imported base infrastructure for a Kubernetes
    /// runtime deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_platform: Option<crate::Platform>,
    /// DNS-style label domain used for Kubernetes resource ownership labels.
    ///
    /// Defaults to `alien.dev` when absent. Whitelabeled Operator builds set this
    /// so generated workloads and optional log collectors share the same label
    /// namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_domain: Option<String>,
    /// Kubernetes label selector that narrows which raw resources the observe
    /// pass reports (e.g. `app.kubernetes.io/part-of=my-app`). `None` observes
    /// everything in the namespace. Ignored by cloud observers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observe_label_selector: Option<String>,
    /// When true the observe pass reports raw resources across every namespace
    /// (cluster scope); otherwise it stays within the operator's own namespace.
    /// The label selector, if any, still filters within whichever scope applies.
    /// Ignored by cloud observers.
    #[serde(default)]
    #[builder(default)]
    pub observe_all_namespaces: bool,
    /// Public endpoint URLs for exposed resources (optional override).
    ///
    /// Use this only when a caller already knows the public URL. Managed public
    /// endpoint flows should prefer `domain_metadata` plus controller-reported
    /// load balancer outputs so DNS, certificate renewal, and route readiness
    /// stay tied to the resource state.
    ///
    /// If not set, platforms determine public endpoint URLs from other sources:
    /// - Managed DNS/TLS flows: `domain_metadata` FQDN or load balancer DNS
    /// - Local: `http://localhost:{allocated_port}`
    /// - Custom or disabled exposure: no public endpoint URL unless a controller reports one
    ///
    /// Outer key: resource ID. Inner key: endpoint name. Value: public URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_endpoints: Option<HashMap<String, HashMap<String, String>>>,
    /// Domain metadata for auto-managed public resources.
    ///
    /// Contains generated hostnames, DNS record state, certificate material,
    /// and renewal markers for platforms that use managed public endpoints.
    /// Kubernetes uses this only when its exposure mode is `generated`; BYO and
    /// disabled Kubernetes exposure do not receive managed domain metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_metadata: Option<DomainMetadata>,
    /// OTLP observability configuration for log export (optional).
    ///
    /// When set, worker runtimes export captured application logs through this
    /// endpoint. Container orchestrators may use it for their node-level log
    /// collectors, but app container configs must not receive the auth header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring: Option<OtlpConfig>,
    /// Manager base URL (e.g., "https://manager.alien.dev").
    ///
    /// The manager IS the container registry — its `/v2/` endpoint serves as
    /// the OCI Distribution API. Controllers derive the proxy host from this
    /// to configure pull auth (RegistryCredentials, imagePullSecrets).
    ///
    /// When None (e.g., `alien dev`), controllers use image URIs as-is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manager_url: Option<String>,
    /// Exact audience for bounded runtime identity exchange. When present,
    /// runtimes must use identity exchange and cannot fall back to stored tokens.
    /// Absence preserves the legacy mode for installs awaiting migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_identity_audience: Option<String>,
    /// Deployment token for pull authentication with the manager's registry.
    ///
    /// Used by controllers to configure registry credentials so cloud platforms
    /// and K8s can pull images from the manager's `/v2/` endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_token: Option<String>,
    /// Native image registry host+prefix for platforms that require it.
    ///
    /// Lambda (ECR) and Cloud Run (GAR) require native registry URIs. Other
    /// runtimes, including Azure Container Apps, pull through the manager's
    /// registry proxy.
    ///
    /// Derived by the manager from the artifact registry binding:
    /// - ECR: `{account_id}.dkr.ecr.{region}.amazonaws.com/{repository_prefix}`
    /// - GAR: `{region}-docker.pkg.dev/{project_id}/{repository_name}`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_image_host: Option<String>,
    /// Operator requests to replace a replica's persistent volume with a new
    /// volume made from a snapshot. A container controller performs each
    /// request once, identified by its `request_id`, and reports it in
    /// `ContainerOutputs.volumes`. Only a volume that a controller reports in
    /// `ContainerOutputs.volumes` can be the target of a request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub volume_restores: Vec<VolumeRestoreRequest>,
    /// How the deletion this run performs treats data. Under `Erase`, controllers also
    /// delete what they would otherwise keep after the resource is gone, such as a
    /// container's final volume snapshots.
    /// Not serialized: deletion sets it from the policy persisted in runtime metadata,
    /// so a stored configuration cannot request an erase.
    #[serde(skip)]
    #[cfg_attr(feature = "openapi", schema(ignore))]
    #[builder(default)]
    pub deletion_policy: DeletionPolicy,
}

/// A value the manager holds, named by slot, with the digest the sender expects.
///
/// The slot is `env:NAME` for an environment variable, `input:ID` for a stack
/// input, or `cenv.<container>.<NAME>` for a dynamic container's variable. An
/// input's stored value is the JSON text of the input value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ValueSlotRef {
    /// `env:NAME`, `input:ID` or `cenv.<container>.<NAME>`.
    pub slot: String,
    /// Whether the value is a secret. The manager checks it against the stored kind.
    pub kind: EnvironmentVariableType,
    /// Keyed digest of the value, as lowercase hex.
    pub digest: String,
    /// Variable name, repeated for display. The slot decides the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_name: Option<String>,
    /// Target resource patterns for an environment variable. `None` means all resources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<String>>,
}

/// Replace one replica's persistent volume with a new volume made from a snapshot.
///
/// The controller stops the replica, snapshots the volume it is about to
/// replace (so the restore can be undone), creates the new volume in the same
/// zone, starts the replica on it, and deletes the replaced volume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct VolumeRestoreRequest {
    /// Unique ID of this request. A controller performs each request once.
    pub request_id: String,
    /// ID of the container resource that owns the volume
    pub resource_id: String,
    /// Replica ordinal whose volume is replaced
    pub ordinal: u32,
    /// Cloud ID of the snapshot to restore: an EBS snapshot ID, a Compute
    /// Engine snapshot name, or an Azure snapshot resource ID
    pub snapshot_id: String,
}

/// Resource-attribute key marking OTLP telemetry as Alien system-component
/// output (infrastructure daemons and internal runtimes) rather than user
/// workload. Log consumers — the CLI log viewer and the dashboard — hide
/// telemetry carrying this attribute by default. The value is the string
/// `"true"`.
///
/// System components set this on their own telemetry's resource attributes so
/// consumers can filter generically, without enumerating component names.
pub const ALIEN_SYSTEM_RESOURCE_ATTRIBUTE: &str = "alien.system";

/// Prefix that marks one captured output line of a Worker as Alien system output.
///
/// The worker runtime strips it from the line and records
/// [`ALIEN_SYSTEM_RESOURCE_ATTRIBUTE`] on the exported record. Alien components that run as a
/// Worker's application write it at the start of every line they print. Like the attribute, it
/// only hides a line by default; it grants nothing.
pub const ALIEN_SYSTEM_LOG_LINE_PREFIX: &str = "\u{1e}ALIEN_SYSTEM\u{1f}";

/// OTLP log export configuration for a deployment.
///
/// When set, injected compute runtimes export captured application logs
/// through the given endpoint via OTLP/HTTP; which resources are injected
/// is platform-dependent. Workers read auth headers from a runtime-only
/// secret. Runtime-less Containers and Daemons receive standard OTEL auth
/// variables only at the final hosting boundary: Local passes them directly
/// to the process and Kubernetes projects them from a per-workload Secret.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OtlpConfig {
    /// Full OTLP logs endpoint URL.
    /// Example: "https://<manager-host>/v1/logs"
    pub logs_endpoint: String,
    /// Auth header value in "key=value,..." format.
    /// Example: "authorization=Bearer <write-token>"
    pub logs_auth_header: String,
    /// Full OTLP metrics endpoint URL (optional).
    /// When set, the worker runtime exports its own VM/container orchestration metrics here.
    /// Example: "https://api.axiom.co/v1/metrics"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics_endpoint: Option<String>,
    /// Auth header value for the metrics endpoint in "key=value,..." format (optional).
    ///
    /// When absent, `logs_auth_header` is reused for metrics -- suitable when the same
    /// credential covers both signals. When present (e.g. Axiom with separate datasets),
    /// this value is used exclusively for metrics.
    ///
    /// Example: "authorization=Bearer <token>,x-axiom-dataset=<metrics-dataset>"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics_auth_header: Option<String>,
    /// Resource attributes attached to every OTLP signal emitted for this deployment.
    ///
    /// Platform managers use this for stable identity such as `alien.workspace_id`,
    /// `alien.project_id`, `alien.deployment_group_id`, and `alien.deployment_id`.
    /// Runtime-specific resource attributes such as `service.name` remain owned by
    /// the runtime/exporter.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub resource_attributes: HashMap<String, String>,
}

#[cfg(test)]
mod input_values_tests {
    use super::*;

    /// Older actors serialize configs without the field; it must deserialize
    /// to an empty map, and an empty map must not serialize at all.
    #[test]
    fn input_values_default_to_empty_and_stay_off_the_wire() {
        let json = serde_json::json!({
            "stackSettings": serde_json::to_value(crate::StackSettings::default()).unwrap(),
            "environmentVariables": {
                "variables": [],
                "hash": "hash",
                "createdAt": "2026-01-01T00:00:00Z",
            },
            "externalBindings": {},
        });
        let config: DeploymentConfig =
            serde_json::from_value(json).expect("old shape deserializes");
        assert!(config.input_values.is_empty());
        assert!(config.stored_secret_input_ids.is_none());

        let round = serde_json::to_value(&config).expect("serializes");
        assert!(round.get("storedSecretInputIds").is_none());
        assert!(
            round.get("inputValues").is_none(),
            "empty map must not serialize"
        );
    }
    #[test]
    fn stored_secret_presence_roundtrips_without_values_or_gate_answers() {
        let mut config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        assert!(config.stored_secret_input_ids.is_none());
        config.stored_secret_input_ids = Some(Vec::new());
        let empty = serde_json::to_value(&config).unwrap();
        assert_eq!(empty["storedSecretInputIds"], serde_json::json!([]));
        assert_eq!(
            serde_json::from_value::<DeploymentConfig>(empty)
                .unwrap()
                .stored_secret_input_ids,
            Some(Vec::new())
        );
        config.stored_secret_input_ids =
            Some(vec!["apiKey".to_string(), "enableFeature".to_string()]);
        let wire = serde_json::to_value(&config).unwrap();
        assert_eq!(
            wire["storedSecretInputIds"],
            serde_json::json!(["apiKey", "enableFeature"])
        );
        assert!(wire.get("inputValues").is_none());
        let decoded: DeploymentConfig = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            decoded.stored_secret_input_ids,
            config.stored_secret_input_ids
        );
        assert_eq!(serde_json::to_value(decoded.clone()).unwrap(), wire);
        assert!(decoded.input_values.is_empty());
        for default in [false, true] {
            let input: crate::StackInputDefinition = serde_json::from_value(serde_json::json!({
                "id": "enableFeature", "kind": "boolean", "providedBy": ["deployer"],
                "required": false, "label": "Enable feature", "description": "", "default": crate::StackInputDefaultValue::Boolean(default)
            }))
            .unwrap();
            assert_eq!(
                crate::gate_resolves_true(
                    &[input],
                    "enableFeature",
                    &decoded.input_values,
                    "worker",
                    crate::Platform::Aws
                )
                .unwrap(),
                default
            );
        }
        assert!(crate::gate_resolves_true(
            &[],
            "enableFeature",
            &decoded.input_values,
            "worker",
            crate::Platform::Aws
        )
        .is_err());
    }

    fn deployment_config_with_cluster(
        cluster_image: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .compute_backend(crate::ComputeBackend::Horizon(crate::HorizonConfig {
                url: "https://horizon.example.com".to_string(),
                horizon_machine_image: None,
                clusters: HashMap::from([(
                    "compute".to_string(),
                    crate::HorizonClusterConfig {
                        cluster_id: "cluster-1".to_string(),
                        management_token: "hm_test".to_string(),
                        runtime_identity: None,
                        horizon_machine_image: cluster_image
                            .map(|image| serde_json::from_value(image).unwrap()),
                    },
                )]),
            }))
            .build();
        serde_json::to_value(&config).unwrap()
    }

    fn machine_image(version: &str) -> serde_json::Value {
        serde_json::json!({
            "channel": "stable",
            "machineImageVersion": version,
            "horizondVersion": version,
            "gitSha": "abc123",
            "createdAt": "2026-01-01T00:00:00Z",
            "baseImage": { "name": "linux", "version": "1" },
            "horizondArtifacts": {}
        })
    }

    /// A deployment config written before clusters could carry a machine image, as managers
    /// receive and store it, reads back with no cluster image.
    #[test]
    fn deployment_config_without_cluster_machine_image_reads_as_before() {
        let wire = deployment_config_with_cluster(None);
        let cluster = &wire["computeBackend"]["clusters"]["compute"];
        assert_eq!(
            cluster,
            &serde_json::json!({ "clusterId": "cluster-1", "managementToken": "hm_test" })
        );

        let config: DeploymentConfig = serde_json::from_value(wire).unwrap();
        let Some(crate::ComputeBackend::Horizon(horizon)) = config.compute_backend else {
            panic!("the Horizon backend reads back");
        };
        assert_eq!(horizon.clusters["compute"].horizon_machine_image, None);
    }

    /// The types on the path from the deployment config to a cluster as they were before the
    /// cluster machine image existed: none of them rejects unknown fields.
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PreviousDeploymentConfig {
        compute_backend: Option<PreviousComputeBackend>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type", rename_all = "camelCase")]
    enum PreviousComputeBackend {
        Horizon(PreviousHorizonConfig),
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PreviousHorizonConfig {
        clusters: HashMap<String, PreviousHorizonClusterConfig>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct PreviousHorizonClusterConfig {
        cluster_id: String,
        management_token: String,
    }

    /// A manager built before the field reads a deployment config that sets it and ignores it.
    #[test]
    fn deployment_config_with_cluster_machine_image_reads_in_older_readers() {
        let wire = deployment_config_with_cluster(Some(machine_image("1")));
        assert!(wire["computeBackend"]["clusters"]["compute"]["horizonMachineImage"].is_object());

        let previous: PreviousDeploymentConfig = serde_json::from_value(wire.clone())
            .expect("an older reader must accept a cluster that carries its own image");
        let Some(PreviousComputeBackend::Horizon(horizon)) = previous.compute_backend else {
            panic!("the Horizon backend reads back");
        };
        assert_eq!(
            horizon.clusters["compute"],
            PreviousHorizonClusterConfig {
                cluster_id: "cluster-1".to_string(),
                management_token: "hm_test".to_string(),
            }
        );

        let current: DeploymentConfig = serde_json::from_value(wire).unwrap();
        let Some(crate::ComputeBackend::Horizon(horizon)) = current.compute_backend else {
            panic!("the Horizon backend reads back");
        };
        assert_eq!(
            horizon
                .machine_image_for("compute")
                .map(|image| image.machine_image_version.as_str()),
            Some("1")
        );
    }
}
