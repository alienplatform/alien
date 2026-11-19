//! Deployment state, step results, and runtime metadata.

use crate::{ObservedInventoryBatch, Platform, ResourceHeartbeat, StackState};
use alien_error::AlienError;
use bon::Builder;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::{DeploymentStatus, EnvironmentInfo, ReleaseInfo};

/// Actor that owns structural work during the initial setup phase.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum InitialSetupAuthority {
    /// Setup state was registered by an external setup engine. Alien may only
    /// continue controller states that the importer explicitly initialized.
    #[default]
    ImportedHandoff,
    /// Alien is the setup engine and is running with administrator credentials.
    DirectSetup,
}

/// One-shot authority for a setup re-import to replace setup-owned resources.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SetupUpdateAuthorization {
    /// Unique revision used by persistence layers for compare-and-swap updates.
    pub nonce: String,
    /// Setup resources and permissions digest (`Stack::setup_update_digest`) of the last
    /// successful deployment. The field name is retained for serialized compatibility.
    pub baseline_frozen_digest: String,
    /// Setup resources and permissions digest (`Stack::setup_update_digest`) of the stack
    /// the setup re-import prepared.
    pub target_frozen_digest: String,
    /// Release whose stack was prepared by setup.
    pub release_id: String,
    /// Stable setup target recorded on the imported deployment.
    pub setup_target: String,
    /// Exact setup artifact revision that authored this authority.
    pub setup_fingerprint: String,
    /// Setup fingerprint contract version.
    pub setup_fingerprint_version: u32,
}

/// A setup report the manager declined, for display next to the waiting update.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SetupReportRefusal {
    /// Plain-language reason, safe to show to the customer.
    pub reason: String,
    /// Release the chart reported.
    pub reported_release_id: String,
    /// When the manager declined it.
    pub at: chrono::DateTime<chrono::Utc>,
}

/// A waiting release whose setup change a Helm chart upgrade cannot carry. The
/// manager works this out from the release and the installed setup, so every
/// place that offers an upgrade agrees with the manager's report handling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudSetupRequired {
    /// The waiting release this applies to.
    pub release_id: String,
    /// Plain-language reason, safe to show to the customer.
    pub reason: String,
}

/// The Helm release an Operator reports it was installed as, so the exact
/// upgrade command can be shown. Reported, never trusted for authority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct InstalledHelmRelease {
    /// Release name, as passed to `helm install`.
    pub name: String,
    /// Kubernetes namespace the release is installed in.
    pub namespace: String,
}

/// A credential outage's durable retry window, scoped to its deployment phase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CredentialRetry {
    /// First credential failure in this phase; subsequent attempts keep this timestamp.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Changing phase starts a new window instead of carrying a previous outage forward.
    pub status: DeploymentStatus,
}

/// Explicit disposition of durable data when deleting a deployment.
/// Missing values from older checkpoints always preserve data.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum DeletionPolicy {
    #[default]
    Retain,
    Erase,
}

/// When an air-gapped site's latest status was written and received.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AirgappedReportMetadata {
    /// Site clock time; absent for reports from older clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Manager clock time when the report was accepted.
    pub received_at: chrono::DateTime<chrono::Utc>,
}

