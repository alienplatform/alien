//! Sync protocol types for operator ↔ manager communication.
//!
//! The operator periodically calls `POST /v1/sync` with a `SyncRequest` and
//! receives a `SyncResponse` containing the target deployment state (and, when a
//! self-update is pinned, an `operator_target`).

use serde::{Deserialize, Serialize};

use crate::{
    DeploymentConfig, DeploymentState, ObservedInventoryBatch, ReleaseInfo, ResourceHeartbeat,
};

/// State of an Operator capability as observed inside the environment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum OperatorCapabilityState {
    /// The Operator has the permission or local facility needed for the capability.
    Granted,
    /// The environment explicitly denied the capability.
    Denied,
    /// The capability does not apply in this environment.
    Unavailable,
}

/// Report-only Operator capability status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OperatorCapabilityReport {
    /// Stable capability key, such as `k8s-workloads` or `logs`.
    pub key: String,
    /// Whether the capability is currently usable.
    pub state: OperatorCapabilityState,
    /// Optional human-readable detail from the Operator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Request sent by the operator to the manager during periodic sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRequest {
    /// The deployment ID this operator is managing.
    pub deployment_id: String,
    /// Current deployment state as seen by the operator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_state: Option<DeploymentState>,
    /// Managed Alien resource status samples emitted by the Operator's deployment step.
    #[serde(
        default,
        rename = "resourceHeartbeats",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub heartbeats: Vec<ResourceHeartbeat>,
    /// Observed raw-resource inventory batches successfully read by the Operator.
    #[serde(
        default,
        rename = "observedInventoryBatches",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub observed_inventory_batches: Vec<ObservedInventoryBatch>,
    /// Report-only capabilities observed by the Operator.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<OperatorCapabilityReport>,
    /// Version of the Operator binary reporting this sync (from
    /// `env!("CARGO_PKG_VERSION")` at build time). Lets the manager build
    /// fleet-wide version inventory and decide whether to send an
    /// `operator_target`. Optional for back-compat with older operators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_version: Option<String>,
    /// Version of the `alien-launcher` supervising this operator (os-service
    /// packaging only; sourced from `ALIEN_LAUNCHER_VERSION`, which the launcher
    /// sets on spawn). Reported, never driven — the launcher is frozen and only
    /// changes via a state-preserving redeploy. The manager compares it against
    /// `OperatorBinaryTarget::min_launcher_version` and withholds targets the
    /// installed launcher is too old to actuate. None on Kubernetes and for
    /// operators not run under a launcher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher_version: Option<String>,
    /// Host OS the operator runs on. From `std::env::consts::OS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_os: Option<OperatorOs>,
    /// Host CPU architecture the operator runs on. From `std::env::consts::ARCH`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_arch: Option<OperatorArch>,
    /// How the operator is supervised — `os-service` (launcher) or `kubernetes`
    /// (Helm). Detected at runtime from `KUBERNETES_SERVICE_HOST`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packaging: Option<OperatorPackaging>,
    /// Container image repository the operator was pulled from (without the
    /// tag), e.g. `ghcr.io/alien-dev/alien-operator`. The chart injects this via
    /// `ALIEN_OPERATOR_IMAGE_REPOSITORY` (= `.Values.runtime.image.repository`),
    /// so admins can see the supply-chain link before pinning a new tag.
    /// Optional and Kubernetes-only — the os-service packaging fills the same role
    /// with its launcher manifest URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_image_repository: Option<String>,
    /// Outcome of the operator's in-flight self-update for the currently-pinned
    /// target, if any. Absent when no update is in flight. Lets the manager
    /// distinguish "still converging" from "the last attempt failed" instead of
    /// inferring failure from a stalled version. Optional for back-compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_update: Option<OperatorUpdateReport>,
}

/// Supervisor packaging for an operator. Drives which `operator_target` payload
/// (`binary` vs `helm`) the manager sends and how the operator actuates the
/// upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperatorPackaging {
    /// Native OS service — the launcher swaps the binary on disk.
    OsService,
    /// Kubernetes pod — operator creates a Helm-runner Job that runs `helm upgrade --atomic`.
    Kubernetes,
}

