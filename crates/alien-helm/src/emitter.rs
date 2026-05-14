//! Per-resource Helm emitter trait.

use alien_core::{import::EmitContext, Result};
use indexmap::IndexMap;

/// Entry contributed to `values.yaml`'s user-facing `infrastructure:` map.
///
/// The chart converts this map into the agent's local `ExternalBindings`
/// config; it is not manager ImportData.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfrastructureValue {
    /// Resource id from the stack — used as the map key under `infrastructure:`.
    pub id: String,
    /// Runtime binding family (`storage`, `queue`, `kv`, ...).
    pub binding_type: String,
    /// Sub-service this resource binds to (e.g. `s3`, `sqs`, `redis`).
    pub service: String,
    /// Sub-fields placed under the entry. Iteration order matches
    /// emitter contribution order so reviewers see fields in a stable
    /// shape.
    pub fields: IndexMap<String, String>,
}

/// Cloud-identity annotation set contributed to a generated
/// `ServiceAccount` template by a target overlay (e.g. EKS / GKE / AKS).
/// Returned by emitters as a hint, but the actual annotations land in
/// the `examples/<target>.yaml` file the customer copies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceAccountIdentity {
    /// Annotation key — `eks.amazonaws.com/role-arn` /
    /// `iam.gke.io/gcp-service-account` / `azure.workload.identity/client-id`.
    pub annotation_key: String,
    /// Annotation value template the customer fills in.
    pub annotation_value: String,
}

/// Helm fragment emitted by a single `(resource_type, platform)` emitter.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HelmFragment {
    /// `infrastructure.<id>` value contribution (when this resource needs
    /// a runtime binding).
    pub infrastructure: Option<InfrastructureValue>,
    /// Extra `templates/<path>` files to add to the chart (e.g. a
    /// per-resource ConfigMap or NetworkPolicy). Keyed by relative path
    /// inside the chart's `templates/` directory.
    pub extra_templates: IndexMap<String, String>,
}

impl HelmFragment {
    /// Empty fragment.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Builder helper.
    pub fn with_infrastructure(mut self, value: InfrastructureValue) -> Self {
        self.infrastructure = Some(value);
        self
    }
}

/// Generator-side trait — emit the per-resource Helm contribution.
pub trait HelmEmitter: Send + Sync {
    /// Emit the chart fragment for this resource.
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<HelmFragment>;
}
