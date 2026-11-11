use std::collections::BTreeMap;
use std::time::Duration;

use alien_core::{
    bindings::KeyBinding,
    import::{data::GcpKeyImportData, ImportContext},
    GcpCloudKmsKeyHeartbeatData, HeartbeatBackend, Key, KeyFingerprint, KeyHeartbeatData,
    KeyHeartbeatStatus, KeyOutputs, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus, StackResourceState,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_gcp_clients::{
    cloud_kms::{CloudKmsApi, CreateCryptoKeyRequest, VersionTemplate},
    iam::{Binding, IamPolicy},
};
use alien_macros::controller;
use alien_permissions::key_access::{gcp_key_roles, key_access_plan};
use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
    import::ResourceImporter,
    import_helpers::make_imported_state,
};

/// Name of the one crypto key in the ring, as in the Terraform setup.
const CRYPTO_KEY_ID: &str = "key";
/// Same rotation as the Terraform setup: 90 days.
const ROTATION_SECONDS: i64 = 7_776_000;

#[controller]
pub struct GcpKeyController {
    #[serde(default)]
    pub(crate) available: Option<bool>,
    pub(crate) crypto_key_name: Option<String>,
    pub(crate) primary_version: Option<String>,
    /// Whether Alien made this key in a direct setup run. Older states are imports.
    #[serde(default)]
    pub(crate) origin: super::KeyOrigin,
    /// Full name of the key ring, chosen before the first call so every retry reuses it.
    #[serde(default)]
    pub(crate) key_ring_name: Option<String>,
    /// Digest of the grants last written.
    #[serde(default)]
    pub(crate) access_revision: Option<String>,
    /// (role, member) pairs this controller put on the crypto key's IAM policy.
    #[serde(default)]
    pub(crate) bindings: Vec<(String, String)>,
}