/// Runtime metadata for deployment
///
/// Stores deployment state that needs to persist across step calls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetadata {
    /// User-authorized data disposition, persisted before deletion starts.
    #[serde(default)]
    pub deletion_policy: DeletionPolicy,

    /// Most recent air-gapped report accepted by the manager.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub airgapped_report: Option<AirgappedReportMetadata>,

    /// Version of the Operator binary that last reported this deployment, set
    /// by the manager from the sync request. Missing until an Operator that
    /// reports its version has synced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_version: Option<String>,

    /// Bounded credential retry state shared by all deployment drivers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_retry: Option<CredentialRetry>,

    /// Actor authorized to perform structural Frozen-resource work during
    /// InitialSetup. Missing legacy values fail closed as ImportedHandoff.
    #[serde(default)]
    pub initial_setup_authority: InitialSetupAuthority,

    /// Setup-owned grant to keep using the direct setup executor for updates.
    /// Runtime settings and release contents must never create or change this grant.
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub automatic_setup: bool,

    /// An imported install whose machines have not been selected yet. Set when the setup
    /// import registers the deployment; the first InitialSetup step picks machines from free
    /// quota and clears it with the same result, so the pick happens once and a later step
    /// never re-picks against quota that its own machines reduced. Deployments stored
    /// without it deserialize as done.
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub first_setup_machines_unselected: bool,

    /// Hash of the environment variables snapshot that was last synced to the vault
    /// Used to avoid redundant sync operations during incremental deployment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_synced_env_vars_hash: Option<String>,

    /// Exact vault keys owned by the deployment secret synchronizer. This
    /// inventory lets a later snapshot delete removed keys without listing or
    /// touching unrelated values in the same vault.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub last_synced_secret_names: Vec<String>,

    /// Credential revocation completed before resource teardown. A delete retry
    /// can use this receipt after the vault or its binding is gone. Secret sync
    /// clears it before a deployment can write runtime credentials again.
    #[serde(default, skip_serializing_if = "is_false")]
    pub vault_credentials_revoked: bool,

    /// Digest of the installed data inventory whose runtime public-access and
    /// expiry configuration was protected before cleanup. The serialized field
    /// name is retained for compatibility. This is not a
    /// receipt for effective expiry propagation or privileged setup teardown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_storage_protection_hash: Option<String>,

    /// UTC API-confirmation time for the matching data protection digest.
    /// Provider propagation must be qualified separately before final retention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_storage_protection_confirmed_at: Option<chrono::DateTime<chrono::Utc>>,

    /// Start of the bounded wait for retained data's expiry controls to disable.
    /// Cleared on confirmation, explicit retry, or returning to normal execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_data_protection_started_at: Option<chrono::DateTime<chrono::Utc>>,

    /// The prepared (mutated) stack from the last successful deployment phase
    /// This is the stack AFTER mutations have been applied (with service accounts, vault, etc.)
    /// Used for compatibility checks during updates to compare mutated stacks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepared_stack: Option<crate::Stack>,

    /// Canonical resolved answers for inputs that gate Frozen resources,
    /// keyed by input id, recorded when the deployment is created or its
    /// setup import registers.
    ///
    /// Manual installations keep these answers fixed. An installation with an
    /// ongoing-management grant refreshes them when preparing a release. Live
    /// resources sharing a gate use the same recorded answer as Frozen resources.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub persisted_gate_answers: GateAnswers,

    /// Prepared target for an update that has not reached Running yet. Keeping
    /// it separate preserves the last successful baseline across retries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_prepared_stack: Option<crate::Stack>,

    /// Release the in-flight stack was prepared from: `pending_prepared_stack`
    /// during updates, or `prepared_stack` during local first startup. The target can move
    /// to a newer release while that stack is still being applied; this tells
    /// the update which release actually converged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_prepared_release_id: Option<String>,

    /// One-shot setup update authority. It contains only non-secret identity
    /// and canonical resource digests, never the imported payload or tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_update_authorization: Option<SetupUpdateAuthorization>,

    /// Why the manager last declined a setup report from the installed chart,
    /// with when. Cleared when a report is accepted or no longer needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_report_refusal: Option<SetupReportRefusal>,

    /// Set while the waiting release changes something only the cloud setup
    /// can apply, so a chart upgrade is not offered for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_setup_required: Option<CloudSetupRequired>,

    /// The Helm release the Operator reported it runs as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_helm_release: Option<InstalledHelmRelease>,

    /// Last generated CLI package revision whose direct setup was applied.
    /// This lets a newer generated CLI refresh setup-owned infrastructure once
    /// before handing runtime changes back to the hosted manager.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_setup_revision: Option<String>,

    /// Whether cross-account registry access has been successfully granted.
    /// Set to true after the manager successfully sets the ECR/GAR repo policy
    /// for this deployment's target account. Prevents redundant API calls on
    /// every reconcile tick.
    #[serde(default, skip_serializing_if = "is_false")]
    pub registry_access_granted: bool,

    /// What that grant opened, compared against what the deployment needs now so a grant made
    /// before a resource existed is finished rather than skipped, and the revoke names what was
    /// granted. Absent on a grant recorded before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_access: Option<RegistryAccess>,

    /// What a direct setup created for resources it does not own, keyed by resource id. Setup
    /// teardown removes exactly what is recorded here.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub setup_scaffolding: BTreeMap<String, SetupScaffolding>,

    /// Whether each vault-native deployer secret is in the customer's secret
    /// store, with where it goes. Checked from metadata only; no value is ever
    /// read or recorded here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deployer_secrets: Vec<crate::DeployerSecretReport>,

    /// What a one-token call with the customer admin's setup credentials said about each
    /// model an AI resource declares. Written once, during setup; the AI controller's heartbeat
    /// reports it. Absent when no model is declared or the setup could not check.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ai_access_checks: Vec<AiAccessCheck>,
    /// When `deployer_secrets` was last read from the customer's secret store. While the
    /// deployment waits for secrets the check is repeated at most every 30 seconds, because
    /// the engine steps much faster than the store's rate limits allow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployer_secrets_checked_at: Option<chrono::DateTime<chrono::Utc>>,

    /// Resources held back while a required deployer secret is missing: the ones that read
    /// it and everything that depends on them. Everything else keeps installing. Empty once
    /// no required secret is missing. An Operator that predates this field leaves it absent,
    /// so readers must treat absence as unknown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources_waiting_for_secrets: Vec<String>,
}

