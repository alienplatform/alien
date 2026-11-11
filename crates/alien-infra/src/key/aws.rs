use std::time::Duration;

use alien_core::{
    bindings::KeyBinding,
    import::{data::AwsKeyImportData, ImportContext},
    AwsKmsKeyHeartbeatData, HeartbeatBackend, Key, KeyFingerprint, KeyHeartbeatData,
    KeyHeartbeatStatus, KeyOutputs, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus, StackResourceState,
};
use alien_aws_clients::{
    kms::{CreateKeyRequest, KmsApi, KmsTag},
    resourcegroupstagging::{GetResourcesRequest, TagFilter},
    ResourceGroupsTaggingApi,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_permissions::{
    aws_key_policy::{aws_key_policy_for_stack, LiteralAwsPrincipals},
    key_access::{aws_key_grants, key_access_plan},
};
use alien_macros::controller;
use chrono::Utc;

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
    import::ResourceImporter,
    import_helpers::make_imported_state,
};

#[controller]
pub struct AwsKeyController {
    // Older installations have only DescribeKey permission until setup is rerun.
    #[serde(default)]
    pub(crate) availability_probe: bool,
    #[serde(default)]
    pub(crate) available: Option<bool>,
    pub(crate) key_arn: Option<String>,
    pub(crate) region: Option<String>,
    /// Whether Alien made this key in a direct setup run. Older states are imports.
    #[serde(default)]
    pub(crate) origin: super::KeyOrigin,
    /// Saved by the step before the one that sends `CreateKey`, so a crash after the call is found on the rerun.
    #[serde(default)]
    pub(crate) create_requested: bool,
    /// Digest of the key policy and grants last written.
    #[serde(default)]
    pub(crate) access_revision: Option<String>,
    /// Inline policies this controller put on roles: (role name, policy name).
    #[serde(default)]
    pub(crate) grants: Vec<(String, String)>,
}

