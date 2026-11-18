//! What a direct setup run places around a sandbox's reasoning engine, matching what the setup
//! package places: the preview signing identity, the three grants on the engine, the signing
//! permission for the workload identities that execute in the sandbox, and the persistent storage
//! bucket with object access for those identities.
//!
//! The package and this module are two implementations of one setup. Each item here has the same
//! role, scope and member as its package twin (see `alien_core::sandbox_setup_parity`); an item
//! that exists only in the package leaves a direct install that works until the feature needing
//! it is used.

use std::collections::BTreeSet;

use alien_core::{ResourceRef, Sandbox};
use alien_error::{AlienError, Context};
use alien_gcp_clients::agent_platform::{
    bind_member, grant_on_engine, revoke_on_engine, unbind_member,
};
use alien_gcp_clients::iam::{CreateServiceAccountRequest, ServiceAccount as GcpServiceAccount};
use alien_permissions::generators::{
    GcpBindingResourceKind, GcpBindingTargetScope, GcpCustomRole, GcpRuntimePermissionsGenerator,
};
use alien_permissions::BindingTarget;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::{GcpCustomRoleNaming, ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use crate::service_account::GcpServiceAccountController;

const EXECUTE: &str = "sandbox/execute";
const TEMPLATES: &str = "sandbox/templates";
const PREVIEW_EDGE: &str = "sandbox/preview-edge";

/// The one permission a workload needs on the signing identity: `signJwt` and nothing else, so it
/// can ask for a short-lived JWT as that identity and mint no token or blob as it.
pub(crate) const SIGN_JWT_PERMISSION: &str = "iam.serviceAccounts.signJwt";

/// What this controller created for the engine, kept so its teardown removes exactly that.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EngineSetupRecord {
    /// The signing service account's email, set only when this controller created it.
    #[serde(default)]
    pub signer_created: Option<String>,
    /// Full name of the `signJwt` custom role this controller created or adopted.
    #[serde(default)]
    pub sign_role: Option<String>,
    /// `(role, member)` pairs this controller placed on the engine.
    #[serde(default)]
    pub engine_grants: Vec<(String, String)>,
    /// Members (`serviceAccount:<email>`) this controller let sign as the preview signer.
    #[serde(default)]
    pub sign_members: Vec<String>,
    /// The persistent storage bucket this controller created or took over by its owner label.
    /// Kept on teardown, like the package's retained bucket: it holds the keys' data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_bucket: Option<String>,
    /// Members this controller gave object access on that bucket.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub storage_members: Vec<String>,
}

/// The role the package grants each execute owner on the persistent storage bucket.
pub(crate) const STORAGE_OBJECT_ROLE: &str = "roles/storage.objectUser";

/// The bucket label a direct setup run marks its persistent storage bucket with: a label value
/// cannot hold the owner text, so it holds a digest of it.
pub(crate) const STORAGE_OWNER_LABEL: &str = "alien-setup-owner";

/// The persistent storage bucket's name, as the package names it.
pub(crate) fn persistent_storage_bucket_name(resource_prefix: &str, sandbox_id: &str) -> String {
    format!("{resource_prefix}-{sandbox_id}-storage")
}

