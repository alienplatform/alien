//! What a direct setup creates for a Frozen Google Cloud sandbox: the repository its image is
//! copied into, and the manager's write access to that one repository.
//!
//! The manager's runtime identity copies each new image into the repository (the template only
//! accepts an image the creating identity can read) but is never granted what creating a
//! repository or changing who may use it takes. Setup holds the deployer's credentials, so it
//! makes both here and records them; teardown deletes exactly what is recorded.
//!
//! One mutating call per step, so a retry after any failure repeats nothing.

use std::collections::{BTreeMap, HashMap};

use alien_core::gar_repository::{
    sandbox_image_repository, sandbox_image_write_role_id, SANDBOX_IMAGE_WRITE_PERMISSIONS,
};
use alien_core::{ClientConfig, Platform, ResourceStatus, SetupScaffolding, StackState};
use alien_error::{AlienError, Context};
use alien_gcp_clients::artifactregistry::{
    CleanupPolicy, CleanupPolicyAction, CleanupPolicyCondition, CleanupPolicyConditionTagState,
    Repository, RepositoryFormat,
};
use alien_gcp_clients::iam::Binding;
use alien_gcp_clients::GcpClientConfig;
use alien_permissions::generators::GcpCustomRole;
use tracing::info;

use super::{ScaffoldingProgress, SetupScaffoldingContext};
use crate::core::state_utils::StackResourceStateExt as _;
use crate::remote_stack_management::GcpRemoteStackManagementController;
use crate::{ErrorData, Result};

/// Label every repository this step creates carries. A repository of the same name without it is
/// someone else's: it is never adopted, so teardown never deletes it.
const OWNER_LABEL: &str = "alien-setup-scaffolding";

/// The repository's cleanup policy: untagged versions go after a week. Every copied digest is
/// tagged, so a digest a live template uses is never cleaned. The same policy as the controller's
/// and the Terraform module's.
pub(crate) fn image_repository_body(sandbox_id: &str, owner: Option<&str>) -> Repository {
    let policy = CleanupPolicy {
        id: Some("delete-untagged".to_string()),
        action: Some(CleanupPolicyAction::Delete),
        condition: Some(CleanupPolicyCondition {
            tag_state: Some(CleanupPolicyConditionTagState::Untagged),
            tag_prefixes: Vec::new(),
            version_name_prefixes: Vec::new(),
            package_name_prefixes: Vec::new(),
            older_than: Some("604800s".to_string()),
            newer_than: None,
        }),
        most_recent_versions: None,
    };
    Repository {
        format: Some(RepositoryFormat::Docker),
        description: Some(format!("Image of sandbox '{sandbox_id}'")),
        labels: owner.map(|owner| HashMap::from([(OWNER_LABEL.to_string(), owner.to_string())])),
        cleanup_policies: Some(HashMap::from([("delete-untagged".to_string(), policy)])),
        // A tag is made from the digest it names and never moves, so no tag update is needed and
        // none is allowed: moving one would leave a live digest untagged.
        repository_config: Some(
            alien_gcp_clients::artifactregistry::RepositoryConfig::DockerConfig(
                alien_gcp_clients::artifactregistry::DockerRepositoryConfig {
                    immutable_tags: Some(true),
                },
            ),
        ),
        ..Default::default()
    }
}

fn gcp_config(client_config: &ClientConfig) -> Result<&GcpClientConfig> {
    match client_config {
        ClientConfig::Gcp(config) => Ok(config.as_ref()),
        other => Err(AlienError::new(ErrorData::ClientConfigMismatch {
            required_platform: Platform::Gcp,
            found_platform: other.platform(),
        })),
    }
}

fn is_not_found(error: &AlienError<alien_client_core::ErrorData>) -> bool {
    matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
    )
}

fn names(ctx: &SetupScaffoldingContext<'_>, sandbox_id: &str) -> Result<(String, String)> {
    let invalid = |what: &str| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "'{}' and sandbox '{sandbox_id}' do not make a valid {what}",
                ctx.resource_prefix
            ),
            resource_id: Some(sandbox_id.to_string()),
        })
    };
    let repository = sandbox_image_repository(ctx.resource_prefix, sandbox_id)
        .ok_or_else(|| invalid("repository id"))?;
    let role = sandbox_image_write_role_id(ctx.resource_prefix, sandbox_id)
        .ok_or_else(|| invalid("custom role id"))?;
    Ok((repository, role))
}