/// Host operating system an operator runs on.
///
/// Serialized values match `std::env::consts::OS`, so the manager can line the
/// operator up against the binaries it builds. Unsupported OSes are reported as
/// `None` (the operator can't self-update to a binary that doesn't exist) rather
/// than as a string the manager can't act on. Prefer this over
/// `instance_catalog::Architecture` / `build_targets::BinaryTarget`: those model
/// buildable *targets* (and spell arm as `arm64`), not the OS the operator
/// happens to run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperatorOs {
    Linux,
    Macos,
    Windows,
}

impl OperatorOs {
    /// Detect the current host OS. `None` for OSes we don't ship binaries for.
    pub fn detect() -> Option<Self> {
        Self::from_consts(std::env::consts::OS)
    }

    /// Map a `std::env::consts::OS` value to a supported OS.
    pub fn from_consts(os: &str) -> Option<Self> {
        match os {
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::Macos),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }

    /// Wire/string form (matches `std::env::consts::OS`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
        }
    }
}

/// Host CPU architecture an operator runs on.
///
/// Serialized values match `std::env::consts::ARCH` (`x86_64` / `aarch64`) —
/// deliberately not the `arm64` spelling `instance_catalog::Architecture` uses,
/// so it round-trips exactly what the operator reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorArch {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
}

impl OperatorArch {
    /// Detect the current host arch. `None` for arches we don't ship binaries for.
    pub fn detect() -> Option<Self> {
        Self::from_consts(std::env::consts::ARCH)
    }

    /// Map a `std::env::consts::ARCH` value to a supported architecture.
    pub fn from_consts(arch: &str) -> Option<Self> {
        match arch {
            "x86_64" => Some(Self::X86_64),
            "aarch64" => Some(Self::Aarch64),
            _ => None,
        }
    }

    /// Wire/string form (matches `std::env::consts::ARCH`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }
}

/// Operator-reported state of the current self-update attempt.
///
/// Success is deliberately not a variant — convergence
/// (`operator_version == target_operator_version`) is the success signal. This
/// report only distinguishes "an attempt is running" from "the last attempt
/// failed", so the manager can surface a truthful failure instead of inferring
/// one from a stalled version. Internally tagged by `state`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum OperatorUpdateReport {
    /// A Job for `target_version` is currently running (attempt `attempt`,
    /// 1-based). Reported so the dashboard can show "running" from the operator's
    /// own view rather than only "pinned".
    #[serde(rename_all = "camelCase")]
    InProgress {
        target_version: String,
        attempt: u32,
    },
    /// The most recent attempt for `target_version` failed; the operator is still
    /// on its prior version (rolled back / never swapped) and will back off and
    /// retry. Whether the *episode* is terminal is decided manager-side.
    #[serde(rename_all = "camelCase")]
    Failed {
        target_version: String,
        /// Which stage failed — see `OperatorUpdatePhase`.
        phase: OperatorUpdatePhase,
        /// Human-readable detail: helm error, image-pull reason, k8s API error.
        message: String,
        attempt: u32,
    },
}

/// Which stage of a self-update attempt failed. Maps to operator triage:
/// `Pull` → the image tag/registry; `Apply` → the chart/values/cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperatorUpdatePhase {
    /// Creating the Helm-runner Job / staging the binary failed — the operator
    /// knows directly (the k8s API rejected the create, or a download failed).
    Spawn,
    /// The new pod's image could not be pulled (ImagePullBackOff / ErrImagePull
    /// / not-found). Read from the Job pod's container `waiting.reason`.
    Pull,
    /// The image pulled, but `helm upgrade` failed or `--atomic` rolled back
    /// (or, on os-service, the launcher's health-gated swap rolled back).
    Apply,
}