fn storage_owner_label_value(owner: &str) -> String {
    let digest = Sha256::digest(owner.as_bytes());
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// Creates the persistent storage bucket and gives each identity that executes in the sandbox
/// object access, as the package does, and returns the bucket. `None` when the sandbox declares
/// no persistent storage. A declared encryption key is refused: only the package grants the
/// storage service its use of the key.
pub(crate) async fn set_up_persistent_store(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
    record: &mut EngineSetupRecord,
) -> Result<Option<String>> {
    let Some(storage) = &sandbox.persistent_storage else {
        return Ok(None);
    };
    if storage.encryption_key.is_some() {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "sandbox '{}' encrypts its persistent storage with a key, which a direct setup \
                 run cannot grant the storage service; install from the Terraform setup package",
                sandbox.id
            ),
            resource_id: Some(sandbox.id.clone()),
        }));
    }
    let gcp_config = ctx.get_gcp_config()?;
    let gcs = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;
    let name = persistent_storage_bucket_name(ctx.resource_prefix, &sandbox.id);
    if record.storage_bucket.as_deref() != Some(name.as_str()) {
        let owner = storage_owner_label_value(&crate::setup_owner::owner_value(
            ctx.resource_prefix,
            &sandbox.id,
            ctx.setup_owner_deployment_id()?,
        ));
        let bucket = alien_gcp_clients::gcs::Bucket {
            location: Some(gcp_config.region.clone()),
            storage_class: Some("STANDARD".to_string()),
            iam_configuration: Some(alien_gcp_clients::gcs::IamConfiguration {
                uniform_bucket_level_access: Some(
                    alien_gcp_clients::gcs::UniformBucketLevelAccess {
                        enabled: true,
                        locked_time: None,
                    },
                ),
                public_access_prevention: Some("enforced".to_string()),
            }),
            labels: Some([(STORAGE_OWNER_LABEL.to_string(), owner.clone())].into()),
            ..Default::default()
        };
        match gcs.create_bucket(name.clone(), bucket).await {
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                // Taken over only when it carries this deployment's label, which proves an
                // earlier attempt of this setup made it.
                let existing =
                    gcs.get_bucket(name.clone())
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!("Failed to read the existing bucket {name}"),
                            resource_id: Some(sandbox.id.clone()),
                        })?;
                let ours = existing
                    .labels
                    .as_ref()
                    .and_then(|labels| labels.get(STORAGE_OWNER_LABEL))
                    .is_some_and(|value| *value == owner);
                if !ours {
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "bucket {name} already exists and this deployment did not create \
                             it, so setup will not keep persistent storage in it. Delete or \
                             rename it, or install from the Terraform setup package."
                        ),
                        resource_id: Some(sandbox.id.clone()),
                    }));
                }
            }
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create the persistent storage bucket {name}"),
                    resource_id: Some(sandbox.id.clone()),
                });
            }
        }
        record.storage_bucket = Some(name.clone());
    }

    let wanted: Vec<String> = execute_owner_emails(ctx, &sandbox.id)?
        .into_iter()
        .map(|owner| format!("serviceAccount:{owner}"))
        .collect();
    let dropped: Vec<String> = record
        .storage_members
        .iter()
        .filter(|member| !wanted.contains(member))
        .cloned()
        .collect();
    let added: Vec<&String> = wanted
        .iter()
        .filter(|member| !record.storage_members.contains(member))
        .collect();
    if !added.is_empty() || !dropped.is_empty() {
        let mut policy = gcs.get_bucket_iam_policy(name.clone()).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to read the IAM policy of bucket {name}"),
                resource_id: Some(sandbox.id.clone()),
            },
        )?;
        // Old and new stay recorded until the write lands, so a failure part-way still lets
        // teardown remove what may have been bound.
        let mut remembered = record.storage_members.clone();
        for member in &wanted {
            if !remembered.contains(member) {
                remembered.push(member.clone());
            }
        }
        record.storage_members = remembered;
        let mut changed = false;
        for member in &wanted {
            changed |= bind_member(&mut policy, STORAGE_OBJECT_ROLE, member);
        }
        for member in &dropped {
            changed |= unbind_member(&mut policy, STORAGE_OBJECT_ROLE, member);
        }
        if changed {
            gcs.set_bucket_iam_policy(name.clone(), policy)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to give the sandbox's workloads access to {name}"),
                    resource_id: Some(sandbox.id.clone()),
                })?;
        }
    }
    record.storage_members = wanted;
    Ok(Some(name))
}