/// The setup-time answer for one declared model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AiAccessCheck {
    /// The AI resource that declares the model.
    pub resource_id: String,
    /// The model id as the app uses it.
    pub model_id: String,
    /// `verified`, `refused` (the cloud refused the model) or `not-checked`.
    pub result: AiAccessCheckResult,
    /// The cloud's error code or the reason the check could not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub tested_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum AiAccessCheckResult {
    Verified,
    Refused,
    NotChecked,
}

/// Cloud objects a direct setup created so a runtime-owned resource can run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SetupScaffolding {
    /// An AWS sandbox's image-build role, and for `egress: deny` its egress objects.
    #[serde(rename_all = "camelCase")]
    AwsSandbox {
        /// IAM role the image build runs as.
        build_role_name: String,
        /// What `egress: deny` needs; absent for an open sandbox.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        egress: Option<AwsSandboxEgressScaffolding>,
        /// A Frozen sandbox's MicroVM image, built during setup. A Live one's image belongs to its
        /// runtime controller.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image_arn: Option<String>,
    },
    /// A Frozen GCP sandbox's image repository and the manager's write access to it. Setup makes
    /// them because the runtime identity may write images into the repository but cannot create
    /// it or change who may; an image-only release then rolls without setup.
    #[serde(rename_all = "camelCase")]
    GcpSandbox {
        /// Artifact Registry repository the sandbox's image is copied into.
        image_repository: String,
        /// Custom role (no delete) the manager is granted on that repository.
        write_role: String,
        /// Whether the manager holds `write_role` on the repository.
        #[serde(default)]
        writer_granted: bool,
    },
}

/// The objects that keep an AWS deny sandbox's sessions inside the VPC. Each id is recorded as
/// soon as the object exists and cleared once it is deleted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsSandboxEgressScaffolding {
    /// IAM role Lambda assumes to place the connector's network interfaces.
    pub operator_role_name: String,
    /// Security group permitting egress to loopback only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_group_id: Option<String>,
    /// `AWS::Lambda::NetworkConnector` the sessions start with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector_arn: Option<String>,
    /// Cloud Control request creating or deleting the connector that AWS has not finished. Kept
    /// so the request's outcome, and AWS's reason when it fails, is read on a later call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector_request: Option<String>,
}

impl SetupScaffolding {
    /// Takes each object `found` names that this record does not, and keeps every one it does:
    /// a recorded id is what setup last saw, and teardown must delete that one.
    pub fn fill_missing(&mut self, found: SetupScaffolding) {
        match (self, found) {
            (
                SetupScaffolding::AwsSandbox {
                    egress, image_arn, ..
                },
                SetupScaffolding::AwsSandbox {
                    egress: found_egress,
                    image_arn: found_image_arn,
                    ..
                },
            ) => {
                if image_arn.is_none() {
                    *image_arn = found_image_arn;
                }
                match (egress.as_mut(), found_egress) {
                    (None, found_egress) => *egress = found_egress,
                    (Some(recorded), Some(found)) => {
                        if recorded.security_group_id.is_none() {
                            recorded.security_group_id = found.security_group_id;
                        }
                        if recorded.connector_arn.is_none() {
                            recorded.connector_arn = found.connector_arn;
                        }
                    }
                    (Some(_), None) => {}
                }
            }
            // A repository is found by its fixed name, so a record never lacks anything a found
            // one would add.
            (SetupScaffolding::GcpSandbox { .. }, _) | (_, SetupScaffolding::GcpSandbox { .. }) => {}
        }
    }
}