#[controller]
impl AwsKeyController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        self.region = Some(ctx.get_aws_config()?.region.clone());
        // Saved with this step's transition, before any CreateKey call: a crash after the
        // call finds the flag set and looks for the key it may have made.
        self.create_requested = true;
        Ok(HandlerAction::Continue {
            state: CreatingKey,
            suggested_delay: None,
        })
    }

    #[handler(state = CreatingKey, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn creating_key(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let aws = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_kms_client(aws).await?;
        let policy = serde_json::to_string(&desired_policy(ctx, &key.id)?)
            .into_alien_error()
            .context(ErrorData::ResourceStateSerializationFailed {
                resource_id: key.id.clone(),
                message: "Failed to serialize the key policy".to_string(),
            })?;

        if self.create_requested && self.key_arn.is_none() {
            // A crash between the CreateKey call and the checkpoint left a key this
            // deployment may own. Tags are written by anyone who can tag, so the key is
            // adopted only after its metadata and policy are checked and rewritten.
            let tagging = ctx
                .service_provider
                .get_aws_resource_groups_tagging_client(aws)
                .await?;
            if let Some(arn) = find_tagged_key(ctx, tagging.as_ref(), client.as_ref(), &key.id).await? {
                client
                    .put_key_policy(&arn, &policy)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to apply the key policy to the adopted key".to_string(),
                        resource_id: Some(key.id.clone()),
                    })?;
                self.key_arn = Some(arn);
                self.origin = super::KeyOrigin::Created;
                return Ok(HandlerAction::Continue {
                    state: EnablingRotation,
                    suggested_delay: None,
                });
            }
        }

        let created = client
            .create_key(CreateKeyRequest {
                description: alien_core::key_description(&key.id),
                key_spec: "SYMMETRIC_DEFAULT".to_string(),
                key_usage: "ENCRYPT_DECRYPT".to_string(),
                policy,
                tags: key_tags(ctx.resource_prefix, &key.id),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create the AWS KMS key".to_string(),
                resource_id: Some(key.id.clone()),
            })?;
        self.key_arn = Some(created.key_metadata.arn);
        self.origin = super::KeyOrigin::Created;
        Ok(HandlerAction::Continue {
            state: EnablingRotation,
            suggested_delay: None,
        })
    }

    #[handler(state = EnablingRotation, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn enabling_rotation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let key_arn = self.created_key_arn(&key.id)?;
        ctx.service_provider
            .get_aws_kms_client(ctx.get_aws_config()?)
            .await?
            .enable_key_rotation(&key_arn)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to enable AWS KMS key rotation".to_string(),
                resource_id: Some(key.id.clone()),
            })?;
        Ok(HandlerAction::Continue {
            state: ApplyingGrants,
            suggested_delay: None,
        })
    }

    #[handler(state = ApplyingGrants, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn applying_grants(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.apply_grants(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if self.origin != super::KeyOrigin::Created || !super::may_write_keys(ctx) {
            // Imported keys belong to their template; after setup the manager has no
            // authority to change a key.
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }
        let key_arn = self.created_key_arn(&key.id)?;
        let policy = serde_json::to_string(&desired_policy(ctx, &key.id)?)
            .into_alien_error()
            .context(ErrorData::ResourceStateSerializationFailed {
                resource_id: key.id.clone(),
                message: "Failed to serialize the key policy".to_string(),
            })?;
        ctx.service_provider
            .get_aws_kms_client(ctx.get_aws_config()?)
            .await?
            .put_key_policy(&key_arn, &policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update the AWS KMS key policy".to_string(),
                resource_id: Some(key.id.clone()),
            })?;
        Ok(HandlerAction::Continue {
            state: UpdatingGrants,
            suggested_delay: None,
        })
    }

    #[handler(state = UpdatingGrants, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn updating_grants(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.apply_grants(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    /// Removes the grants this controller wrote and keeps the key: nothing in Alien deletes
    /// key material. The inline policies live on roles that teardown deletes, so without
    /// setup credentials there is nothing left to do.
    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if self.key_arn.is_none() {
            // Setup failed before the key existed (or never ran): nothing to keep or remove.
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
        if super::may_write_keys(ctx) && !self.grants.is_empty() {
            let iam = ctx
                .service_provider
                .get_aws_iam_client(ctx.get_aws_config()?)
                .await?;
            // An entry leaves the list only once its delete succeeded, so a retry after a
            // failure still knows the grants that are left.
            while let Some((role, policy)) = self.grants.last().cloned() {
                match iam.delete_role_policy(&role, &policy).await {
                    Ok(()) => {}
                    Err(error)
                        if matches!(
                            error.error,
                            Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                        ) => {}
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to remove key grant '{policy}' from role '{role}'"),
                            resource_id: Some(key.id.clone()),
                        }));
                    }
                }
                self.grants.pop();
            }
        }
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // A failed check keeps the last observation unless AWS answered about the
        // key itself; a throttle or 5xx must not pause every dependent resource.
        let config = ctx.desired_resource_config::<Key>()?;
        let key_arn = self.key_arn.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceStateSerializationFailed {
                resource_id: config.id.clone(),
                message: "Imported AWS key is missing its ARN".to_string(),
            })
        })?;
        let client = ctx
            .service_provider
            .get_aws_kms_client(ctx.get_aws_config()?)
            .await?;
        let metadata = client
            .describe_key(key_arn)
            .await
            .inspect_err(|error| {
                if super::aws_key_revoked(error) {
                    self.available = Some(false);
                }
            })
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read AWS KMS key metadata".to_string(),
                resource_id: Some(config.id.clone()),
            })?
            .key_metadata;
        if metadata.arn != key_arn {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "AWS KMS returned metadata for a different key".to_string(),
            }));
        }
        let mut usable = metadata.key_state == "Enabled" && metadata.enabled;
        if usable
            && self.availability_probe
            && metadata.origin.as_deref() == Some("EXTERNAL_KEY_STORE")
        {
            // DescribeKey cannot observe the external customer's key. Encrypting
            // public probe text exercises the gateway without permitting data reads.
            usable = alien_aws_clients::kms::probe_external_key(client.as_ref(), key_arn)
                .await
                .inspect_err(|error| {
                    if super::aws_key_revoked(error) {
                        self.available = Some(false);
                    }
                })
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to check customer key availability".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
        }
        self.available = Some(usable);
        let (health, lifecycle) = match metadata.key_state.as_str() {
            "Enabled" if usable => (ObservedHealth::Healthy, ProviderLifecycleState::Running),
            "Enabled" | "Disabled" => (ObservedHealth::Unhealthy, ProviderLifecycleState::Stopped),
            "PendingDeletion" | "PendingReplicaDeletion" => {
                (ObservedHealth::Unhealthy, ProviderLifecycleState::Deleting)
            }
            _ => (ObservedHealth::Degraded, ProviderLifecycleState::Unknown),
        };
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Key::RESOURCE_TYPE,
            controller_platform: Platform::Aws,
            backend: HeartbeatBackend::Aws,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Key(KeyHeartbeatData::AwsKms(AwsKmsKeyHeartbeatData {
                status: KeyHeartbeatStatus {
                    health,
                    lifecycle,
                    message: (self.available != Some(true))
                        .then(|| "customer key unavailable".to_string()),
                },
                key_arn: metadata.arn,
                key_state: metadata.key_state,
                enabled: metadata.enabled,
                key_spec: metadata.key_spec,
                key_usage: metadata.key_usage,
            })),
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
        let key_arn = self.key_arn.clone()?;
        Some(ResourceOutputs::new(KeyOutputs {
            // A key whose checks kept failing until RefreshFailed is no longer
            // observed, so its last value would never change again.
            available: Some(self.state == AwsKeyState::Ready && self.available == Some(true)),
            fingerprint: KeyFingerprint::Aws {
                key_arn: key_arn.clone(),
            },
            wrapping_key_id: key_arn,
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        let Some(key_arn) = &self.key_arn else {
            return Ok(None);
        };
        Ok(Some(
            serde_json::to_value(KeyBinding::aws_kms(key_arn, self.region.as_deref()))
                .into_alien_error()
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize Key binding parameters".to_string(),
                })?,
        ))
    }
}