/// Revokes the object access [`set_up_persistent_store`] recorded. The bucket stays: it holds
/// the keys' data, and the package keeps it on uninstall too.
async fn tear_down_persistent_store(
    ctx: &ResourceControllerContext<'_>,
    record: &EngineSetupRecord,
) -> Result<()> {
    let (Some(name), false) = (&record.storage_bucket, record.storage_members.is_empty()) else {
        return Ok(());
    };
    let gcp_config = ctx.get_gcp_config()?;
    let gcs = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;
    let mut policy = match gcs.get_bucket_iam_policy(name.clone()).await {
        Ok(policy) => policy,
        Err(error)
            if matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(());
        }
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to read the IAM policy of bucket {name}"),
                resource_id: None,
            })
        }
    };
    let mut changed = false;
    for member in &record.storage_members {
        changed |= unbind_member(&mut policy, STORAGE_OBJECT_ROLE, member);
    }
    if changed {
        gcs.set_bucket_iam_policy(name.clone(), policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to remove object access on bucket {name}"),
                resource_id: None,
            })?;
    }
    Ok(())
}

/// Text in the signer's description that only this deployment's setup writes; it is how a
/// retry tells its own earlier signer from an account somebody else made under the same name.
pub(crate) fn signer_marker(
    resource_prefix: &str,
    sandbox_id: &str,
    deployment_id: &str,
) -> String {
    crate::setup_owner::description_marker(&crate::setup_owner::owner_value(
        resource_prefix,
        sandbox_id,
        deployment_id,
    ))
}

/// Account id of the signing identity: readable, at most 30 characters, deterministic.
pub(crate) fn preview_signer_account_id(resource_prefix: &str, sandbox_id: &str) -> String {
    let full = format!("{resource_prefix}-{sandbox_id}-preview")
        .to_lowercase()
        .replace('_', "-");
    if full.len() <= 30 {
        return full.trim_matches('-').to_string();
    }
    let digest = Sha256::digest(full.as_bytes());
    let hash: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    let head: String = full.chars().take(21).collect();
    format!("{}-{hash}", head.trim_matches('-'))
}

fn missing(config_id: &str, what: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::DependencyNotReady {
        resource_id: config_id.to_string(),
        dependency_id: what.to_string(),
    })
}

/// Emails of the workload identities whose profile grants `sandbox/execute` on this sandbox, by
/// name or through the wildcard. Mirrors the package's `execute_owner_labels`.
fn execute_owner_emails(
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
) -> Result<Vec<String>> {
    let mut emails = Vec::new();
    for (profile_name, profile) in ctx.desired_stack.permission_profiles() {
        let granted = [sandbox_id, "*"].iter().any(|key| {
            profile
                .0
                .get(*key)
                .is_some_and(|refs| refs.iter().any(|reference| reference.id() == EXECUTE))
        });
        if !granted {
            continue;
        }
        let account_id = format!("{profile_name}-sa");
        let Some(entry) = ctx.desired_stack.resources.get(&account_id) else {
            continue;
        };
        let controller = ctx
            .require_dependency::<GcpServiceAccountController>(&ResourceRef::from(&entry.config))?;
        let email = controller
            .service_account_email
            .ok_or_else(|| missing(sandbox_id, &account_id))?;
        emails.push(email);
    }
    Ok(emails)
}

/// `(role, member)` grants on the engine for one permission set, as the package emits them: the
/// plan's engine-scoped bindings only, custom roles ensured first.
async fn engine_grants_for(
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
    engine: &str,
    permission_set_id: &str,
    member_email: &str,
) -> Result<Vec<(String, String)>> {
    let permission_set = alien_permissions::get_permission_set(permission_set_id)
        .cloned()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("{permission_set_id} permission set is not registered"),
                resource_id: Some(sandbox_id.to_string()),
            })
        })?;
    let context = ResourcePermissionsHelper::build_gcp_permission_context(ctx, engine)?;
    let plan = GcpRuntimePermissionsGenerator::new()
        .generate_grant_plan(&permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to generate the {permission_set_id} engine grant"),
            resource_id: Some(sandbox_id.to_string()),
        })?;
    let bindings: Vec<_> = plan
        .bindings_for_target(GcpBindingTargetScope::CurrentResource)
        .into_iter()
        .filter(|b| b.resource_kind == Some(GcpBindingResourceKind::VertexAiReasoningEngine))
        .collect();
    ResourcePermissionsHelper::ensure_gcp_custom_roles_for_bindings(
        ctx,
        permission_set_id,
        &plan,
        &bindings,
    )
    .await?;
    let member = format!("serviceAccount:{member_email}");
    Ok(bindings
        .into_iter()
        .map(|b| (b.role, member.clone()))
        .collect())
}