/// The cross-account read a manager opened on Alien's registry for one deployment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RegistryAccess {
    /// Repository identifiers the grant names, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<String>,

    /// Compute services the grant admits, sorted. Each pulls as its own principal, so a service
    /// added later needs the policy rewritten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service_types: Vec<String>,

    /// A protected install's verifier, which deploys its functions from these images and so
    /// reads them as well. `None` on an unprotected install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier: Option<String>,
}

/// Deployment state
///
/// Represents the current state of deployed infrastructure, including release tracking.
/// This is platform-agnostic - no backend IDs or database relationships.
///
/// The deployment engine manages releases internally: when a deployment succeeds,
/// it promotes `target_release` to `current_release` and clears `target_release`.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentState {
    /// Current lifecycle phase
    pub status: DeploymentStatus,
    /// Target cloud platform (AWS, GCP, Azure, Kubernetes)
    pub platform: Platform,
    /// Currently deployed release (None for first deployment)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_release: Option<ReleaseInfo>,
    /// Target release to deploy (None when synced with current)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_release: Option<ReleaseInfo>,
    /// Infrastructure resource tracking (which resources exist, their status, outputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_state: Option<StackState>,
    /// Deployment-level error for failures not owned by a specific resource.
    ///
    /// Resource controller failures belong in `stack_state.resources[*].error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AlienError>,
    /// Cloud account details (account ID, project number, region)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_info: Option<EnvironmentInfo>,
    /// Deployment-specific data (prepared stacks, phase tracking, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_metadata: Option<RuntimeMetadata>,
    /// Whether a retry has been requested for a failed deployment
    /// When true and status is a failed state, the deployment system will retry failed resources
    #[serde(default, skip_serializing_if = "is_false")]
    pub retry_requested: bool,
    /// Protocol version for cross-actor compatibility.
    /// All actors (manager, push client, agent) check this before stepping.
    /// Mismatched versions produce a clear error instead of silent corruption.
    /// See docs/02-manager/10-deployment-protocol.md.
    pub protocol_version: u32,
}

impl DeploymentState {
    /// Returns whether this state carries desired infrastructure for the
    /// deployment runner to converge.
    pub fn has_desired(&self) -> bool {
        self.current_release.is_some()
            || self.target_release.is_some()
            || self.stack_state.is_some()
    }

    /// The stack an unfinished update is still applying after a newer target release arrived,
    /// with the release it was prepared from. Such an update finishes on that stack before the
    /// newer target is prepared, so its resources need that release's extensions, not the
    /// target's. `None` when no update is unfinished or it applies the target release.
    pub fn unfinished_update(&self) -> Option<(&str, &crate::Stack)> {
        let metadata = self.runtime_metadata.as_ref()?;
        let stack = metadata.pending_prepared_stack.as_ref()?;
        let prepared_from = metadata.pending_prepared_release_id.as_deref()?;
        let target = self.target_release.as_ref()?.release_id.as_deref()?;
        (prepared_from != target).then_some((prepared_from, stack))
    }

    /// The stack whose resources the deployment applies: an unfinished update's, otherwise the
    /// target release's, otherwise the current release's.
    pub fn applied_stack(&self) -> Option<&crate::Stack> {
        self.unfinished_update()
            .map(|(_, stack)| stack)
            .or(self.target_release.as_ref().map(|release| &release.stack))
            .or(self.current_release.as_ref().map(|release| &release.stack))
    }
}

/// Result of a deployment step
///
/// Contains the complete next deployment state along with hints for the platform.
/// This replaces the old delta-based `DeploymentStateUpdate` approach.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentStepResult {
    /// The complete next deployment state
    pub state: DeploymentState,

    /// Suggested delay before next step (optimization hint)
    /// - `None`: No suggested delay, can poll immediately
    /// - `Some(ms)`: Wait this many milliseconds before next step
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_delay_ms: Option<u64>,

    /// Whether to update heartbeat timestamp (monitoring signal)
    /// - `false`: Don't update heartbeat (default for most steps)
    /// - `true`: Update lastHeartbeatAt (for successful health checks in Running state)
    #[serde(default, skip_serializing_if = "is_false")]
    pub update_heartbeat: bool,

    /// Managed Alien resource status samples emitted by controllers during this step.
    #[serde(
        default,
        rename = "resourceHeartbeats",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub heartbeats: Vec<ResourceHeartbeat>,

    /// Observed raw-resource inventory batches read during this step.
    #[serde(
        default,
        rename = "observedInventoryBatches",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub observed_inventory_batches: Vec<ObservedInventoryBatch>,
}

pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}