#[controller]
impl GcpKeyController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let gcp = ctx.get_gcp_config()?;
        self.key_ring_name = Some(format!(
            "projects/{}/locations/{}/keyRings/{}",
            gcp.project_id,
            gcp.region,
            key_ring_id(ctx.resource_prefix, &key.id)
        ));
        Ok(HandlerAction::Continue {
            state: CreatingKeyRing,
            suggested_delay: None,
        })
    }

    #[handler(state = CreatingKeyRing, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn creating_key_ring(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let gcp = ctx.get_gcp_config()?;
        let ring = self
            .key_ring_name
            .clone()
            .ok_or_else(|| missing(&key.id, "key ring name"))?;
        let (location, ring_id) = ring
            .rsplit_once("/keyRings/")
            .ok_or_else(|| missing(&key.id, "key ring name"))?;
        let client = ctx.service_provider.get_gcp_cloud_kms_client(gcp)?;
        match client.create_key_ring(location, ring_id).await {
            Ok(_) => {}
            // The name carries this deployment's prefix and a digest, so an existing ring is
            // an earlier attempt of this run. A ring holds nothing until a key is in it, and
            // the key is checked before it is used.
            Err(error) if is_conflict(&error) => {}
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to create the Cloud KMS key ring".to_string(),
                    resource_id: Some(key.id.clone()),
                }));
            }
        }
        Ok(HandlerAction::Continue {
            state: CreatingCryptoKey,
            suggested_delay: None,
        })
    }

    #[handler(state = CreatingCryptoKey, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn creating_crypto_key(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let gcp = ctx.get_gcp_config()?;
        let ring = self
            .key_ring_name
            .clone()
            .ok_or_else(|| missing(&key.id, "key ring name"))?;
        let client = ctx.service_provider.get_gcp_cloud_kms_client(gcp)?;
        let labels = key_labels(ctx.resource_prefix, &key.id);
        let request = CreateCryptoKeyRequest {
            purpose: "ENCRYPT_DECRYPT".to_string(),
            version_template: VersionTemplate {
                algorithm: "GOOGLE_SYMMETRIC_ENCRYPTION".to_string(),
                protection_level: Some("SOFTWARE".to_string()),
            },
            labels: labels.clone(),
            rotation_period: format!("{ROTATION_SECONDS}s"),
            next_rotation_time: (Utc::now() + chrono::Duration::seconds(ROTATION_SECONDS))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        let name = format!("{ring}/cryptoKeys/{CRYPTO_KEY_ID}");
        let (created, adopted) =
            match client
                .create_crypto_key(&ring, CRYPTO_KEY_ID, request)
                .await
            {
                Ok(created) => (created, false),
                Err(error) if is_conflict(&error) => {
                    // An earlier attempt of this run, or a key someone put under our name: it is
                    // used only if its metadata is what Alien creates and it is labelled ours.
                    let existing = client.get_crypto_key(&name).await.context(
                        ErrorData::CloudPlatformError {
                            message: "Failed to read the existing Cloud KMS key".to_string(),
                            resource_id: Some(key.id.clone()),
                        },
                    )?;
                    check_adoptable(&existing, &name, &labels, &key.id)?;
                    (existing, true)
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: "Failed to create the Cloud KMS key".to_string(),
                        resource_id: Some(key.id.clone()),
                    }));
                }
            };
        self.crypto_key_name = Some(created.name);
        self.primary_version = created.primary.map(|version| version.name);
        self.origin = super::KeyOrigin::Created;
        // An adopted key's policy is replaced, not merged: labels are writable by anyone
        // who can label, so nothing on it is trusted.
        self.apply_bindings(ctx, adopted).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        if self.origin == super::KeyOrigin::Created && super::may_write_keys(ctx) {
            self.apply_bindings(ctx, false).await?;
        }
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    /// Removes this deployment's members from the key's policy and keeps the key: Cloud KMS
    /// cannot delete keys or rings, and nothing in Alien destroys key versions.
    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if self.crypto_key_name.is_none() {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }
        if self.origin != super::KeyOrigin::Created {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(key.id.clone()),
                message: format!(
                    "Key '{}' is owned by the setup that created it; remove it with that setup",
                    key.id
                ),
            }));
        }
        // The IAM policy belongs to the key, which outlives the deployment: only the admin's
        // setup credentials may edit it.
        if super::may_write_keys(ctx) && !self.bindings.is_empty() {
            self.write_bindings(ctx, &[], false).await?;
        }
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // A failed check keeps the last observation unless Cloud KMS answered about
        // the key itself; a throttle or 5xx must not pause every dependent resource.
        let config = ctx.desired_resource_config::<Key>()?;
        let crypto_key_name = self.crypto_key_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceStateSerializationFailed {
                resource_id: config.id.clone(),
                message: "Imported GCP key is missing its CryptoKey name".to_string(),
            })
        })?;
        let key = ctx
            .service_provider
            .get_gcp_cloud_kms_client(ctx.get_gcp_config()?)?
            .get_crypto_key(crypto_key_name)
            .await
            .inspect_err(|error| {
                if super::key_revoked_by_status(error, crypto_key_name) {
                    self.available = Some(false);
                }
            })
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read GCP Cloud KMS key metadata".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        if key.name != crypto_key_name {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "GCP Cloud KMS returned metadata for a different key".to_string(),
            }));
        }
        let primary_state = key.primary.as_ref().map(|version| version.state.as_str());
        let (health, lifecycle) = match primary_state {
            Some("ENABLED") => (ObservedHealth::Healthy, ProviderLifecycleState::Running),
            Some("DISABLED") => (ObservedHealth::Unhealthy, ProviderLifecycleState::Stopped),
            Some("DESTROY_SCHEDULED") | Some("DESTROYED") => {
                (ObservedHealth::Unhealthy, ProviderLifecycleState::Deleting)
            }
            _ => (ObservedHealth::Degraded, ProviderLifecycleState::Unknown),
        };
        self.available = Some(primary_state == Some("ENABLED"));
        self.primary_version = key.primary.as_ref().map(|version| version.name.clone());
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Key::RESOURCE_TYPE,
            controller_platform: Platform::Gcp,
            backend: HeartbeatBackend::Gcp,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Key(KeyHeartbeatData::GcpCloudKms(
                GcpCloudKmsKeyHeartbeatData {
                    status: KeyHeartbeatStatus {
                        health,
                        lifecycle,
                        message: (self.available != Some(true))
                            .then(|| "customer key unavailable".to_string()),
                    },
                    crypto_key_name: key.name,
                    purpose: key.purpose,
                    primary_version: key.primary.as_ref().map(|version| version.name.clone()),
                    primary_state: key.primary.as_ref().map(|version| version.state.clone()),
                    algorithm: key.primary.map(|version| version.algorithm),
                },
            )),
            raw: vec![],
        });
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn requires_convergence_reconciliation(&self) -> bool {
        true
    }

    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        if self.origin != super::KeyOrigin::Created || !super::may_write_keys(ctx) {
            return Ok(false);
        }
        let key = ctx.desired_resource_config::<Key>()?;
        Ok(self.access_revision.as_deref() != Some(desired_revision(ctx, &key.id)?.as_str()))
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        Some(ResourceOutputs::new(KeyOutputs {
            // A key whose checks kept failing until RefreshFailed is no longer
            // observed, so its last value would never change again.
            available: Some(self.state == GcpKeyState::Ready && self.available == Some(true)),
            fingerprint: KeyFingerprint::Gcp {
                crypto_key_name: self.crypto_key_name.clone()?,
            },
            wrapping_key_id: self.primary_version.clone()?,
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        let Some(crypto_key_name) = &self.crypto_key_name else {
            return Ok(None);
        };
        Ok(Some(
            serde_json::to_value(KeyBinding::gcp_cloud_kms(crypto_key_name))
                .into_alien_error()
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize Key binding parameters".to_string(),
                })?,
        ))
    }
}