impl AwsKeyController {
    /// A ready key for tests of resources that depend on it.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn mock_ready(key_arn: &str) -> Self {
        Self {
            state: AwsKeyState::Ready,
            key_arn: Some(key_arn.to_string()),
            origin: super::KeyOrigin::Created,
            ..Default::default()
        }
    }

    fn created_key_arn(&self, key_id: &str) -> Result<String> {
        self.key_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceStateSerializationFailed {
                resource_id: key_id.to_string(),
                message: "AWS key has no ARN yet".to_string(),
            })
        })
    }

    /// Puts the plan's grants on the management, access and runtime roles and removes
    /// the ones an earlier run wrote that the plan no longer has.
    async fn apply_grants(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let key_arn = self.created_key_arn(&key.id)?;
        let wanted = desired_grants(ctx, &key.id, &key_arn)?;
        let iam = ctx
            .service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?;
        for grant in &wanted {
            iam.put_role_policy(&grant.role, &grant.policy, &grant.document)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to grant key access to role '{}'", grant.role),
                    resource_id: Some(key.id.clone()),
                })?;
        }
        for (role, policy) in &self.grants {
            if !wanted.iter().any(|grant| &grant.role == role && &grant.policy == policy) {
                match iam.delete_role_policy(role, policy).await {
                    Ok(()) => {}
                    Err(error)
                        if matches!(
                            error.error,
                            Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                        ) => {}
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to remove stale key grant from role '{role}'"),
                            resource_id: Some(key.id.clone()),
                        }));
                    }
                }
            }
        }
        self.grants = wanted
            .into_iter()
            .map(|grant| (grant.role, grant.policy))
            .collect();
        self.access_revision = Some(desired_revision(ctx, &key.id)?);
        Ok(())
    }
}