/// Creates the signing identity (when preview ports are declared) and places every grant the
/// package places on the engine. Safe to repeat: a retry finds each piece in place.
///
/// `record` is updated before each cloud write so a failure part-way still lets teardown remove
/// what was made.
pub(crate) async fn set_up_engine_access(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
    engine: &str,
    record: &mut EngineSetupRecord,
) -> Result<Option<String>> {
    let gcp_config = ctx.get_gcp_config()?;
    let mut grants: Vec<(String, String)> = Vec::new();
    let mut signer_email = None;

    if !sandbox.preview_ports.is_empty() {
        let iam = ctx.service_provider.get_gcp_iam_client(gcp_config)?;
        let account_id = preview_signer_account_id(ctx.resource_prefix, &sandbox.id);
        let email = format!(
            "{account_id}@{}.iam.gserviceaccount.com",
            gcp_config.project_id
        );
        let owner = crate::setup_owner::owner_value(
            ctx.resource_prefix,
            &sandbox.id,
            ctx.setup_owner_deployment_id()?,
        );
        let marker = crate::setup_owner::description_marker(&owner);
        let created = iam
            .create_service_account(
                account_id.clone(),
                CreateServiceAccountRequest::builder()
                    .service_account(
                        GcpServiceAccount::builder()
                            .display_name("Preview edge signer".to_string())
                            .description(format!(
                                "Signs the short-lived credential the Agent Platform edge checks \
                                 for preview. Holds the execute verb on this deployment's engine \
                                 and nothing else. {marker}"
                            ))
                            .build(),
                    )
                    .build(),
            )
            .await;
        match created {
            Ok(_) => {
                record.signer_created = Some(email.clone());
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                // Taken over only when it carries this deployment's marker, which proves an
                // earlier attempt of this setup made it. Teardown deletes only what is recorded.
                let existing = iam.get_service_account(email.clone()).await.context(
                    ErrorData::CloudPlatformError {
                        message: format!("Failed to read the existing service account {email}"),
                        resource_id: Some(sandbox.id.clone()),
                    },
                )?;
                if existing.description.as_deref().is_some_and(|description| {
                    crate::setup_owner::description_names_owner(description, &owner)
                }) {
                    record.signer_created = Some(email.clone());
                } else {
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "service account {email} already exists and this deployment did \
                             not create it. Setup will not take it over, because destroying \
                             the deployment would delete it. Delete it, or install from the \
                             Terraform setup package, which imports it."
                        ),
                        resource_id: Some(sandbox.id.clone()),
                    }));
                }
            }
            Err(error) => {
                return Err(
                    ResourcePermissionsHelper::gcp_service_account_creation_error(
                        error,
                        &gcp_config.project_id,
                    ),
                );
            }
        }
        grants.extend(engine_grants_for(ctx, &sandbox.id, engine, PREVIEW_EDGE, &email).await?);

        // The role holding only signJwt, project-wide with a fixed id, bound on the signer for
        // each identity that executes in this sandbox.
        let owners = execute_owner_emails(ctx, &sandbox.id)?;
        let wanted: Vec<String> = owners
            .iter()
            .map(|owner| format!("serviceAccount:{owner}"))
            .collect();
        let dropped: Vec<String> = record
            .sign_members
            .iter()
            .filter(|member| !wanted.contains(member))
            .cloned()
            .collect();
        if !owners.is_empty() || !dropped.is_empty() {
            let naming = GcpCustomRoleNaming::for_deployment(ctx.state)?;
            let role_id = format!(
                "role_{}_sandbox_preview_sign",
                naming.namespace(ctx.resource_prefix)
            );
            let role_name = format!("projects/{}/roles/{role_id}", gcp_config.project_id);
            if !owners.is_empty() {
                ResourcePermissionsHelper::ensure_gcp_custom_roles(
                    ctx,
                    PREVIEW_EDGE,
                    vec![GcpCustomRole {
                        role_id: role_id.clone(),
                        name: role_name.clone(),
                        title: "Sign the preview edge credential".to_string(),
                        description: format!(
                            "Sign a JWT as the preview edge signer. Granted on that one service \
                             account. Resource prefix: {}.",
                            ctx.resource_prefix
                        ),
                        included_permissions: vec![SIGN_JWT_PERMISSION.to_string()],
                        stage: "GA".to_string(),
                    }],
                )
                .await?;
                record.sign_role = Some(role_name.clone());
            }
            let mut policy = iam
                .get_service_account_iam_policy(email.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to read the preview signer's IAM policy".to_string(),
                    resource_id: Some(sandbox.id.clone()),
                })?;
            // Remember old and new until the write lands, so a failure part-way still lets
            // teardown (or the next run) remove what was bound.
            let mut remembered = record.sign_members.clone();
            for member in &wanted {
                if !remembered.contains(member) {
                    remembered.push(member.clone());
                }
            }
            record.sign_members = remembered;
            let mut changed = false;
            for member in &wanted {
                changed |= bind_member(&mut policy, &role_name, member);
            }
            for member in &dropped {
                changed |= unbind_member(&mut policy, &role_name, member);
            }
            if changed {
                policy.version = Some(3);
                iam.set_service_account_iam_policy(email.clone(), policy)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to update who may sign as the preview signer".to_string(),
                        resource_id: Some(sandbox.id.clone()),
                    })?;
            }
            record.sign_members = wanted;
        }
        signer_email = Some(email);
    }

    // Remote caller: the engine grant of the stack's Remote Bindings identity.
    if let Some(entry) = ctx.desired_stack.resources.get(&sandbox.id) {
        if alien_core::remote_bindings::remote_binding_is_deliverable(entry) {
            if let Some(definition) = alien_core::remote_bindings::remote_binding_for_entry(entry) {
                if let Some((_, identity)) = ctx.desired_stack.resources.iter().find(|(_, e)| {
                    e.config.resource_type() == alien_core::RemoteBindings::RESOURCE_TYPE
                }) {
                    let controller = ctx
                        .require_dependency::<crate::remote_bindings::GcpRemoteBindingsController>(
                            &ResourceRef::from(&identity.config),
                        )?;
                    let email = controller
                        .service_account_email
                        .ok_or_else(|| missing(&sandbox.id, "remote-bindings"))?;
                    grants.extend(
                        engine_grants_for(
                            ctx,
                            &sandbox.id,
                            engine,
                            definition.permission_set,
                            &email,
                        )
                        .await?,
                    );
                }
            }
        }
    }

    // Management: template replacement, when a remote manager identity exists and the
    // management profile grants it on this sandbox.
    let management_granted = ctx
        .desired_stack
        .management()
        .profile()
        .and_then(|profile| profile.0.get(&sandbox.id))
        .is_some_and(|refs| refs.iter().any(|reference| reference.id() == TEMPLATES));
    if management_granted {
        if let Some(email) =
            ResourcePermissionsHelper::get_gcp_management_service_account_email(ctx)?
        {
            grants.extend(engine_grants_for(ctx, &sandbox.id, engine, TEMPLATES, &email).await?);
        }
    }

    let distinct: BTreeSet<(String, String)> = grants.into_iter().collect();
    let grants: Vec<(String, String)> = distinct.into_iter().collect();
    let revoked: Vec<(String, String)> = record
        .engine_grants
        .iter()
        .filter(|grant| !grants.contains(grant))
        .cloned()
        .collect();
    if !grants.is_empty() || !revoked.is_empty() {
        let platform = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;
        // Old and new stay recorded until both writes land, so a failure part-way still lets
        // teardown remove everything that may have been placed.
        let mut remembered = record.engine_grants.clone();
        for grant in &grants {
            if !remembered.contains(grant) {
                remembered.push(grant.clone());
            }
        }
        record.engine_grants = remembered;
        grant_on_engine(platform.as_ref(), engine, &grants)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to place the sandbox grants on the engine".to_string(),
                resource_id: Some(sandbox.id.clone()),
            })?;
        // A profile that lost its sandbox verbs loses the grants they gave.
        revoke_on_engine(platform.as_ref(), engine, &revoked)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to remove engine grants the stack no longer gives".to_string(),
                resource_id: Some(sandbox.id.clone()),
            })?;
        record.engine_grants = grants;
    }
    Ok(signer_email)
}