/// The management service account's email once its controller has created it. `None` while it is
/// still being created: a grant naming an account that does not exist yet is refused.
fn management_member(stack_state: &StackState) -> Result<Option<String>> {
    for state in stack_state.resources.values() {
        if state.resource_type != alien_core::RemoteStackManagement::RESOURCE_TYPE.to_string()
            || state.internal_state.is_none()
        {
            continue;
        }
        let controller =
            state.get_internal_controller_typed::<GcpRemoteStackManagementController>()?;
        if let Some(email) = controller.service_account_email {
            return Ok(Some(format!("serviceAccount:{email}")));
        }
        if state.status == ResourceStatus::Running {
            break;
        }
    }
    Ok(None)
}

pub(super) async fn reconcile(
    ctx: &SetupScaffoldingContext<'_>,
    stack_state: &StackState,
    sandbox_id: &str,
    records: &mut BTreeMap<String, SetupScaffolding>,
) -> Result<ScaffoldingProgress> {
    let gcp = gcp_config(ctx.client_config)?;
    let (repository, role_id) = names(ctx, sandbox_id)?;
    let Some(member) = management_member(stack_state)? else {
        return Ok(ScaffoldingProgress::InProgress);
    };
    let (project, region) = (gcp.project_id.clone(), gcp.region.clone());
    let cloud_error = |message: String| ErrorData::CloudPlatformError {
        message,
        resource_id: Some(sandbox_id.to_string()),
    };
    let registry = ctx.service_provider.get_gcp_artifact_registry_client(gcp)?;

    // 1. The repository. Recorded before the create call, so a crash after it still tears it down.
    match registry
        .get_repository(project.clone(), region.clone(), repository.clone())
        .await
    {
        Ok(existing) => {
            let ours = existing
                .labels
                .as_ref()
                .and_then(|labels| labels.get(OWNER_LABEL))
                .is_some_and(|owner| owner == ctx.resource_prefix);
            if !ours {
                return Err(AlienError::new(ErrorData::SetupScaffoldingNotAdoptable {
                    resource_id: sandbox_id.to_string(),
                    object: format!("Artifact Registry repository '{repository}'"),
                    reason: format!(
                        "it does not carry setup's '{OWNER_LABEL}' label. Delete or rename it, \
                         then run setup again."
                    ),
                }));
            }
        }
        Err(error) if is_not_found(&error) => {
            record(records, sandbox_id, &repository, &role_id);
            registry
                .create_repository(
                    project.clone(),
                    region.clone(),
                    repository.clone(),
                    image_repository_body(sandbox_id, Some(ctx.resource_prefix)),
                )
                .await
                .context(cloud_error(format!(
                    "Failed to create the sandbox image repository '{repository}'"
                )))?;
            info!(sandbox_id, %repository, "Created the sandbox image repository");
            return Ok(ScaffoldingProgress::InProgress);
        }
        Err(error) => {
            return Err(error).context(cloud_error(format!(
                "Failed to read the sandbox image repository '{repository}'"
            )))
        }
    }
    record(records, sandbox_id, &repository, &role_id);

    // 2. The role: no delete of any kind.
    let iam = ctx.service_provider.get_gcp_iam_client(gcp)?;
    let role = GcpCustomRole {
        name: format!("projects/{project}/roles/{role_id}"),
        role_id: role_id.clone(),
        title: "Copy a sandbox image into its repository".to_string(),
        description: "Read, upload and tag in a sandbox image repository. Cannot delete."
            .to_string(),
        included_permissions: SANDBOX_IMAGE_WRITE_PERMISSIONS
            .iter()
            .map(|permission| permission.to_string())
            .collect(),
        stage: "GA".to_string(),
    };
    crate::core::ensure_gcp_custom_role(iam.as_ref(), sandbox_id, &role).await?;

    // 3. The grant, on the repository alone, added behind its etag.
    let mut policy = registry
        .get_repository_iam_policy(project.clone(), region.clone(), repository.clone())
        .await
        .context(cloud_error(format!(
            "Failed to read the IAM policy of repository '{repository}'"
        )))?;
    let role_name = role.name.clone();
    let granted = policy
        .bindings
        .iter()
        .any(|binding| binding.role == role_name && binding.members.contains(&member));
    if !granted {
        match policy
            .bindings
            .iter_mut()
            .find(|binding| binding.role == role_name && binding.condition.is_none())
        {
            Some(binding) => binding.members.push(member),
            None => policy.bindings.push(Binding {
                role: role_name,
                members: vec![member],
                condition: None,
            }),
        }
        policy.version = Some(3);
        registry
            .set_repository_iam_policy(project, region, repository.clone(), policy)
            .await
            .context(cloud_error(format!(
                "Failed to let the manager write to repository '{repository}'"
            )))?;
        info!(sandbox_id, %repository, "Granted the manager write access to the sandbox image repository");
        return Ok(ScaffoldingProgress::InProgress);
    }
    if let Some(SetupScaffolding::GcpSandbox { writer_granted, .. }) = records.get_mut(sandbox_id) {
        if !std::mem::replace(writer_granted, true) {
            info!(
            sandbox_id,
            %repository,
            role = %role_id,
            manager = %member,
            "Setup made the sandbox image repository and granted the manager write (no delete) on it"
                );
        }
    }
    Ok(ScaffoldingProgress::Done)
}