/// Response from the manager to the operator sync request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResponse {
    /// Authoritative deployment state from the manager.
    ///
    /// Pull operators use this to hydrate local state when attaching to an
    /// already-imported deployment. Absent means the operator's local state is
    /// already authoritative or no state has been established yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_state: Option<DeploymentState>,
    /// Target deployment the operator should converge toward.
    /// None means no changes needed or this is an observe-only deployment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetDeployment>,
    /// Public URL for the commands API (e.g. `https://manager.example.com/v1`).
    /// Cloud-deployed workers use this to poll for pending commands.
    /// When absent, the operator falls back to its sync URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands_url: Option<String>,
    /// Desired operator self-update target. The operator acts on whichever
    /// payload matches its packaging: `binary` for `os-service`, `helm` for
    /// `kubernetes`. None means no upgrade pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_target: Option<OperatorTarget>,
}

/// Desired operator upgrade payload, sent by the manager on the sync exchange.
///
/// The operator reads `binary` or `helm` depending on its packaging. The manager is
/// the single source of truth for the target version per deployment per channel,
/// and on Kubernetes also for the full desired values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorTarget {
    /// Target operator version (e.g. "1.4.0").
    pub version: String,
    /// The operator should refuse the upgrade if its own version is older than
    /// this — used by the manager to enforce a floor on incremental migrations.
    pub min_supported_version: String,
    /// OS-service actuation payload. Present iff the deployment's packaging is `os-service`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<OperatorBinaryTarget>,
    /// Kubernetes actuation payload. Present iff the deployment's packaging is `kubernetes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helm: Option<OperatorHelmTarget>,
}

/// OS-service binary upgrade payload — the operator downloads, verifies the
/// SHA-256, stages the binary, and exits; the launcher performs the
/// health-gated swap.
///
/// The manager resolves the artifact for THIS host's `(os, arch)` — both are
/// known from the `SyncRequest` — and sends exactly one url + sha256 +
/// signature. (An earlier draft carried an artifacts map with a single sha256;
/// that was unsound — one digest cannot cover N different binaries — and
/// unnecessary, since the manager already knows the host.)
///
/// The `signature` field is **future work**: it rides along on the wire so
/// newer operators can enforce it once the signing infrastructure lands, but
/// the current launcher trusts SHA-256 + HTTPS for the download.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorBinaryTarget {
    /// Download URL for this host's `(os, arch)` — resolved by the manager
    /// from the host's reported `operator_os` / `operator_arch`.
    pub url: String,
    /// SHA-256 digest of exactly that artifact, lowercase hex.
    pub sha256: String,
    /// ed25519 detached signature over that artifact, base64-encoded.
    /// **Future:** verified against the launcher's pinned public key before
    /// the binary is exec'd. Not enforced in the current iteration.
    pub signature: String,
    /// The installed (frozen) launcher must be >= this version, or the manager
    /// withholds the target and surfaces "redeploy required" instead. The
    /// launcher never self-updates; it is only replaced by a state-preserving
    /// redeploy.
    pub min_launcher_version: String,
}

/// Kubernetes upgrade payload — the operator writes the full values to a
/// ConfigMap (sensitive values to a Secret) and creates a Helm-runner Job that
/// runs `helm upgrade --atomic`. Helm's revision-scoped rollback covers both the
/// image and the values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorHelmTarget {
    /// OCI registry reference for the chart (e.g. `oci://ghcr.io/alienplatform/alien-operator`).
    pub chart_repo: String,
    /// Chart version (e.g. `"1.4.0"`).
    pub chart_version: String,
    /// Full desired values document (manager-owned). Stored verbatim by Helm in
    /// the release Secret, so MUST NOT contain raw secrets — those go via
    /// `sensitive_values` as references.
    pub values: serde_json::Value,
    /// Sensitive values, expressed as references to existing Kubernetes Secrets
    /// in the namespace. Keyed by JSON-Pointer path into `values`. The operator
    /// materializes these into a Secret the chart mounts via
    /// `valueFrom: secretKeyRef:`, so the material never reaches the release
    /// history Secret base64-encoded.
    #[serde(default)]
    pub sensitive_values: std::collections::BTreeMap<String, SecretRef>,
}