/// Removes what [`set_up_engine_access`] recorded. Each step tolerates the piece being gone.
pub(crate) async fn tear_down_engine_access(
    ctx: &ResourceControllerContext<'_>,
    engine: &str,
    record: &EngineSetupRecord,
) -> Result<()> {
    let gcp_config = ctx.get_gcp_config()?;
    tear_down_persistent_store(ctx, record).await?;
    if !record.engine_grants.is_empty() {
        let platform = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;
        // A failed revoke is returned, with the record kept, so the delete is retried rather
        // than forgetting a grant that still names a removed identity.
        revoke_on_engine(platform.as_ref(), engine, &record.engine_grants)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to remove the sandbox grants from the engine".to_string(),
                resource_id: None,
            })?;
    }
    if record.signer_created.is_none() {
        return Ok(());
    }
    let iam = ctx.service_provider.get_gcp_iam_client(gcp_config)?;
    if let Some(email) = &record.signer_created {
        if let Err(error) = iam.delete_service_account(email.clone()).await {
            if !matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) {
                // Returned so the delete retries; the record stays until the signer is gone.
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete the preview signer {email}"),
                    resource_id: None,
                });
            }
        }
    }
    // Note: when several sandboxes of one deployment are deleted at once, each sees the others'
    // signers still present, so the shared role is left to the deployment-wide role cleanup.
    // The signing role is one project-wide role shared by every preview sandbox of the
    // deployment, so one sandbox's teardown must not delete it. Its description names the
    // deployment prefix, so the deployment-wide custom role cleanup removes it.
    Ok(())
}

