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
    /// Compute backend for Container and Function resources.
    /// When None, the platform default is used (Horizon for cloud platforms).
    /// Contains cluster IDs and management tokens for container orchestration.
    /// Machine tokens are stored in environment_variables as built-in secret vars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_backend: Option<ComputeBackend>,
    /// External bindings for pre-existing services.
    /// Required for Kubernetes platform (all infrastructure resources).
    /// Optional for cloud platforms (override specific resources).
    #[serde(default)]
    pub external_bindings: ExternalBindings,
    /// Public URLs for exposed resources (optional override for all platforms).
    ///
    /// - **Kubernetes**: Pre-computed by Helm from services config (highly recommended)
    /// - **Cloud**: Optional override of domain_metadata or load balancer DNS
    /// - **Local**: Optional override of dynamic localhost URLs
    ///
    /// If not set, platforms determine public URLs from other sources:
    /// - Cloud: domain_metadata FQDN or load balancer DNS
    /// - Local: http://localhost:{allocated_port}
    /// - Kubernetes: None (unless provided by Helm)
    ///
    /// Key: resource ID, Value: public URL (e.g., "https://api.acme.com")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_urls: Option<HashMap<String, String>>,
    /// Domain metadata for auto-managed public resources (AWS/GCP/Azure).
    /// Contains certificate data for cloud provider import and renewal detection.
    /// Not used by Kubernetes (uses TLS Secrets) or Local (no TLS) platforms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_metadata: Option<DomainMetadata>,
    /// OTLP observability configuration for log export (optional).
    ///
    /// When set, alien-deployment injects OTEL_EXPORTER_OTLP_* env vars into
    /// container/function configs, and alien-infra embeds --otlp-logs-* flags
    /// into horizond VM startup scripts.
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
    /// Only Lambda (ECR) and Cloud Run (GAR) require native registry URIs.
    /// All other platforms pull through the manager's proxy.
    ///
    /// Derived by the manager from the artifact registry binding:
    /// - ECR: `{account_id}.dkr.ecr.{region}.amazonaws.com/{repository_prefix}`
    /// - GAR: `{region}-docker.pkg.dev/{project_id}/{repository_name}`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_image_host: Option<String>,
}

/// OTLP log export configuration for a deployment.
///
/// When set, all compute workloads (containers and horizond VM workers) export
/// their logs to the given endpoint via OTLP/HTTP.
///
/// The `logs_auth_header` is stored as plain text in DeploymentConfig because
/// alien-runtime reads `OTEL_EXPORTER_OTLP_HEADERS` at tracing-init time,
/// before vault secrets load. For horizond, the infra controller writes the
/// same value to the cloud vault (same pattern as the machine token) and the
/// startup script fetches it at boot via IAM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OtlpConfig {
    /// Full OTLP logs endpoint URL.
    /// Example: "https://<manager-host>/v1/logs"
    pub logs_endpoint: String,
    /// Auth header value in "key=value,..." format used for container OTLP env var injection.
    ///
    /// `alien-deployment` injects this as the `OTEL_EXPORTER_OTLP_HEADERS` plain env var
    /// into all containers. It must be plain (not a vault secret) because alien-runtime
    /// reads `OTEL_EXPORTER_OTLP_HEADERS` at tracing-init time, before vault secrets load.
    ///
    /// horizond VM workers do NOT use this field directly. The ContainerCluster infra
    /// controller writes the same value to the cloud vault (GCP: Secret Manager,
    /// AWS: Secrets Manager, Azure: Key Vault) and the startup script fetches it at
    /// boot via IAM -- the same pattern as the machine token.
    ///
    /// Example: "authorization=Bearer <write-token>"
    pub logs_auth_header: String,
    /// Full OTLP metrics endpoint URL (optional).
    /// When set, horizond exports its own VM/container orchestration metrics here.
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
}