/// The repository a crashed step created without saving its record, found by name and setup's
/// owner label. One without the label is never claimed: teardown deletes a recorded repository.
pub(super) async fn recover(
    ctx: &SetupScaffoldingContext<'_>,
    sandbox_id: &str,
) -> Result<Option<SetupScaffolding>> {
    let gcp = gcp_config(ctx.client_config)?;
    let (repository, role_id) = names(ctx, sandbox_id)?;
    let registry = ctx.service_provider.get_gcp_artifact_registry_client(gcp)?;
    let existing = match registry
        .get_repository(
            gcp.project_id.clone(),
            gcp.region.clone(),
            repository.clone(),
        )
        .await
    {
        Ok(existing) => existing,
        Err(error) if is_not_found(&error) => return Ok(None),
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to read the sandbox image repository '{repository}'"),
                resource_id: Some(sandbox_id.to_string()),
            })
        }
    };
    let ours = existing
        .labels
        .as_ref()
        .and_then(|labels| labels.get(OWNER_LABEL))
        .is_some_and(|owner| owner == ctx.resource_prefix);
    if !ours {
        return Ok(None);
    }
    info!(sandbox_id, %repository, "Recovered setup scaffolding the record did not hold");
    Ok(Some(SetupScaffolding::GcpSandbox {
        image_repository: repository,
        write_role: role_id,
        writer_granted: false,
    }))
}

fn record(
    records: &mut BTreeMap<String, SetupScaffolding>,
    sandbox_id: &str,
    repository: &str,
    role_id: &str,
) {
    records
        .entry(sandbox_id.to_string())
        .or_insert_with(|| SetupScaffolding::GcpSandbox {
            image_repository: repository.to_string(),
            write_role: role_id.to_string(),
            writer_granted: false,
        });
}