/// Deletes the preview signer when its description carries this deployment's marker, for a
/// teardown whose state lost `EngineSetupRecord`. An account without the marker (made by the
/// Terraform package or by somebody else) is left alone.
pub(crate) async fn remove_marked_signer(
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
) -> Result<()> {
    let gcp_config = ctx.get_gcp_config()?;
    let iam = ctx.service_provider.get_gcp_iam_client(gcp_config)?;
    let account_id = preview_signer_account_id(ctx.resource_prefix, sandbox_id);
    let email = format!(
        "{account_id}@{}.iam.gserviceaccount.com",
        gcp_config.project_id
    );
    let owner = crate::setup_owner::owner_value(
        ctx.resource_prefix,
        sandbox_id,
        ctx.setup_owner_deployment_id()?,
    );
    let existing = match iam.get_service_account(email.clone()).await {
        Ok(account) => account,
        Err(error)
            if matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(());
        }
        Err(error) => Err(error).context(ErrorData::CloudPlatformError {
            message: format!("Failed to read service account {email} to check ownership"),
            resource_id: Some(sandbox_id.to_string()),
        })?,
    };
    if existing
        .description
        .as_deref()
        .is_some_and(|text| crate::setup_owner::description_names_owner(text, &owner))
    {
        if let Err(error) = iam.delete_service_account(email.clone()).await {
            if !matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete the preview signer {email}"),
                    resource_id: Some(sandbox_id.to_string()),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signer_account_id_fits_the_limit_and_is_stable() {
        let short = preview_signer_account_id("r0ea24e8", "box");
        assert_eq!(short, "r0ea24e8-box-preview");
        let long = preview_signer_account_id("r0ea24e8", "a-very-long-sandbox-identifier");
        assert!(long.len() <= 30, "{long}");
        assert_eq!(
            long,
            preview_signer_account_id("r0ea24e8", "a-very-long-sandbox-identifier")
        );
        assert_ne!(
            long,
            preview_signer_account_id("r0ea24e8", "a-very-long-sandbox-identifieR2")
        );
    }
}