impl GcpKeyController {
    async fn apply_bindings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        replace_policy: bool,
    ) -> Result<()> {
        let key = ctx.desired_resource_config::<Key>()?;
        let wanted = desired_bindings(ctx, &key.id)?;
        self.write_bindings(ctx, &wanted, replace_policy).await?;
        self.bindings = wanted;
        self.access_revision = Some(desired_revision(ctx, &key.id)?);
        Ok(())
    }

    /// Makes the crypto key's IAM policy hold `wanted` and none of the pairs an earlier run
    /// wrote that are not wanted any more. Members someone else added stay, unless
    /// `replace_policy` (an adopted key), which writes exactly `wanted`.
    async fn write_bindings(
        &self,
        ctx: &ResourceControllerContext<'_>,
        wanted: &[(String, String)],
        replace_policy: bool,
    ) -> Result<()> {
        let key = ctx.desired_resource_config::<Key>()?;
        let name = self
            .crypto_key_name
            .as_deref()
            .ok_or_else(|| missing(&key.id, "crypto key name"))?;
        let client = ctx
            .service_provider
            .get_gcp_cloud_kms_client(ctx.get_gcp_config()?)?;
        let mut attempts = 0;
        loop {
            attempts += 1;
            let current = client.get_crypto_key_iam_policy(name).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to read the Cloud KMS key's IAM policy".to_string(),
                    resource_id: Some(key.id.clone()),
                },
            )?;
            let policy = merged_policy(current, &self.bindings, wanted, replace_policy);
            match client.set_crypto_key_iam_policy(name, policy).await {
                Ok(_) => return Ok(()),
                Err(error) if is_conflict(&error) && attempts < 5 => continue,
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: "Failed to write the Cloud KMS key's IAM policy".to_string(),
                        resource_id: Some(key.id.clone()),
                    }));
                }
            }
        }
    }
}

fn missing(key_id: &str, what: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceStateSerializationFailed {
        resource_id: key_id.to_string(),
        message: format!("GCP key state is missing its {what}"),
    })
}

/// Cloud KMS answers a duplicate create with 409 `ALREADY_EXISTS` and a stale etag with 409
/// `ABORTED` (some surfaces use 412).
fn is_conflict(error: &alien_client_core::Error) -> bool {
    matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
    ) || super::cloud_response(error).is_some_and(|(status, _, _)| matches!(status, 409 | 412))
}

/// `<prefix>-<id>` as the Terraform setup names it, trimmed to leave room for a digest of the
/// full name. The digest replaces Terraform's random suffix so a retry computes the same ring.
pub(crate) fn key_ring_id(prefix: &str, key_id: &str) -> String {
    let base: String = format!("{prefix}-{key_id}")
        .to_lowercase()
        .replace('_', "-")
        .chars()
        .take(56)
        .collect();
    let digest = Sha256::digest(format!("{prefix}/{key_id}").as_bytes());
    let suffix: String = digest
        .iter()
        .take(3)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{}-{suffix}", base.trim_matches('-'))
}

/// Labels the Terraform setup puts on its keys.
fn key_labels(prefix: &str, key_id: &str) -> BTreeMap<String, String> {
    alien_core::setup_resource_tags(prefix, key_id, "key")
        .into_iter()
        .map(|(label, value)| (label, sanitize_label(&value)))
        .collect()
}

fn sanitize_label(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(63)
        .collect()
}

/// A key found under our name is used only when Alien would have created it that way.
fn check_adoptable(
    existing: &alien_gcp_clients::cloud_kms::CryptoKey,
    name: &str,
    labels: &BTreeMap<String, String>,
    key_id: &str,
) -> Result<()> {
    let refuse = |why: String| {
        Err(AlienError::new(ErrorData::ResourceDrift {
            resource_id: key_id.to_string(),
            message: format!(
                "Key '{name}' already exists and is not a key Alien creates ({why}); refusing to adopt it"
            ),
        }))
    };
    if existing.name != name {
        return refuse("it is a different key".to_string());
    }
    if existing.purpose != "ENCRYPT_DECRYPT" {
        return refuse(format!("purpose {}", existing.purpose));
    }
    let template = existing.version_template.as_ref();
    if template.map(|t| t.algorithm.as_str()) != Some("GOOGLE_SYMMETRIC_ENCRYPTION")
        || template.and_then(|t| t.protection_level.as_deref()) != Some("SOFTWARE")
    {
        return refuse("it is not a Google-managed software symmetric key".to_string());
    }
    if labels
        .iter()
        .any(|(label, value)| existing.labels.get(label) != Some(value))
    {
        return refuse("it does not carry this deployment's labels".to_string());
    }
    Ok(())
}