/// Deletes the repository (its policy goes with it), then the role. Done once both are gone.
pub(super) async fn teardown(
    ctx: &SetupScaffoldingContext<'_>,
    sandbox_id: &str,
    image_repository: &str,
    write_role: &str,
) -> Result<ScaffoldingProgress> {
    let gcp = gcp_config(ctx.client_config)?;
    let (project, region) = (gcp.project_id.clone(), gcp.region.clone());
    let cloud_error = |message: String| ErrorData::CloudPlatformError {
        message,
        resource_id: Some(sandbox_id.to_string()),
    };
    let registry = ctx.service_provider.get_gcp_artifact_registry_client(gcp)?;
    match registry
        .get_repository(
            project.clone(),
            region.clone(),
            image_repository.to_string(),
        )
        .await
    {
        Ok(existing)
            if !existing
                .labels
                .as_ref()
                .and_then(|labels| labels.get(OWNER_LABEL))
                .is_some_and(|owner| owner == ctx.resource_prefix) =>
        {
            // Someone replaced it after setup made it; it is not ours to delete, by name or not.
            tracing::warn!(
                id = %sandbox_id, repository = %image_repository,
                "the sandbox image repository no longer carries setup's label; leaving it"
            );
        }
        Ok(_) => {
            registry
                .delete_repository(project, region, image_repository.to_string())
                .await
                .context(cloud_error(format!(
                    "Failed to delete the sandbox image repository '{image_repository}'"
                )))?;
            return Ok(ScaffoldingProgress::InProgress);
        }
        Err(error) if is_not_found(&error) => {}
        Err(error) => {
            return Err(error).context(cloud_error(format!(
                "Failed to read the sandbox image repository '{image_repository}'"
            )))
        }
    }
    let iam = ctx.service_provider.get_gcp_iam_client(gcp)?;
    match iam.delete_role(write_role.to_string()).await {
        Ok(_) => {}
        Err(error) if is_not_found(&error) => {}
        Err(error) => {
            return Err(error).context(cloud_error(format!(
                "Failed to delete the custom role '{write_role}'"
            )))
        }
    }
    Ok(ScaffoldingProgress::Done)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_core::{RemoteStackManagement, ResourceLifecycle, StackResourceState};
    use alien_gcp_clients::artifactregistry::MockArtifactRegistryApi;
    use alien_gcp_clients::iam::{IamPolicy, MockIamApi, Role};
    use alien_gcp_clients::longrunning::Operation;
    use alien_gcp_clients::GcpClientConfigExt;

    use super::*;
    use crate::MockPlatformServiceProvider;

    const PREFIX: &str = "acme";
    const MANAGER: &str = "serviceAccount:acme-management@test-project-123.iam.gserviceaccount.com";

    fn client_config() -> ClientConfig {
        ClientConfig::Gcp(Box::new(GcpClientConfig::mock()))
    }

    fn state(email: Option<&str>) -> StackState {
        let mut state = StackState::new(Platform::Gcp);
        state.resource_prefix = PREFIX.to_string();
        let mut resource = StackResourceState::builder()
            .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(alien_core::Resource::new(
                RemoteStackManagement::new("management".to_string()).build(),
            ))
            .lifecycle(ResourceLifecycle::Frozen)
            .build();
        resource
            .set_internal_controller(Some(Box::new(GcpRemoteStackManagementController {
                service_account_email: email.map(str::to_string),
                ..Default::default()
            })))
            .unwrap();
        state.resources.insert("management".to_string(), resource);
        state
    }

    fn not_found() -> AlienError<alien_client_core::ErrorData> {
        AlienError::new(alien_client_core::ErrorData::RemoteResourceNotFound {
            resource_type: "repository".to_string(),
            resource_name: "acme-agents-img".to_string(),
        })
    }

    fn labelled(owner: Option<&str>) -> Repository {
        Repository {
            labels: owner
                .map(|owner| HashMap::from([(OWNER_LABEL.to_string(), owner.to_string())])),
            ..Default::default()
        }
    }

    fn provider(registry: MockArtifactRegistryApi, iam: MockIamApi) -> MockPlatformServiceProvider {
        let (registry, iam) = (Arc::new(registry), Arc::new(iam));
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_artifact_registry_client()
            .returning(move |_| Ok(registry.clone()));
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        provider
    }

    async fn step(
        provider: &MockPlatformServiceProvider,
        state: &StackState,
        records: &mut BTreeMap<String, SetupScaffolding>,
    ) -> Result<ScaffoldingProgress> {
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: provider,
            resource_prefix: PREFIX,
        };
        reconcile(&ctx, state, "agents", records).await
    }

    fn operation() -> Operation {
        serde_json::from_value(serde_json::json!({"name": "operations/op", "done": true})).unwrap()
    }

    fn existing_role() -> Role {
        Role::builder()
            .title("Copy a sandbox image into its repository".to_string())
            .description(
                "Read, upload and tag in a sandbox image repository. Cannot delete.".to_string(),
            )
            .included_permissions(
                SANDBOX_IMAGE_WRITE_PERMISSIONS
                    .iter()
                    .map(|permission| permission.to_string())
                    .collect(),
            )
            .stage(alien_gcp_clients::iam::RoleLaunchStage::Ga)
            .build()
    }

    /// A crash after Google made the repository but before the step result was saved leaves no
    /// record; teardown finds the repository by its name and setup's label instead.
    #[tokio::test]
    async fn an_unrecorded_repository_with_setups_label_is_recovered_and_one_without_is_not() {
        for (label, recovered) in [(Some(PREFIX), true), (Some("other"), false), (None, false)] {
            let mut registry = MockArtifactRegistryApi::new();
            registry
                .expect_get_repository()
                .returning(move |_, _, _| Ok(labelled(label)));
            let provider = provider(registry, MockIamApi::new());
            let client_config = client_config();
            let ctx = SetupScaffoldingContext {
                client_config: &client_config,
                service_provider: &provider,
                resource_prefix: PREFIX,
            };
            let stack = stack_with_image("registry.example/agents:1");
            let mut records = BTreeMap::new();
            crate::setup_scaffolding::recover_unrecorded(&ctx, &stack, Platform::Gcp, &mut records)
                .await
                .unwrap();
            assert_eq!(records.contains_key("agents"), recovered, "label {label:?}");
        }
    }

    /// Nothing is created until the management account exists: a grant naming it would be refused.
    #[tokio::test]
    async fn waits_for_the_management_account_and_creates_nothing() {
        let provider = provider(MockArtifactRegistryApi::new(), MockIamApi::new());
        let mut records = BTreeMap::new();
        let progress = step(&provider, &state(None), &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::InProgress);
        assert!(records.is_empty());
    }

    /// The record is written in the same step as the create call, so a crash after it still tears
    /// the repository down.
    #[tokio::test]
    async fn a_missing_repository_is_recorded_then_created_with_setups_label() {
        let mut registry = MockArtifactRegistryApi::new();
        registry
            .expect_get_repository()
            .times(1)
            .returning(|_, _, _| Err(not_found()));
        registry
            .expect_create_repository()
            .times(1)
            .withf(|project, location, id, body| {
                project == "test-project-123"
                    && location == "us-central1"
                    && id == "acme-agents-img"
                    && body.format == Some(RepositoryFormat::Docker)
                    && matches!(
                        body.repository_config,
                        Some(
                            alien_gcp_clients::artifactregistry::RepositoryConfig::DockerConfig(
                                alien_gcp_clients::artifactregistry::DockerRepositoryConfig {
                                    immutable_tags: Some(true)
                                }
                            )
                        )
                    )
                    && body.labels.as_ref().is_some_and(|labels| {
                        labels.get(OWNER_LABEL).map(String::as_str) == Some(PREFIX)
                    })
                    && body
                        .cleanup_policies
                        .as_ref()
                        .is_some_and(|p| p.contains_key("delete-untagged"))
            })
            .returning(|_, _, _, _| Ok(operation()));
        let provider = provider(registry, MockIamApi::new());
        let mut records = BTreeMap::new();
        let progress = step(
            &provider,
            &state(Some(
                "acme-management@test-project-123.iam.gserviceaccount.com",
            )),
            &mut records,
        )
        .await
        .unwrap();
        assert_eq!(progress, ScaffoldingProgress::InProgress);
        assert_eq!(
            records["agents"],
            SetupScaffolding::GcpSandbox {
                image_repository: "acme-agents-img".to_string(),
                write_role: "acme_agents_image_write".to_string(),
                writer_granted: false,
            }
        );
    }

    /// The grant is the custom role (no delete) for the management account, on the repository
    /// alone; once it is in place the record says so and setup is done.
    #[tokio::test]
    async fn the_manager_is_granted_the_no_delete_role_on_the_repository_alone() {
        let mut registry = MockArtifactRegistryApi::new();
        registry
            .expect_get_repository()
            .returning(|_, _, _| Ok(labelled(Some(PREFIX))));
        registry
            .expect_get_repository_iam_policy()
            .times(1)
            .returning(|_, _, _| Ok(IamPolicy::builder().etag("etag-1".to_string()).build()));
        registry
            .expect_set_repository_iam_policy()
            .times(1)
            .withf(|_, _, repository, policy| {
                repository == "acme-agents-img"
                    && policy.etag.as_deref() == Some("etag-1")
                    && policy.bindings.len() == 1
                    && policy.bindings[0].role
                        == "projects/test-project-123/roles/acme_agents_image_write"
                    && policy.bindings[0].members == [MANAGER]
            })
            .returning(|_, _, _, policy| Ok(policy));
        let mut iam = MockIamApi::new();
        iam.expect_get_role().returning(|_| Ok(existing_role()));
        let provider = provider(registry, iam);
        let mut records = BTreeMap::new();
        let state = state(Some(
            "acme-management@test-project-123.iam.gserviceaccount.com",
        ));
        let progress = step(&provider, &state, &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::InProgress);
    }

    #[tokio::test]
    async fn an_existing_grant_finishes_and_is_recorded() {
        let mut registry = MockArtifactRegistryApi::new();
        registry
            .expect_get_repository()
            .returning(|_, _, _| Ok(labelled(Some(PREFIX))));
        registry
            .expect_get_repository_iam_policy()
            .returning(|_, _, _| {
                Ok(IamPolicy::builder()
                    .bindings(vec![Binding {
                        role: "projects/test-project-123/roles/acme_agents_image_write".to_string(),
                        members: vec![MANAGER.to_string()],
                        condition: None,
                    }])
                    .build())
            });
        registry.expect_set_repository_iam_policy().times(0);
        let mut iam = MockIamApi::new();
        iam.expect_get_role().returning(|_| Ok(existing_role()));
        let provider = provider(registry, iam);
        let mut records = BTreeMap::new();
        let state = state(Some(
            "acme-management@test-project-123.iam.gserviceaccount.com",
        ));
        let progress = step(&provider, &state, &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::Done);
        assert!(matches!(
            records["agents"],
            SetupScaffolding::GcpSandbox {
                writer_granted: true,
                ..
            }
        ));
    }

    /// A repository of that name that setup did not label is someone else's: refused, never
    /// adopted, so teardown never deletes it.
    #[tokio::test]
    async fn a_repository_without_setups_label_is_refused() {
        let mut registry = MockArtifactRegistryApi::new();
        registry
            .expect_get_repository()
            .returning(|_, _, _| Ok(labelled(None)));
        registry.expect_set_repository_iam_policy().times(0);
        let provider = provider(registry, MockIamApi::new());
        let mut records = BTreeMap::new();
        let state = state(Some(
            "acme-management@test-project-123.iam.gserviceaccount.com",
        ));
        let error = step(&provider, &state, &mut records).await.unwrap_err();
        assert_eq!(error.code, "SETUP_SCAFFOLDING_NOT_ADOPTABLE");
        assert!(records.is_empty());
    }

    /// Teardown deletes the repository, then the role, and drops the record only once both are gone.
    #[tokio::test]
    async fn teardown_deletes_the_repository_then_the_role() {
        let mut registry = MockArtifactRegistryApi::new();
        let mut seen = 0;
        registry.expect_get_repository().returning(move |_, _, _| {
            seen += 1;
            if seen == 1 {
                Ok(labelled(Some(PREFIX)))
            } else {
                Err(not_found())
            }
        });
        registry
            .expect_delete_repository()
            .times(1)
            .withf(|_, _, id| id == "acme-agents-img")
            .returning(|_, _, _| Ok(operation()));
        let mut iam = MockIamApi::new();
        iam.expect_delete_role()
            .times(1)
            .withf(|name| name == "acme_agents_image_write")
            .returning(|_| Ok(existing_role()));
        let provider = provider(registry, iam);
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: &provider,
            resource_prefix: PREFIX,
        };
        let mut records = BTreeMap::from([(
            "agents".to_string(),
            SetupScaffolding::GcpSandbox {
                image_repository: "acme-agents-img".to_string(),
                write_role: "acme_agents_image_write".to_string(),
                writer_granted: true,
            },
        )]);
        let first = super::super::teardown(&ctx, &mut records).await.unwrap();
        assert_eq!(first, ScaffoldingProgress::InProgress);
        assert!(records.contains_key("agents"));
        let second = super::super::teardown(&ctx, &mut records).await.unwrap();
        assert_eq!(second, ScaffoldingProgress::Done);
        assert!(records.is_empty());
    }

    /// A repository of the recorded name that no longer carries setup's label was replaced by
    /// someone else; teardown leaves it and still removes the role.
    #[tokio::test]
    async fn teardown_leaves_a_repository_without_setups_label() {
        let mut registry = MockArtifactRegistryApi::new();
        registry
            .expect_get_repository()
            .returning(|_, _, _| Ok(labelled(None)));
        registry.expect_delete_repository().never();
        let mut iam = MockIamApi::new();
        iam.expect_delete_role()
            .times(1)
            .returning(|_| Ok(existing_role()));
        let provider = provider(registry, iam);
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: &provider,
            resource_prefix: PREFIX,
        };
        let mut records = BTreeMap::from([(
            "agents".to_string(),
            SetupScaffolding::GcpSandbox {
                image_repository: "acme-agents-img".to_string(),
                write_role: "acme_agents_image_write".to_string(),
                writer_granted: true,
            },
        )]);
        let done = super::super::teardown(&ctx, &mut records).await.unwrap();
        assert_eq!(done, ScaffoldingProgress::Done);
    }

    fn stack_with_image(image: &str) -> alien_core::Stack {
        alien_core::Stack::new("acme".to_string())
            .add(
                alien_core::Sandbox::new("agents".to_string())
                    .code(alien_core::SandboxCode::Image {
                        image: image.to_string(),
                        bundled_from: None,
                    })
                    .egress(alien_core::SandboxEgress::None)
                    .lifecycle(alien_core::SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build()
    }

    /// A Live sandbox copies its image into a repository the runtime identity cannot create, so
    /// setup scaffolds it under either lifecycle.
    #[test]
    fn a_live_sandbox_is_scaffolded_too() {
        for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
            let stack = alien_core::Stack::new("acme".to_string())
                .add(
                    alien_core::Sandbox::new("agents".to_string())
                        .code(alien_core::SandboxCode::Image {
                            image: "ubuntu".to_string(),
                            bundled_from: None,
                        })
                        .egress(alien_core::SandboxEgress::None)
                        .lifecycle(alien_core::SandboxLifecyclePolicy {
                            max_lifetime_seconds: None,
                            idle_pause_seconds: None,
                        })
                        .build(),
                    lifecycle,
                )
                .build();
            assert!(super::super::scaffolds_any(&stack, Platform::Gcp), "{lifecycle:?}");
        }
    }

    fn record(writer: bool) -> BTreeMap<String, SetupScaffolding> {
        BTreeMap::from([(
            "agents".to_string(),
            SetupScaffolding::GcpSandbox {
                image_repository: "acme-agents-img".to_string(),
                write_role: "acme_agents_image_write".to_string(),
                writer_granted: writer,
            },
        )])
    }

    /// The sandbox's state as a setup run leaves it: with or without the template grant recorded.
    fn state_with_grant(granted: bool) -> StackState {
        let mut state = StackState::new(Platform::Gcp);
        let mut resource = StackResourceState::builder()
            .resource_type(alien_core::Sandbox::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(alien_core::Resource::new(
                stack_with_image("x").resources["agents"]
                    .config
                    .downcast_ref::<alien_core::Sandbox>()
                    .unwrap()
                    .clone(),
            ))
            .lifecycle(ResourceLifecycle::Frozen)
            .build();
        resource.internal_state = Some(if granted {
            serde_json::json!({"type": "GcpAgentPlatformTemplateController", "engineSetup": {
                "engineGrants": [[format!("projects/p/roles/role_{}_sandbox_templates_x", PREFIX), MANAGER]]
            }})
        } else {
            serde_json::json!({"type": "GcpAgentPlatformTemplateController"})
        });
        state.resources.insert("agents".to_string(), resource);
        state.resource_prefix = PREFIX.to_string();
        state
    }

    fn image_changes(records: &BTreeMap<String, SetupScaffolding>, granted: bool) -> Vec<String> {
        super::super::changes_requiring_setup(
            &client_config(),
            &stack_with_image("registry.example/a@sha256:1"),
            records,
            &state_with_grant(granted),
            &stack_with_image("registry.example/a@sha256:2"),
            Platform::Gcp,
        )
        .unwrap()
    }

    /// An image-only release rolls without setup only when setup recorded both the repository
    /// write access and the template grant. Any other install gets the one message.
    #[test]
    fn an_image_only_release_rolls_only_with_both_grants_recorded() {
        assert!(image_changes(&record(true), true).is_empty());
        for (records, granted) in [
            (record(true), false),
            (record(false), true),
            (BTreeMap::new(), true),
        ] {
            let changes = image_changes(&records, granted);
            assert_eq!(changes.len(), 1, "{records:?}");
            assert!(changes[0].contains("Running the deployment's setup once enables"));
        }
        // No change, no message, whatever was recorded.
        let same = super::super::changes_requiring_setup(
            &client_config(),
            &stack_with_image("registry.example/a@sha256:1"),
            &BTreeMap::new(),
            &state_with_grant(false),
            &stack_with_image("registry.example/a@sha256:1"),
            Platform::Gcp,
        )
        .unwrap();
        assert!(same.is_empty());
    }
}