struct RoleGrant {
    role: String,
    policy: String,
    document: String,
}

fn principals<'a>(
    ctx: &'a ResourceControllerContext<'_>,
    aws: &'a alien_core::AwsClientConfig,
) -> LiteralAwsPrincipals<'a> {
    LiteralAwsPrincipals {
        partition: crate::sandbox::aws_partition(&aws.region),
        account_id: &aws.account_id,
        prefix: ctx.resource_prefix,
    }
}

fn desired_policy(ctx: &ResourceControllerContext<'_>, key_id: &str) -> Result<serde_json::Value> {
    let aws = ctx.get_aws_config()?;
    let plan = key_access_plan(ctx.desired_stack, key_id).context(ErrorData::ResourceConfigInvalid {
        resource_id: Some(key_id.to_string()),
        message: "Failed to work out which roles may use the key".to_string(),
    })?;
    Ok(aws_key_policy_for_stack(
        &plan,
        &principals(ctx, aws),
    ))
}

fn desired_grants(
    ctx: &ResourceControllerContext<'_>,
    key_id: &str,
    key_arn: &str,
) -> Result<Vec<RoleGrant>> {
    let plan = key_access_plan(ctx.desired_stack, key_id).context(ErrorData::ResourceConfigInvalid {
        resource_id: Some(key_id.to_string()),
        message: "Failed to work out which roles may use the key".to_string(),
    })?;
    let grants = aws_key_grants(&plan, key_arn).context(ErrorData::ResourceConfigInvalid {
        resource_id: Some(key_id.to_string()),
        message: "Failed to render the key grants".to_string(),
    })?;
    let document = |statements: Vec<serde_json::Value>| {
        serde_json::json!({ "Version": "2012-10-17", "Statement": statements }).to_string()
    };
    let prefix = ctx.resource_prefix;
    let mut wanted = Vec::new();
    if plan.has_management_identity && !grants.management.is_empty() {
        wanted.push(RoleGrant {
            role: crate::remote_stack_management::aws_management_role_name(prefix),
            policy: format!("{key_id}-key-metadata"),
            document: document(grants.management.clone()),
        });
    }
    if plan.remote_bindings {
        wanted.push(RoleGrant {
            role: format!("{prefix}-access"),
            policy: format!("{key_id}-key-access"),
            document: document(vec![grants.cryptography.clone()]),
        });
    }
    for account in &plan.runtime_cryptography {
        wanted.push(RoleGrant {
            role: format!("{prefix}-{account}"),
            policy: format!("{key_id}-key-runtime"),
            document: document(vec![grants.cryptography.clone()]),
        });
    }
    // The remote access role writes objects of a Storage published through Remote Bindings.
    if plan.remote_storage_access {
        wanted.push(RoleGrant {
            role: format!("{prefix}-access"),
            policy: format!("{key_id}-key-native-storage"),
            document: document(vec![grants.native_storage.clone()]),
        });
    }
    // Roles that read or write objects of a Storage encrypted with this key.
    for account in &plan.native_storage_accounts {
        wanted.push(RoleGrant {
            role: format!("{prefix}-{account}"),
            policy: format!("{key_id}-key-native-storage"),
            document: document(vec![grants.native_storage.clone()]),
        });
    }
    Ok(wanted)
}

/// Changes when the policy, the grants or the tags a created key depends on change.
fn desired_revision(ctx: &ResourceControllerContext<'_>, key_id: &str) -> Result<String> {
    let policy = desired_policy(ctx, key_id)?.to_string();
    let mut grants = desired_grants(ctx, key_id, "key")?
        .into_iter()
        .map(|grant| format!("{}={}={}", grant.role, grant.policy, grant.document))
        .collect::<Vec<_>>();
    grants.sort();
    Ok(super::access_revision(&[&policy, &grants.join("\n")]))
}