/// Reference to a Kubernetes Secret key holding sensitive data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretRef {
    pub name: String,
    pub key: String,
}

/// Target deployment state for the operator to converge toward.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDeployment {
    /// Release information (ID, version, stack definition).
    pub release_info: ReleaseInfo,
    /// Full deployment configuration (settings, env vars, etc.).
    pub config: DeploymentConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_sync_request() -> SyncRequest {
        SyncRequest {
            deployment_id: "dep_abc123".to_string(),
            current_state: None,
            heartbeats: Vec::new(),
            observed_inventory_batches: Vec::new(),
            capabilities: Vec::new(),
            operator_version: None,
            launcher_version: None,
            operator_os: None,
            operator_arch: None,
            packaging: None,
            operator_image_repository: None,
            operator_update: None,
        }
    }

    fn empty_sync_response() -> SyncResponse {
        SyncResponse {
            current_state: None,
            target: None,
            commands_url: None,
            operator_target: None,
        }
    }

    #[test]
    fn test_sync_request_serialization() {
        let req = empty_sync_request();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["deploymentId"], "dep_abc123");
        assert!(json.get("currentState").is_none());
        assert!(json.get("resourceHeartbeats").is_none());
        assert!(json.get("capabilities").is_none());
        assert!(json.get("operatorVersion").is_none());
    }

    #[test]
    fn test_sync_request_deserialization() {
        let json = r#"{"deploymentId": "dep_xyz"}"#;
        let req: SyncRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.deployment_id, "dep_xyz");
        assert!(req.current_state.is_none());
        assert!(req.heartbeats.is_empty());
        assert!(req.observed_inventory_batches.is_empty());
        assert!(req.capabilities.is_empty());
        assert!(req.operator_version.is_none());
    }

    #[test]
    fn test_sync_response_empty() {
        let resp = empty_sync_response();
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("target").is_none());
        assert!(json.get("currentState").is_none());
    }

    #[test]
    fn test_sync_response_roundtrip_no_target() {
        let resp = empty_sync_response();
        let serialized = serde_json::to_string(&resp).unwrap();
        let deserialized: SyncResponse = serde_json::from_str(&serialized).unwrap();
        assert!(deserialized.target.is_none());
        assert!(deserialized.current_state.is_none());
    }

    #[test]
    fn test_sync_request_with_camel_case() {
        let json = r#"{"deploymentId": "dep_1", "currentState": null}"#;
        let req: SyncRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.deployment_id, "dep_1");
        assert!(req.current_state.is_none());
        assert!(req.heartbeats.is_empty());
        assert!(req.capabilities.is_empty());

        // snake_case should NOT work
        let json = r#"{"deployment_id": "dep_1"}"#;
        assert!(serde_json::from_str::<SyncRequest>(json).is_err());
    }

    // --- operator self-update wire format tests --------------------------

    /// An operator that fills in the self-update fields produces JSON the
    /// manager can deserialize, with the expected camelCase + kebab-case values.
    #[test]
    fn test_sync_request_with_self_update_fields_roundtrip() {
        let mut req = empty_sync_request();
        req.operator_version = Some("1.3.5".to_string());
        req.launcher_version = Some("0.1.0".to_string());
        req.operator_os = Some(OperatorOs::Linux);
        req.operator_arch = Some(OperatorArch::Aarch64);
        req.packaging = Some(OperatorPackaging::Kubernetes);
        req.operator_image_repository = Some("ghcr.io/alien-dev/alien-operator".to_string());
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["operatorVersion"], "1.3.5");
        assert_eq!(json["launcherVersion"], "0.1.0");
        assert_eq!(json["operatorOs"], "linux");
        assert_eq!(json["operatorArch"], "aarch64");
        assert_eq!(json["packaging"], "kubernetes"); // kebab-case enum
        assert_eq!(
            json["operatorImageRepository"],
            "ghcr.io/alien-dev/alien-operator"
        );
        let back: SyncRequest = serde_json::from_value(json).unwrap();
        assert_eq!(back.operator_version.as_deref(), Some("1.3.5"));
        assert_eq!(back.launcher_version.as_deref(), Some("0.1.0"));
        assert_eq!(back.packaging, Some(OperatorPackaging::Kubernetes));
        assert_eq!(back.operator_os, Some(OperatorOs::Linux));
        assert_eq!(back.operator_arch, Some(OperatorArch::Aarch64));
    }

    /// The os/arch enums must serialize to the exact `std::env::consts` spellings
    /// the operator reports — in particular `aarch64` (not the `arm64` that
    /// `instance_catalog::Architecture` uses), which is the reason they're
    /// dedicated enums rather than a reuse of the catalog one.
    #[test]
    fn test_operator_os_arch_wire_values() {
        assert_eq!(serde_json::to_value(OperatorOs::Linux).unwrap(), "linux");
        assert_eq!(serde_json::to_value(OperatorOs::Macos).unwrap(), "macos");
        assert_eq!(serde_json::to_value(OperatorOs::Windows).unwrap(), "windows");
        assert_eq!(serde_json::to_value(OperatorArch::X86_64).unwrap(), "x86_64");
        assert_eq!(serde_json::to_value(OperatorArch::Aarch64).unwrap(), "aarch64");

        // from_consts round-trips std::env::consts values; unknowns are dropped.
        assert_eq!(OperatorOs::from_consts("macos"), Some(OperatorOs::Macos));
        assert_eq!(OperatorOs::from_consts("freebsd"), None);
        assert_eq!(OperatorArch::from_consts("aarch64"), Some(OperatorArch::Aarch64));
        assert_eq!(OperatorArch::from_consts("arm"), None);
        assert_eq!(OperatorOs::Macos.as_str(), "macos");
        assert_eq!(OperatorArch::Aarch64.as_str(), "aarch64");
    }

    /// Backward compat: an old operator omits the self-update fields; the
    /// manager deserializes them as None/empty.
    #[test]
    fn test_sync_request_old_operator_no_self_update_fields() {
        let json = r#"{"deploymentId": "dep_old", "currentState": null}"#;
        let req: SyncRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.deployment_id, "dep_old");
        assert!(req.operator_version.is_none());
        assert!(req.launcher_version.is_none());
        assert!(req.operator_os.is_none());
        assert!(req.operator_arch.is_none());
        assert!(req.packaging.is_none());
        assert!(req.operator_update.is_none());
    }

    /// `launcherVersion` is omitted from the JSON when unset (Kubernetes and
    /// launcher-less operators), so old managers see no unknown-field noise.
    #[test]
    fn test_sync_request_launcher_version_omitted_when_none() {
        let req = empty_sync_request();
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("launcherVersion").is_none());
    }

    /// The SyncResponse's `operatorTarget` is omitted when None so an old
    /// operator still gets a clean response.
    #[test]
    fn test_sync_response_old_client_no_operator_target() {
        let resp = empty_sync_response();
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("operatorTarget").is_none());
    }

    #[test]
    fn test_operator_packaging_kebab_case() {
        assert_eq!(
            serde_json::to_value(OperatorPackaging::OsService).unwrap(),
            serde_json::json!("os-service")
        );
        assert_eq!(
            serde_json::to_value(OperatorPackaging::Kubernetes).unwrap(),
            serde_json::json!("kubernetes")
        );
        let r: OperatorPackaging = serde_json::from_str("\"os-service\"").unwrap();
        assert_eq!(r, OperatorPackaging::OsService);
    }

    /// `OperatorUpdateReport` round-trips: internally tagged by `state`
    /// (camelCase variant), camelCase fields, kebab-case phase.
    #[test]
    fn test_operator_update_report_roundtrip() {
        let in_progress = OperatorUpdateReport::InProgress {
            target_version: "1.4.0".to_string(),
            attempt: 1,
        };
        let json = serde_json::to_value(&in_progress).unwrap();
        assert_eq!(json["state"], "inProgress");
        assert_eq!(json["targetVersion"], "1.4.0");
        assert_eq!(json["attempt"], 1);
        assert_eq!(
            serde_json::from_value::<OperatorUpdateReport>(json).unwrap(),
            in_progress
        );

        let failed = OperatorUpdateReport::Failed {
            target_version: "1.4.0".to_string(),
            phase: OperatorUpdatePhase::Pull,
            message: "image :1.4.0 not found".to_string(),
            attempt: 3,
        };
        let json = serde_json::to_value(&failed).unwrap();
        assert_eq!(json["state"], "failed");
        assert_eq!(json["phase"], "pull"); // kebab-case
        assert_eq!(json["targetVersion"], "1.4.0");
        assert_eq!(json["message"], "image :1.4.0 not found");
        assert_eq!(json["attempt"], 3);
        assert_eq!(
            serde_json::from_value::<OperatorUpdateReport>(json).unwrap(),
            failed
        );
    }

    /// `OperatorBinaryTarget` is per-host resolved: exactly one url + sha256 +
    /// signature + minLauncherVersion, all camelCase. (The earlier artifacts-map
    /// shape with a single sha256 was unsound — one digest cannot cover N
    /// binaries — and must not reappear.)
    #[test]
    fn test_operator_binary_target_roundtrip() {
        let binary = OperatorBinaryTarget {
            url: "https://example.com/releases/v1.4.0/alien-operator-1.4.0-linux-x86_64"
                .to_string(),
            sha256: "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3f9c71a1e4a6f2e0e6d5c4b3a"
                .to_string(),
            signature: "c2lnbmF0dXJlLXBsYWNlaG9sZGVy".to_string(),
            min_launcher_version: "0.1.0".to_string(),
        };
        let target = OperatorTarget {
            version: "1.4.0".to_string(),
            min_supported_version: "1.0.0".to_string(),
            binary: Some(binary),
            helm: None,
        };
        let json = serde_json::to_value(&target).unwrap();
        assert!(json.get("helm").is_none(), "helm must be omitted");
        let b = &json["binary"];
        assert_eq!(
            b["url"],
            "https://example.com/releases/v1.4.0/alien-operator-1.4.0-linux-x86_64"
        );
        assert_eq!(
            b["sha256"],
            "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3f9c71a1e4a6f2e0e6d5c4b3a"
        );
        assert_eq!(b["signature"], "c2lnbmF0dXJlLXBsYWNlaG9sZGVy");
        assert_eq!(b["minLauncherVersion"], "0.1.0");
        assert!(
            b.get("artifacts").is_none(),
            "the artifacts map shape must not reappear on the wire"
        );
        let back: OperatorTarget = serde_json::from_value(json).unwrap();
        let back_binary = back.binary.expect("binary should round-trip");
        assert_eq!(back_binary.min_launcher_version, "0.1.0");
        assert_eq!(back_binary.sha256.len(), 64);
        assert!(back.helm.is_none());
    }

    #[test]
    fn test_operator_target_helm_roundtrip() {
        let mut sensitive = std::collections::BTreeMap::new();
        sensitive.insert(
            "/management/token".to_string(),
            SecretRef {
                name: "alien-operator".to_string(),
                key: "sync-token".to_string(),
            },
        );
        let helm = OperatorHelmTarget {
            chart_repo: "oci://ghcr.io/alienplatform/alien-operator".to_string(),
            chart_version: "1.4.0".to_string(),
            values: serde_json::json!({"runtime": {"image": {"tag": "1.4.0"}}}),
            sensitive_values: sensitive,
        };
        let target = OperatorTarget {
            version: "1.4.0".to_string(),
            min_supported_version: "1.3.0".to_string(),
            binary: None,
            helm: Some(helm),
        };
        let json = serde_json::to_value(&target).unwrap();
        assert_eq!(json["minSupportedVersion"], "1.3.0");
        assert!(json.get("binary").is_none(), "binary must be omitted");
        assert_eq!(json["helm"]["chartVersion"], "1.4.0");
        let back: OperatorTarget = serde_json::from_value(json).unwrap();
        assert!(back.binary.is_none());
        assert_eq!(back.helm.unwrap().chart_version, "1.4.0");
    }
}