fn desired_bindings(
    ctx: &ResourceControllerContext<'_>,
    key_id: &str,
) -> Result<Vec<(String, String)>> {
    let invalid = |message: &str| ErrorData::ResourceConfigInvalid {
        resource_id: Some(key_id.to_string()),
        message: message.to_string(),
    };
    let plan = key_access_plan(ctx.desired_stack, key_id)
        .context(invalid("Failed to work out which accounts may use the key"))?;
    let roles = gcp_key_roles(&plan).context(invalid("Failed to render the key roles"))?;
    let project = &ctx.get_gcp_config()?.project_id;
    let member = |account_id: String| {
        format!("serviceAccount:{account_id}@{project}.iam.gserviceaccount.com")
    };
    let prefix = ctx.resource_prefix;
    let mut wanted = Vec::new();
    if plan.remote_bindings {
        wanted.push((
            roles.cryptography.clone(),
            member(crate::remote_bindings::service_account_id(prefix)),
        ));
    }
    for account in &plan.runtime_cryptography {
        wanted.push((
            roles.cryptography.clone(),
            member(crate::service_account::get_gcp_service_account_id(
                prefix, account,
            )),
        ));
    }
    if let (Some(role), true) = (roles.management, plan.has_management_identity) {
        wanted.push((
            role.to_string(),
            member(crate::remote_stack_management::get_gcp_management_service_account_id(prefix)),
        ));
    }
    wanted.sort();
    wanted.dedup();
    Ok(wanted)
}

fn desired_revision(ctx: &ResourceControllerContext<'_>, key_id: &str) -> Result<String> {
    let bindings = desired_bindings(ctx, key_id)?
        .into_iter()
        .map(|(role, member)| format!("{role}={member}"))
        .collect::<Vec<_>>();
    Ok(super::access_revision(&[&bindings.join("\n")]))
}

/// `current` without the pairs in `previous` that are not in `wanted`, plus `wanted`; or just
/// `wanted` when `replace_policy`.
fn merged_policy(
    current: IamPolicy,
    previous: &[(String, String)],
    wanted: &[(String, String)],
    replace_policy: bool,
) -> IamPolicy {
    let mut bindings: Vec<Binding> = if replace_policy {
        Vec::new()
    } else {
        current
            .bindings
            .into_iter()
            .map(|mut binding| {
                binding.members.retain(|member| {
                    let ours = previous
                        .iter()
                        .any(|(role, m)| *role == binding.role && m == member);
                    let still_wanted = wanted
                        .iter()
                        .any(|(role, m)| *role == binding.role && m == member);
                    !ours || still_wanted
                });
                binding
            })
            .filter(|binding| !binding.members.is_empty())
            .collect()
    };
    for (role, member) in wanted {
        match bindings
            .iter_mut()
            .find(|binding| &binding.role == role && binding.condition.is_none())
        {
            Some(binding) if binding.members.contains(member) => {}
            Some(binding) => binding.members.push(member.clone()),
            None => bindings.push(Binding {
                role: role.clone(),
                members: vec![member.clone()],
                condition: None,
            }),
        }
    }
    IamPolicy {
        version: Some(3),
        kind: None,
        resource_id: None,
        bindings,
        etag: current.etag,
    }
}

#[derive(Debug, Default)]
pub struct GcpKeyImporter;

impl ResourceImporter for GcpKeyImporter {
    type ImportData = GcpKeyImportData;

    fn merge_reimport(
        &self,
        existing: StackResourceState,
        imported: StackResourceState,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        super::merge_key_reimport(existing, imported, ctx)
    }

    fn import(
        &self,
        data: Self::ImportData,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        make_imported_state(
            GcpKeyController {
                state: GcpKeyState::Ready,
                available: None,
                crypto_key_name: Some(data.crypto_key_name),
                primary_version: Some(data.primary_version),
                origin: super::KeyOrigin::Imported,
                key_ring_name: None,
                access_revision: None,
                bindings: Vec::new(),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}

#[cfg(all(test, feature = "test"))]
#[path = "gcp_tests.rs"]
mod gcp_tests;