fn key_tags(prefix: &str, key_id: &str) -> Vec<KmsTag> {
    let mut tags = alien_core::standard_resource_tags(prefix, key_id)
        .into_iter()
        .map(|(tag_key, tag_value)| KmsTag { tag_key, tag_value })
        .collect::<Vec<_>>();
    tags.sort_by(|a, b| a.tag_key.cmp(&b.tag_key));
    tags
}

/// The one key in this account and region that carries this deployment's tags, checked
/// against what Alien creates. `None` when there is none, an error when there are several.
async fn find_tagged_key(
    ctx: &ResourceControllerContext<'_>,
    tagging: &dyn ResourceGroupsTaggingApi,
    kms: &dyn KmsApi,
    key_id: &str,
) -> Result<Option<String>> {
    let aws = ctx.get_aws_config()?;
    let tags = alien_core::standard_resource_tags(ctx.resource_prefix, key_id);
    let mut candidates = Vec::new();
    let mut token = None;
    loop {
        let page = tagging
            .get_resources(
                GetResourcesRequest::builder()
                    .maybe_pagination_token(token.take())
                    .resource_type_filters(vec!["kms:key".to_string()])
                    .tag_filters(
                        tags.iter()
                            .map(|(key, value)| {
                                TagFilter::builder()
                                    .key(key.clone())
                                    .values(vec![value.clone()])
                                    .build()
                            })
                            .collect(),
                    )
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to look for an earlier key of this deployment".to_string(),
                resource_id: Some(key_id.to_string()),
            })?;
        candidates.extend(page.resource_tag_mapping_list.into_iter().map(|m| m.resource_arn));
        match page.pagination_token.filter(|token| !token.is_empty()) {
            Some(next) => token = Some(next),
            None => break,
        }
    }
    let mut ours = Vec::new();
    for arn in candidates {
        let metadata = kms
            .describe_key(&arn)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to read the tagged key '{arn}'"),
                resource_id: Some(key_id.to_string()),
            })?
            .key_metadata;
        let in_this_account = metadata.arn == arn
            && arn.contains(&format!(":kms:{}:{}:key/", aws.region, aws.account_id));
        // Alien creates symmetric encrypt/decrypt keys with its own key material.
        let made_like_ours = metadata.key_spec == "SYMMETRIC_DEFAULT"
            && metadata.key_usage == "ENCRYPT_DECRYPT"
            && metadata.origin.as_deref() == Some("AWS_KMS");
        if !in_this_account || !made_like_ours {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: key_id.to_string(),
                message: format!(
                    "Key '{arn}' has this deployment's tags but is not a key Alien creates (spec {}, usage {}, origin {:?}); refusing to adopt it",
                    metadata.key_spec, metadata.key_usage, metadata.origin
                ),
            }));
        }
        if matches!(metadata.key_state.as_str(), "PendingDeletion" | "PendingReplicaDeletion") {
            continue;
        }
        ours.push(arn);
    }
    match ours.len() {
        0 => Ok(None),
        1 => Ok(ours.pop()),
        _ => Err(AlienError::new(ErrorData::ResourceDrift {
            resource_id: key_id.to_string(),
            message: format!(
                "More than one key carries this deployment's tags ({}); delete the extra ones and rerun setup",
                ours.join(", ")
            ),
        })),
    }
}

#[derive(Debug, Default)]
pub struct AwsKeyImporter;

#[cfg(all(test, feature = "test"))]
#[path = "aws_tests.rs"]
mod tests;

impl ResourceImporter for AwsKeyImporter {
    type ImportData = AwsKeyImportData;

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
            AwsKeyController {
                state: AwsKeyState::Ready,
                available: None,
                availability_probe: data.availability_probe,
                key_arn: Some(data.key_arn),
                region: Some(ctx.region.to_string()),
                origin: super::KeyOrigin::Imported,
                create_requested: false,
                access_revision: None,
                grants: Vec::new(),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}