/// Answers for inputs gating Frozen resources, keyed by input id.
pub type GateAnswers = IndexMap<String, bool>;

/// Oldest deployment protocol version this binary can read.
pub const MIN_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION: u32 = 1;

/// Deployment protocol written by actors with resumable protection, CORS and
/// database update states and retained deletion. Older readers must be fenced
/// before these states exist.
pub const CURRENT_DEPLOYMENT_PROTOCOL_VERSION: u32 = 4;

/// Latest state protocol this binary can execute.
pub const MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION: u32 = 4;

/// GCP federation changes setup ownership and target credential semantics.
pub const GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION: u32 = 2;

pub fn deployment_protocol_version_for_platform(
    _platform: crate::Platform,
    _base_platform: Option<crate::Platform>,
) -> u32 {
    CURRENT_DEPLOYMENT_PROTOCOL_VERSION
}

/// Accept unchanged reports or the explicit upgrade to this writer protocol.
/// Never permit downgrade or unknown protocols. Provider setup prerequisites
/// remain separate from this checkpoint compatibility check.
pub fn accepts_deployment_protocol_report(registered: u32, reported: u32) -> bool {
    (MIN_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION..=MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION)
        .contains(&registered)
        && (reported == registered
            || (registered < CURRENT_DEPLOYMENT_PROTOCOL_VERSION
                && reported == CURRENT_DEPLOYMENT_PROTOCOL_VERSION))
}

