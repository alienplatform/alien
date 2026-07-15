//! Deployment configuration and OTLP observability settings.

use crate::{ExternalBindings, ManagementConfig, StackSettings};
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{is_false, ComputeBackend, DomainMetadata, EnvironmentVariablesSnapshot};

/// Deployment configuration
///
/// Configuration for how to perform the deployment.
/// Note: Credentials (ClientConfig) are passed separately to step() function.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentConfig {
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
    /// Environment variables snapshot
    pub environment_variables: EnvironmentVariablesSnapshot,
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