/// Backwards-compatible alias for older call sites.
pub const DEPLOYMENT_PROTOCOL_VERSION: u32 = CURRENT_DEPLOYMENT_PROTOCOL_VERSION;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Platform, ReleaseInfo, Stack, StackState};
    use indexmap::IndexMap;

    fn empty_stack() -> Stack {
        Stack {
            id: "stack_test".to_string(),
            resources: IndexMap::new(),
            inputs: vec![],
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
            permissions: crate::PermissionsConfig::default(),
            supported_platforms: None,
        }
    }

    fn release_info(id: &str) -> ReleaseInfo {
        ReleaseInfo {
            release_id: Some(id.to_string()),
            version: None,
            description: None,
            stack: empty_stack(),
            signed: None,
            signer_lists: Vec::new(),
        }
    }

    fn state() -> DeploymentState {
        DeploymentState {
            status: DeploymentStatus::Pending,
            platform: Platform::Kubernetes,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    #[test]
    fn deployment_state_has_desired_when_release_or_stack_state_exists() {
        let observe_only = state();
        assert!(!observe_only.has_desired());

        let mut current = state();
        current.current_release = Some(release_info("rel_current"));
        assert!(current.has_desired());

        let mut target = state();
        target.target_release = Some(release_info("rel_target"));
        assert!(target.has_desired());

        let mut imported = state();
        imported.stack_state = Some(StackState::new(Platform::Kubernetes));
        assert!(imported.has_desired());
    }

    /// An update still applying the stack of an earlier release, after a newer target arrived,
    /// applies that stack until it finishes; once the update applies the target release, or
    /// none is unfinished, the target release's stack is the one applied.
    #[test]
    fn an_unfinished_update_applies_the_stack_it_was_prepared_from() {
        let mut in_flight = empty_stack();
        in_flight.id = "in-flight".to_string();
        let mut deployment = state();
        deployment.current_release = Some(release_info("rel_a"));
        deployment.target_release = Some(release_info("rel_c"));
        assert_eq!(deployment.unfinished_update(), None);
        assert_eq!(
            deployment.applied_stack(),
            Some(&deployment.target_release.as_ref().unwrap().stack)
        );

        deployment.runtime_metadata = Some(RuntimeMetadata {
            pending_prepared_stack: Some(in_flight.clone()),
            pending_prepared_release_id: Some("rel_b".to_string()),
            ..Default::default()
        });
        assert_eq!(deployment.unfinished_update(), Some(("rel_b", &in_flight)));
        assert_eq!(deployment.applied_stack(), Some(&in_flight));

        // Prepared from the target itself: the update applies the target release.
        deployment
            .runtime_metadata
            .as_mut()
            .unwrap()
            .pending_prepared_release_id = Some("rel_c".to_string());
        assert_eq!(deployment.unfinished_update(), None);
    }

    #[test]
    fn runtime_metadata_from_before_secret_inventory_defaults_to_empty() {
        let metadata: RuntimeMetadata = serde_json::from_value(serde_json::json!({
            "lastSyncedEnvVarsHash": "old-hash"
        }))
        .expect("old runtime metadata remains readable");

        assert_eq!(
            metadata.last_synced_env_vars_hash.as_deref(),
            Some("old-hash")
        );
        assert!(metadata.last_synced_secret_names.is_empty());
        assert!(!metadata.vault_credentials_revoked);
        assert!(metadata.retained_storage_protection_hash.is_none());
        assert!(metadata.retained_storage_protection_confirmed_at.is_none());
        assert!(metadata.pending_prepared_stack.is_none());
        assert!(metadata.setup_update_authorization.is_none());
        assert!(metadata.setup_scaffolding.is_empty());
    }

    /// Teardown reads this record back from persisted state, so its wire form is a contract.
    #[test]
    fn setup_scaffolding_round_trips_in_its_persisted_form() {
        let persisted = serde_json::json!({
            "initialSetupAuthority": "directSetup",
            "setupScaffolding": {
                "agents": { "type": "awsSandbox", "buildRoleName": "acme-agents-build" }
            }
        });
        let metadata: RuntimeMetadata =
            serde_json::from_value(persisted.clone()).expect("persisted record reads");
        assert_eq!(
            metadata.setup_scaffolding["agents"],
            SetupScaffolding::AwsSandbox {
                build_role_name: "acme-agents-build".to_string(),
                egress: None,
                image_arn: None,
            }
        );
        assert_eq!(metadata.deletion_policy, DeletionPolicy::Retain);
        let mut expected = persisted;
        expected["deletionPolicy"] = serde_json::json!("retain");
        assert_eq!(serde_json::to_value(&metadata).unwrap(), expected);
        assert!(
            serde_json::to_value(RuntimeMetadata::default()).unwrap()["setupScaffolding"].is_null(),
            "state with no scaffolding serializes as it did before the field existed"
        );
    }

    #[test]
    fn filling_a_record_takes_only_what_it_lacks() {
        let egress = |group: Option<&str>, connector: Option<&str>| AwsSandboxEgressScaffolding {
            operator_role_name: "acme-agents-egress".to_string(),
            security_group_id: group.map(str::to_string),
            connector_arn: connector.map(str::to_string),
            connector_request: None,
        };
        let record = |egress| SetupScaffolding::AwsSandbox {
            build_role_name: "acme-agents-build".to_string(),
            egress,
            image_arn: None,
        };

        let mut partial = record(Some(egress(Some("sg-recorded"), None)));
        partial.fill_missing(record(Some(egress(Some("sg-found"), Some("arn:found")))));
        assert_eq!(
            partial,
            record(Some(egress(Some("sg-recorded"), Some("arn:found")))),
            "a recorded id is the one teardown deletes"
        );

        let mut role_only = record(None);
        role_only.fill_missing(record(Some(egress(Some("sg-found"), None))));
        assert_eq!(role_only, record(Some(egress(Some("sg-found"), None))));

        let image = |arn: Option<&str>| SetupScaffolding::AwsSandbox {
            build_role_name: "acme-agents-build".to_string(),
            egress: None,
            image_arn: arn.map(str::to_string),
        };
        let mut without_image = image(None);
        without_image.fill_missing(image(Some("arn:found")));
        assert_eq!(without_image, image(Some("arn:found")));
        let mut with_image = image(Some("arn:recorded"));
        with_image.fill_missing(image(Some("arn:found")));
        assert_eq!(with_image, image(Some("arn:recorded")));
    }

    /// A deny sandbox's record is written piece by piece, so a partial one must read back as is.
    #[test]
    fn a_partial_egress_record_round_trips_in_its_persisted_form() {
        let persisted = serde_json::json!({
            "type": "awsSandbox",
            "buildRoleName": "acme-agents-build",
            "egress": {
                "operatorRoleName": "acme-agents-egress",
                "securityGroupId": "sg-0123"
            }
        });
        let record: SetupScaffolding =
            serde_json::from_value(persisted.clone()).expect("persisted record reads");
        assert_eq!(
            record,
            SetupScaffolding::AwsSandbox {
                build_role_name: "acme-agents-build".to_string(),
                egress: Some(AwsSandboxEgressScaffolding {
                    operator_role_name: "acme-agents-egress".to_string(),
                    security_group_id: Some("sg-0123".to_string()),
                    connector_arn: None,
                    connector_request: None,
                }),
                image_arn: None,
            }
        );
        assert_eq!(serde_json::to_value(&record).unwrap(), persisted);
    }
}
