//! Target account setup for cross-account E2E tests.
//!
//! Creates a scoped IAM role with auto-generated permissions (from
//! `alien-permissions`) and impersonates it during `push_initial_setup`.
//! This validates that the auto-generated permissions are sufficient for
//! initial setup — the real flow a customer admin would follow.

use std::collections::HashSet;
use std::sync::Arc;

use alien_aws_clients::{ErrorData as AwsErrorData, IamApi};
use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    ManagementConfig, PermissionSet, PermissionSetReference, Platform, ResourceLifecycle, Stack,
    StackSettings, StackState,
};
use alien_permissions::generators::{
    AwsIamPolicy, AwsIamStatement, AwsRuntimePermissionsGenerator,
    AzureRuntimePermissionsGenerator, GcpRuntimePermissionsGenerator,
};
use alien_permissions::{BindingTarget, PermissionContext};
use anyhow::Context;
use tracing::{info, warn};

use crate::config::TestConfig;
use crate::deployment::TestDeployment;
use crate::manager::TestManager;

/// Run the target-side setup for cross-account deployment.
///
/// 1. Auto-generates minimal permissions from the stack definition
/// 2. Creates a temporary scoped IAM role with those permissions
/// 3. Impersonates the role during `push_initial_setup`
/// 4. If initial setup succeeds → permissions are correct
///
/// After this function returns, the manager's deployment loop will resume
/// from `Provisioning` using its own management SA impersonation chain.
pub async fn setup_target(
    config: &TestConfig,
    platform: Platform,
    deployment: &TestDeployment,
    stack: &Stack,
    manager: &Arc<TestManager>,
    management_config: Option<ManagementConfig>,
) -> anyhow::Result<()> {
    if !config.has_platform(platform) {
        anyhow::bail!(
            "Cannot set up target for {}: missing management or target credentials",
            platform.as_str()
        );
    }

    info!(
        platform = %platform.as_str(),
        deployment_id = %deployment.id,
        "setup_target: generating scoped permissions and running initial setup"
    );

    let target_config = build_scoped_target_config(config, platform, &deployment.name, Some(stack))
        .await
        .context("Failed to build scoped target config")?;

    alien_deploy_cli::commands::push_initial_setup(
        manager.client(),
        &deployment.id,
        platform,
        target_config.clone(),
        management_config,
        &manager.public_url,
        &deployment.token,
        None, // no network override from tests
        None,
    )
    .await
    .map_err(|e| anyhow::anyhow!("push_initial_setup failed: {}", e))?;

    // For Azure with shared (external) Container Apps Environment: the management
    // UAMI now exists but lacks `managedEnvironments/join/action` on the shared
    // environment (which is in a different resource group). Grant it here using
    // target credentials, before the manager's Provisioning phase starts.
    if platform == Platform::Azure {
        if let Some(ref shared_env) = config.azure_resources.shared_container_env {
            grant_shared_env_join_permission(
                config,
                &target_config,
                deployment,
                manager,
                shared_env,
            )
            .await
            .context("Failed to grant join permission on shared Container Apps Environment")?;
        }
    }

    info!(
        deployment_id = %deployment.id,
        "setup_target complete — manager will continue from Provisioning"
    );

    Ok(())
}

/// Build credentials for an agent that runs the full deployment loop in the
/// target environment.
///
/// The returned identity can run setup for Frozen resources and manage Live
/// resources. It must not be the broad admin/bootstrap identity.
pub async fn build_agent_client_config(
    config: &TestConfig,
    platform: Platform,
    stack: &Stack,
    deployment_name: &str,
) -> anyhow::Result<ClientConfig> {
    match platform {
        Platform::Aws => {
            let context = aws_permission_context(config)?;
            let setup_policy =
                alien_permissions::initial_setup::generate_aws_initial_setup_policy(&context)
                    .map_err(|e| anyhow::anyhow!("Failed to generate setup policy: {}", e))?;
            let management_policy = generate_aws_management_policy(config, platform, stack)
                .await
                .context("Failed to generate agent management policy")?;

            build_aws_scoped_config_with_policies(
                config,
                deployment_name,
                vec![
                    ("alien-setup".to_string(), setup_policy),
                    ("alien-management".to_string(), management_policy),
                ],
                "agent",
                "Alien E2E scoped agent role (auto-generated permissions)",
            )
            .await
        }
        Platform::Gcp => {
            let mut permission_sets = setup_permission_sets();
            permission_sets.extend(
                management_permission_sets(config, platform, stack)
                    .await
                    .context("Failed to generate agent management permission sets")?,
            );
            build_gcp_scoped_config(config, deployment_name, permission_sets, "agent").await
        }
        Platform::Azure => {
            let mut permission_sets = setup_permission_sets();
            permission_sets.extend(
                management_permission_sets(config, platform, stack)
                    .await
                    .context("Failed to generate agent management permission sets")?,
            );
            build_azure_scoped_config(config, deployment_name, permission_sets, "agent").await
        }
        other => anyhow::bail!("agent target config not supported for platform: {}", other),
    }
}

/// Build a `ClientConfig` that impersonates a scoped role with auto-generated
/// permissions.
async fn build_scoped_target_config(
    config: &TestConfig,
    platform: Platform,
    deployment_name: &str,
    stack: Option<&Stack>,
) -> anyhow::Result<ClientConfig> {
    match platform {
        Platform::Aws => build_aws_scoped_config(config, deployment_name).await,
        Platform::Gcp => {
            let permission_sets = setup_permission_sets_for_stack(stack);
            build_gcp_scoped_config(config, deployment_name, permission_sets, "initial-setup").await
        }
        Platform::Azure => {
            let permission_sets = setup_permission_sets_for_stack(stack);
            build_azure_scoped_config(config, deployment_name, permission_sets, "initial-setup")
                .await
        }
        other => anyhow::bail!("setup_target not supported for platform: {}", other),
    }
}

/// AWS: Create a temporary IAM role with auto-generated permissions and
/// return a ClientConfig that assumes it.
async fn build_aws_scoped_config(
    config: &TestConfig,
    deployment_name: &str,
) -> anyhow::Result<ClientConfig> {
    let context = aws_permission_context(config)?;

    let policy = alien_permissions::initial_setup::generate_aws_initial_setup_policy(&context)
        .map_err(|e| anyhow::anyhow!("Failed to generate initial setup policy: {}", e))?;

    build_aws_scoped_config_with_policies(
        config,
        deployment_name,
        vec![("alien-initial-setup".to_string(), policy)],
        "initial-setup",
        "Alien E2E scoped initial setup role (auto-generated permissions)",
    )
    .await
}

async fn build_aws_scoped_config_with_policies(
    config: &TestConfig,
    deployment_name: &str,
    policies: Vec<(String, AwsIamPolicy)>,
    session_purpose: &str,
    role_description: &str,
) -> anyhow::Result<ClientConfig> {
    use alien_aws_clients::{AwsClientConfigExt as _, AwsCredentialProvider, IamApi};
    use alien_core::{AwsClientConfig, AwsCredentials};

    let target = config
        .aws_target
        .as_ref()
        .context("Missing AWS target credentials")?;

    let account_id = target.account_id.clone().unwrap_or_default();

    // Create the target credentials (admin user)
    let admin_config = AwsClientConfig {
        account_id: account_id.clone(),
        region: target.region.clone(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: target.access_key_id.clone(),
            secret_access_key: target.secret_access_key.clone(),
            session_token: target.session_token.clone(),
        },
        service_overrides: None,
    };

    let admin_creds = AwsCredentialProvider::from_config(admin_config.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create admin credential provider: {}", e))?;

    let iam_client =
        alien_aws_clients::iam::IamClient::new(reqwest::Client::new(), admin_creds.clone());

    // Create the scoped role
    // Use first 8 chars of deployment name for role naming (not for permissions)
    let role_suffix = &deployment_name[..deployment_name.len().min(8)];
    let role_name = format!("alien-e2e-{}-{}", session_purpose, role_suffix);

    // Trust policy: allow the target user to assume this role
    let trust_policy = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": {
                "AWS": format!("arn:aws:iam::{}:root", account_id)
            },
            "Action": "sts:AssumeRole"
        }]
    });

    // Create or update the role
    let create_result = iam_client
        .create_role(alien_aws_clients::iam::CreateRoleRequest {
            role_name: role_name.clone(),
            assume_role_policy_document: trust_policy.to_string(),
            description: Some(role_description.to_string()),
            path: None,
            max_session_duration: None,
            tags: None,
        })
        .await;

    match create_result {
        Ok(response) => {
            info!(
                role_name = %role_name,
                role_arn = %response.create_role_result.role.arn,
                "Created scoped IAM role"
            );
        }
        Err(e) => {
            let err_str = format!("{}", e);
            if err_str.contains("already exists")
                || err_str.contains("EntityAlreadyExists")
                || err_str.contains("Conflict")
            {
                info!(role_name = %role_name, "Scoped IAM role already exists, updating policy");
            } else {
                return Err(anyhow::anyhow!("Failed to create scoped role: {}", e));
            }
        }
    }

    cleanup_scoped_role_policies(&iam_client, &role_name).await?;

    for (policy_name, policy) in policies {
        attach_policy_chunks(&iam_client, &role_name, &policy_name, policy).await?;
    }

    info!(role_name = %role_name, "Attached auto-generated policy to scoped role");

    // Wait for IAM propagation
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    // Assume the scoped role
    let role_arn = format!("arn:aws:iam::{}:role/{}", account_id, role_name);
    let scoped_config = admin_config
        .impersonate(alien_aws_clients::AwsImpersonationConfig {
            role_arn: role_arn.clone(),
            session_name: Some(format!("alien-e2e-{}", session_purpose)),
            duration_seconds: Some(3600),
            external_id: None,
            target_region: None,
        })
        .await
        .map_err(|e| anyhow::anyhow!("Failed to assume scoped role {}: {}", role_arn, e))?;

    info!(role_arn = %role_arn, "Assumed scoped initial setup role");

    Ok(ClientConfig::Aws(Box::new(scoped_config)))
}

fn aws_permission_context(config: &TestConfig) -> anyhow::Result<PermissionContext> {
    let target = config
        .aws_target
        .as_ref()
        .context("Missing AWS target credentials")?;
    let mgmt = config
        .aws_mgmt
        .as_ref()
        .context("Missing AWS management credentials")?;

    let account_id = target.account_id.clone().unwrap_or_default();
    let mgmt_account_id = mgmt
        .account_id
        .as_ref()
        .context("Missing management account ID")?;

    Ok(PermissionContext::new()
        .with_aws_account_id(account_id)
        .with_aws_region(&target.region)
        .with_stack_prefix("*")
        .with_managing_account_id(mgmt_account_id))
}

async fn generate_aws_management_policy(
    config: &TestConfig,
    platform: Platform,
    stack: &Stack,
) -> anyhow::Result<AwsIamPolicy> {
    let context = aws_permission_context(config)?;
    let stack_state = StackState::with_resource_prefix(platform, "e2eagent".to_string());
    let deployment_config = DeploymentConfig {
        stack_settings: StackSettings::default(),
        management_config: None,
        environment_variables: EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        },
        allow_frozen_changes: false,
        compute_backend: None,
        external_bindings: ExternalBindings::default(),
        public_urls: None,
        domain_metadata: None,
        monitoring: None,
        manager_url: None,
        deployment_token: None,
        native_image_host: None,
    };

    let mutated_stack = alien_preflights::runner::PreflightRunner::default()
        .apply_mutations(stack.clone(), &stack_state, &deployment_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to apply stack mutations: {}", e))?;

    let Some(profile) = mutated_stack.management().profile() else {
        return Ok(AwsIamPolicy {
            version: "2012-10-17".to_string(),
            statement: Vec::new(),
        });
    };

    let generator = AwsRuntimePermissionsGenerator::new();
    let mut statements = Vec::new();
    let mut used_sids = HashSet::new();

    for permission_refs in profile.0.values() {
        for permission_ref in permission_refs {
            let permission_set = match permission_ref {
                PermissionSetReference::Name(name) => {
                    let Some(permission_set) = alien_permissions::get_permission_set(name) else {
                        continue;
                    };
                    permission_set.clone()
                }
                PermissionSetReference::Inline(permission_set) => permission_set.clone(),
            };

            if permission_set.platforms.aws.is_none() {
                continue;
            }

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Stack, &context)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to generate AWS policy for {}: {}",
                        permission_set.id,
                        e
                    )
                })?;

            for mut statement in policy.statement {
                if !used_sids.insert(statement.sid.clone()) {
                    let base = statement.sid.clone();
                    let mut index = 2;
                    loop {
                        let candidate = format!("{base}{index}");
                        if used_sids.insert(candidate.clone()) {
                            statement.sid = candidate;
                            break;
                        }
                        index += 1;
                    }
                }
                statements.push(statement);
            }
        }
    }

    Ok(AwsIamPolicy {
        version: "2012-10-17".to_string(),
        statement: statements,
    })
}

async fn attach_policy_chunks(
    iam_client: &alien_aws_clients::iam::IamClient,
    role_name: &str,
    policy_name: &str,
    policy: AwsIamPolicy,
) -> anyhow::Result<()> {
    let chunks = split_policy(policy, 5_500)?;
    let policy_run_id = uuid::Uuid::new_v4().simple().to_string();
    let policy_run_id = &policy_run_id[..8];

    for (index, chunk) in chunks.into_iter().enumerate() {
        let chunk_name = scoped_managed_policy_name(role_name, policy_name, policy_run_id, index);
        let policy_json =
            serde_json::to_string(&chunk).context("Failed to serialize IAM policy")?;

        info!(
            role_name = %role_name,
            policy_name = %chunk_name,
            statements = chunk.statement.len(),
            policy_size = policy_json.len(),
            "Creating scoped IAM managed policy"
        );

        let create_response = iam_client
            .create_policy(&chunk_name, &policy_json, Some("/alien-e2e/".to_string()))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create scoped managed policy: {}", e))?;
        let policy_arn = create_response.create_policy_result.policy.arn;

        if let Err(error) = iam_client.attach_role_policy(role_name, &policy_arn).await {
            let _ = iam_client.delete_policy(&policy_arn).await;
            return Err(anyhow::anyhow!(
                "Failed to attach managed policy to scoped role: {}",
                error
            ));
        }

        info!(
            role_name = %role_name,
            policy_arn = %policy_arn,
            "Attached scoped IAM managed policy"
        );
    }

    Ok(())
}

async fn cleanup_scoped_role_policies(
    iam_client: &alien_aws_clients::iam::IamClient,
    role_name: &str,
) -> anyhow::Result<()> {
    match iam_client.list_role_policies(role_name).await {
        Ok(response) => {
            if let Some(policy_names) = response.list_role_policies_result.policy_names {
                for policy_name in policy_names.member {
                    iam_client
                        .delete_role_policy(role_name, &policy_name)
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Failed to delete stale scoped inline policy {policy_name}: {e}"
                            )
                        })?;
                }
            }
        }
        Err(error) if is_aws_not_found(&error) => {}
        Err(error) => {
            return Err(anyhow::anyhow!(
                "Failed to list stale scoped inline policies: {}",
                error
            ));
        }
    }

    match iam_client.list_attached_role_policies(role_name).await {
        Ok(response) => {
            if let Some(attached_policies) = response
                .list_attached_role_policies_result
                .attached_policies
            {
                for policy in attached_policies.member {
                    iam_client
                        .detach_role_policy(role_name, &policy.policy_arn)
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Failed to detach stale scoped managed policy {}: {e}",
                                policy.policy_arn
                            )
                        })?;

                    if policy.policy_name.starts_with("alien-e2e-scoped-") {
                        match iam_client.delete_policy(&policy.policy_arn).await {
                            Ok(()) => {}
                            Err(error) if is_aws_not_found(&error) => {}
                            Err(error) => {
                                return Err(anyhow::anyhow!(
                                    "Failed to delete stale scoped managed policy {}: {}",
                                    policy.policy_arn,
                                    error
                                ));
                            }
                        }
                    }
                }
            }
        }
        Err(error) if is_aws_not_found(&error) => {}
        Err(error) => {
            return Err(anyhow::anyhow!(
                "Failed to list stale scoped managed policies: {}",
                error
            ));
        }
    }

    Ok(())
}

fn scoped_managed_policy_name(
    role_name: &str,
    policy_name: &str,
    run_id: &str,
    index: usize,
) -> String {
    let suffix = if index == 0 {
        run_id.to_string()
    } else {
        format!("{run_id}-{}", index + 1)
    };
    let prefix = format!("alien-e2e-scoped-{role_name}-{policy_name}");
    let max_prefix_len = 128usize.saturating_sub(suffix.len() + 1);
    let trimmed = prefix
        .chars()
        .take(max_prefix_len)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string();
    format!("{trimmed}-{suffix}")
}

fn is_aws_not_found(error: &alien_error::AlienError<AwsErrorData>) -> bool {
    matches!(
        &error.error,
        Some(AwsErrorData::RemoteResourceNotFound { .. })
    )
}

fn split_policy(policy: AwsIamPolicy, max_json_len: usize) -> anyhow::Result<Vec<AwsIamPolicy>> {
    let mut chunks = Vec::new();
    let mut current = Vec::<AwsIamStatement>::new();

    for statement in policy.statement {
        let mut candidate = current.clone();
        candidate.push(statement.clone());
        let candidate_policy = AwsIamPolicy {
            version: policy.version.clone(),
            statement: candidate,
        };
        let candidate_len = serde_json::to_string(&candidate_policy)
            .context("Failed to serialize IAM policy chunk")?
            .len();

        if candidate_len > max_json_len && !current.is_empty() {
            chunks.push(AwsIamPolicy {
                version: policy.version.clone(),
                statement: current,
            });
            current = vec![statement];
        } else if candidate_len > max_json_len {
            anyhow::bail!(
                "A single IAM statement is too large for inline policy {}",
                statement.sid
            );
        } else {
            current = candidate_policy.statement;
        }
    }

    if !current.is_empty() {
        chunks.push(AwsIamPolicy {
            version: policy.version,
            statement: current,
        });
    }

    Ok(chunks)
}

fn setup_permission_sets() -> Vec<PermissionSet> {
    alien_permissions::list_permission_set_ids()
        .into_iter()
        .filter(|id| {
            let Some((resource_type, operation)) = id.split_once('/') else {
                return false;
            };
            operation == "provision"
                && alien_core::ownership_policy_for_resource_type(resource_type)
                    .should_emit_in_setup(ResourceLifecycle::Frozen)
        })
        .filter_map(|id| alien_permissions::get_permission_set(&id).cloned())
        .collect()
}

fn setup_permission_sets_for_stack(stack: Option<&Stack>) -> Vec<PermissionSet> {
    let Some(stack) = stack else {
        return setup_permission_sets();
    };

    let mut permission_sets = setup_permission_sets();
    let mut seen = permission_sets
        .iter()
        .map(|permission_set| permission_set.id.clone())
        .collect::<HashSet<_>>();

    for permission_set_id in
        alien_permissions::initial_setup::initial_setup_permission_set_ids(stack)
    {
        if seen.insert(permission_set_id.clone()) {
            if let Some(permission_set) = alien_permissions::get_permission_set(&permission_set_id)
            {
                permission_sets.push(permission_set.clone());
            }
        }
    }

    permission_sets
}

async fn management_permission_sets(
    config: &TestConfig,
    platform: Platform,
    stack: &Stack,
) -> anyhow::Result<Vec<PermissionSet>> {
    let stack_state = StackState::with_resource_prefix(platform, "e2eagent".to_string());
    let deployment_config = DeploymentConfig {
        stack_settings: StackSettings::default(),
        management_config: None,
        environment_variables: EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        },
        allow_frozen_changes: false,
        compute_backend: None,
        external_bindings: ExternalBindings::default(),
        public_urls: None,
        domain_metadata: None,
        monitoring: None,
        manager_url: None,
        deployment_token: None,
        native_image_host: None,
    };

    let mutated_stack = alien_preflights::runner::PreflightRunner::default()
        .apply_mutations(stack.clone(), &stack_state, &deployment_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to apply stack mutations: {}", e))?;

    let Some(profile) = mutated_stack.management().profile() else {
        return Ok(Vec::new());
    };

    let mut permission_sets = Vec::new();
    let mut seen = HashSet::new();

    for permission_refs in profile.0.values() {
        for permission_ref in permission_refs {
            let permission_set = match permission_ref {
                PermissionSetReference::Name(name) => {
                    let Some(permission_set) = alien_permissions::get_permission_set(name) else {
                        continue;
                    };
                    permission_set.clone()
                }
                PermissionSetReference::Inline(permission_set) => permission_set.clone(),
            };

            if seen.insert(permission_set.id.clone()) {
                permission_sets.push(permission_set);
            }
        }
    }

    // The config argument is intentionally part of this helper's signature so
    // callers cannot accidentally build management permissions without having
    // the target environment available. Keep the read local and cheap.
    let _ = config;

    Ok(permission_sets)
}

async fn build_gcp_scoped_config(
    config: &TestConfig,
    deployment_name: &str,
    permission_sets: Vec<PermissionSet>,
    purpose: &str,
) -> anyhow::Result<ClientConfig> {
    use alien_gcp_clients::iam::{
        CreateRoleRequest, CreateServiceAccountRequest, Role, RoleLaunchStage, ServiceAccount,
    };
    use alien_gcp_clients::resource_manager::GetPolicyOptions;
    use alien_gcp_clients::{IamApi as _, ResourceManagerApi as _};

    let target_config = build_gcp_target_config(config)?;
    let admin_config = target_config
        .gcp_config()
        .context("Expected GCP target config")?
        .clone();

    let http_client = reqwest::Client::new();
    let iam_client =
        alien_gcp_clients::iam::IamClient::new(http_client.clone(), admin_config.clone());
    let resource_manager_client =
        alien_gcp_clients::ResourceManagerClient::new(http_client, admin_config.clone());

    let account_id = gcp_scoped_account_id(purpose, deployment_name);
    let service_account_email = format!(
        "{account_id}@{}.iam.gserviceaccount.com",
        admin_config.project_id
    );
    match iam_client
        .create_service_account(
            account_id.clone(),
            CreateServiceAccountRequest::builder()
                .service_account(
                    ServiceAccount::builder()
                        .display_name(format!("Alien E2E {purpose}"))
                        .description(format!(
                            "Alien E2E scoped {purpose} identity with generated permissions"
                        ))
                        .build(),
                )
                .build(),
        )
        .await
    {
        Ok(_) => info!(%service_account_email, "Created scoped GCP service account"),
        Err(error) => {
            let error = error.to_string();
            if !(error.contains("already exists") || error.contains("ALREADY_EXISTS")) {
                anyhow::bail!("Failed to create scoped GCP service account: {error}");
            }
            info!(%service_account_email, "Scoped GCP service account already exists");
        }
    }

    let mut permission_context = PermissionContext::new()
        .with_project_name(admin_config.project_id.clone())
        .with_region(admin_config.region.clone())
        .with_stack_prefix("*")
        .with_service_account_name(account_id.clone());
    if let Some(project_number) = &admin_config.project_number {
        permission_context = permission_context.with_project_number(project_number.clone());
    }

    let generator = GcpRuntimePermissionsGenerator::new();
    let mut role_names = Vec::new();
    for permission_set in dedupe_permission_sets(permission_sets) {
        if permission_set.platforms.gcp.is_none() {
            continue;
        }

        let custom_role = generator
            .generate_custom_role(&permission_set, &permission_context)
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to generate GCP custom role for {}: {}",
                    permission_set.id,
                    e
                )
            })?;
        let role_id = custom_role
            .name
            .strip_prefix(&format!("projects/{}/roles/", admin_config.project_id))
            .unwrap_or(&custom_role.name)
            .to_string();
        let role = Role::builder()
            .title(custom_role.title.clone())
            .description(custom_role.description.clone())
            .included_permissions(custom_role.included_permissions.clone())
            .stage(RoleLaunchStage::Ga)
            .build();

        match iam_client.get_role(custom_role.name.clone()).await {
            Ok(_) => {
                iam_client
                    .patch_role(
                        custom_role.name.clone(),
                        role,
                        Some("includedPermissions,title,description,stage".to_string()),
                    )
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to update GCP custom role {role_id}: {e}")
                    })?;
            }
            Err(_) => {
                iam_client
                    .create_role(
                        role_id.clone(),
                        CreateRoleRequest::builder().role(role).build(),
                    )
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to create GCP custom role {role_id}: {e}")
                    })?;
            }
        }

        role_names.push(custom_role.name);
    }

    if role_names.is_empty() {
        anyhow::bail!("No GCP permissions were generated for scoped {purpose} identity");
    }

    let member = format!("serviceAccount:{service_account_email}");
    let mut policy = resource_manager_client
        .get_project_iam_policy(
            admin_config.project_id.clone(),
            Some(
                GetPolicyOptions::builder()
                    .requested_policy_version(3)
                    .build(),
            ),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read GCP project IAM policy: {e}"))?;

    policy.version = Some(3);
    for role_name in role_names {
        add_gcp_iam_binding(&mut policy, &role_name, &member, None);
    }

    resource_manager_client
        .set_project_iam_policy(admin_config.project_id.clone(), policy, None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind scoped GCP roles: {e}"))?;

    let bootstrap_service_account_email = gcp_service_account_key_email(&admin_config.credentials)
        .context("Failed to resolve GCP bootstrap service account email")?;
    let bootstrap_member = format!("serviceAccount:{bootstrap_service_account_email}");
    let mut service_account_policy = iam_client
        .get_service_account_iam_policy(service_account_email.clone())
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to read scoped GCP service account IAM policy before granting token minting: {e}"
            )
        })?;
    service_account_policy.version = Some(3);
    add_gcp_iam_binding(
        &mut service_account_policy,
        "roles/iam.serviceAccountTokenCreator",
        &bootstrap_member,
        None,
    );
    iam_client
        .set_service_account_iam_policy(service_account_email.clone(), service_account_policy)
        .await
        .map_err(|e| {
            anyhow::anyhow!("Failed to grant token minting on scoped GCP service account: {e}")
        })?;

    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    let token = generate_gcp_access_token_with_retry(&iam_client, service_account_email.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to mint scoped GCP access token: {e}"))?;

    Ok(ClientConfig::Gcp(Box::new(alien_core::GcpClientConfig {
        project_id: admin_config.project_id,
        region: admin_config.region,
        credentials: alien_core::GcpCredentials::AccessToken {
            token: token.access_token,
        },
        service_overrides: admin_config.service_overrides,
        project_number: admin_config.project_number,
    })))
}

async fn generate_gcp_access_token_with_retry(
    iam_client: &alien_gcp_clients::iam::IamClient,
    service_account_email: String,
) -> anyhow::Result<alien_gcp_clients::iam::GenerateAccessTokenResponse> {
    use alien_gcp_clients::iam::GenerateAccessTokenRequest;
    use alien_gcp_clients::IamApi as _;

    let request = GenerateAccessTokenRequest::builder()
        .scope(vec![
            "https://www.googleapis.com/auth/cloud-platform".to_string()
        ])
        .lifetime("3600s".to_string())
        .build();

    let delays = [
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(10),
        std::time::Duration::from_secs(20),
        std::time::Duration::from_secs(30),
        std::time::Duration::from_secs(45),
    ];

    for (attempt, delay) in delays.iter().enumerate() {
        match iam_client
            .generate_access_token(service_account_email.clone(), request.clone())
            .await
        {
            Ok(token) => return Ok(token),
            Err(error) => {
                warn!(
                    service_account_email = %service_account_email,
                    attempt = attempt + 1,
                    retry_after_secs = delay.as_secs(),
                    error = %error,
                    "Scoped GCP token mint failed; retrying after IAM propagation delay"
                );
                tokio::time::sleep(*delay).await;
            }
        }
    }

    iam_client
        .generate_access_token(service_account_email, request)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn add_gcp_iam_binding(
    policy: &mut alien_gcp_clients::iam::IamPolicy,
    role: &str,
    member: &str,
    condition: Option<alien_gcp_clients::iam::Expr>,
) {
    if let Some(binding) = policy.bindings.iter_mut().find(|binding| {
        binding.role == role
            && binding
                .condition
                .as_ref()
                .map(|existing| existing.expression.as_str())
                == condition
                    .as_ref()
                    .map(|condition| condition.expression.as_str())
    }) {
        if !binding.members.iter().any(|existing| existing == member) {
            binding.members.push(member.to_string());
        }
        return;
    }

    policy.bindings.push(alien_gcp_clients::iam::Binding {
        role: role.to_string(),
        members: vec![member.to_string()],
        condition,
    });
}

fn gcp_service_account_key_email(
    credentials: &alien_core::GcpCredentials,
) -> anyhow::Result<String> {
    let alien_core::GcpCredentials::ServiceAccountKey { json } = credentials else {
        anyhow::bail!("GCP scoped e2e bootstrap requires service account key credentials");
    };
    let value: serde_json::Value =
        serde_json::from_str(json).context("Failed to parse GCP service account key JSON")?;
    value
        .get("client_email")
        .and_then(|value| value.as_str())
        .filter(|email| !email.is_empty())
        .map(ToString::to_string)
        .context("GCP service account key JSON is missing client_email")
}

async fn build_azure_scoped_config(
    config: &TestConfig,
    deployment_name: &str,
    permission_sets: Vec<PermissionSet>,
    purpose: &str,
) -> anyhow::Result<ClientConfig> {
    use alien_azure_clients::authorization::AuthorizationApi as _;
    use alien_azure_clients::models::authorization_role_assignments::{
        RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
    };
    use alien_azure_clients::models::authorization_role_definitions::{
        Permission, RoleDefinition, RoleDefinitionProperties,
    };

    let target = config
        .azure_target
        .as_ref()
        .context("Missing Azure target credentials")?;
    let agent = &config.azure_resources;
    let agent_client_id = agent
        .agent_client_id
        .as_ref()
        .context("AZURE_AGENT_CLIENT_ID is required for scoped Azure agent e2e credentials")?;
    let agent_client_secret = agent
        .agent_client_secret
        .as_ref()
        .context("AZURE_AGENT_CLIENT_SECRET is required for scoped Azure agent e2e credentials")?;
    let agent_object_id = agent
        .agent_object_id
        .as_ref()
        .context("AZURE_AGENT_OBJECT_ID is required for scoped Azure agent e2e credentials")?;
    ensure_distinct_azure_agent_identity(agent_client_id, &target.client_id)?;

    let admin_config = build_azure_target_config(config)?
        .azure_config()
        .context("Expected Azure target config")?
        .clone();
    let token_cache = alien_azure_clients::AzureTokenCache::new(admin_config.clone());
    let auth_client =
        alien_azure_clients::AzureAuthorizationClient::new(reqwest::Client::new(), token_cache);

    let generator = AzureRuntimePermissionsGenerator::new();
    let mut actions = HashSet::new();
    let mut data_actions = HashSet::new();
    let permission_context = PermissionContext::new()
        .with_subscription_id(admin_config.subscription_id.clone())
        .with_resource_group("*")
        .with_storage_account_name("*")
        .with_stack_prefix("*")
        .with_principal_id(agent_object_id.clone())
        .with_managing_subscription_id(admin_config.subscription_id.clone())
        .with_managing_resource_group("*");

    for permission_set in dedupe_permission_sets(permission_sets) {
        if permission_set.platforms.azure.is_none() {
            continue;
        }

        let role = generator
            .generate_role_definition(&permission_set, BindingTarget::Stack, &permission_context)
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to generate Azure role definition for {}: {}",
                    permission_set.id,
                    e
                )
            })?;
        actions.extend(role.actions);
        data_actions.extend(role.data_actions);
    }

    if actions.is_empty() && data_actions.is_empty() {
        anyhow::bail!("No Azure permissions were generated for scoped {purpose} identity");
    }

    let mut actions = actions.into_iter().collect::<Vec<_>>();
    let mut data_actions = data_actions.into_iter().collect::<Vec<_>>();
    actions.sort();
    data_actions.sort();

    let role_definition_guid = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("alien:e2e:azure:{deployment_name}:{purpose}:role").as_bytes(),
    )
    .to_string();
    let subscription_scope = format!("subscriptions/{}", admin_config.subscription_id);
    let role_definition = RoleDefinition {
        id: None,
        name: Some(role_definition_guid.clone()),
        type_: None,
        properties: Some(RoleDefinitionProperties {
            role_name: Some(format!(
                "Alien E2E {} {}",
                purpose,
                &deployment_name[..deployment_name.len().min(8)]
            )),
            description: Some(format!(
                "Alien E2E scoped {purpose} role with generated permissions"
            )),
            type_: Some("CustomRole".to_string()),
            permissions: vec![Permission {
                actions,
                data_actions,
                not_actions: Vec::new(),
                not_data_actions: Vec::new(),
            }],
            assignable_scopes: vec![format!("/{subscription_scope}")],
            created_by: None,
            created_on: None,
            updated_by: None,
            updated_on: None,
        }),
    };

    let created_role = auth_client
        .create_or_update_role_definition(
            &alien_azure_clients::authorization::Scope::Subscription,
            role_definition_guid.clone(),
            &role_definition,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create scoped Azure role definition: {e}"))?;

    let role_definition_id = created_role.id.unwrap_or_else(|| {
        format!(
            "/{subscription_scope}/providers/Microsoft.Authorization/roleDefinitions/{role_definition_guid}"
        )
    });

    let role_assignment_guid = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("alien:e2e:azure:{deployment_name}:{purpose}:assignment:{agent_object_id}")
            .as_bytes(),
    )
    .to_string();
    let role_assignment_id = format!(
        "/{subscription_scope}/providers/Microsoft.Authorization/roleAssignments/{role_assignment_guid}"
    );

    auth_client
        .create_or_update_role_assignment_by_id(
            role_assignment_id,
            &RoleAssignment {
                id: None,
                name: Some(role_assignment_guid),
                type_: None,
                properties: Some(RoleAssignmentProperties {
                    principal_id: agent_object_id.clone(),
                    role_definition_id,
                    scope: Some(subscription_scope),
                    principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                    condition: None,
                    condition_version: None,
                    delegated_managed_identity_resource_id: None,
                    description: Some(format!("Alien E2E scoped {purpose} assignment")),
                    created_by: None,
                    created_on: None,
                    updated_by: None,
                    updated_on: None,
                }),
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to assign scoped Azure role: {e}"))?;

    tokio::time::sleep(std::time::Duration::from_secs(20)).await;

    Ok(ClientConfig::Azure(Box::new(
        alien_core::AzureClientConfig {
            subscription_id: target.subscription_id.clone(),
            tenant_id: target.tenant_id.clone(),
            region: Some(target.region.clone()),
            credentials: alien_core::AzureCredentials::ServicePrincipal {
                client_id: agent_client_id.clone(),
                client_secret: agent_client_secret.clone(),
            },
            service_overrides: None,
        },
    )))
}

fn ensure_distinct_azure_agent_identity(
    agent_client_id: &str,
    target_client_id: &str,
) -> anyhow::Result<()> {
    if agent_client_id.eq_ignore_ascii_case(target_client_id) {
        anyhow::bail!(
            "AZURE_AGENT_CLIENT_ID must identify a pre-provisioned scoped Azure e2e deployment service principal, not AZURE_TARGET_CLIENT_ID"
        );
    }

    Ok(())
}

fn dedupe_permission_sets(permission_sets: Vec<PermissionSet>) -> Vec<PermissionSet> {
    let mut seen = HashSet::new();
    permission_sets
        .into_iter()
        .filter(|permission_set| seen.insert(permission_set.id.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::ensure_distinct_azure_agent_identity;

    #[test]
    fn rejects_target_credentials_as_azure_agent_identity() {
        let err = ensure_distinct_azure_agent_identity(
            "11111111-2222-3333-4444-555555555555",
            "11111111-2222-3333-4444-555555555555",
        )
        .expect_err("matching client IDs must be rejected");

        assert!(
            err.to_string().contains("not AZURE_TARGET_CLIENT_ID"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn accepts_distinct_azure_agent_identity() {
        ensure_distinct_azure_agent_identity(
            "11111111-2222-3333-4444-555555555555",
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        )
        .expect("distinct client IDs should be accepted");
    }
}

fn gcp_scoped_account_id(purpose: &str, deployment_name: &str) -> String {
    let raw =
        format!("a-e2e-{}-{}", purpose.replace('_', "-"), deployment_name).to_ascii_lowercase();
    let sanitized = raw
        .chars()
        .filter(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || *ch == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    sanitized[..sanitized.len().min(30)]
        .trim_end_matches('-')
        .to_string()
}

/// GCP: Use target SA credentials directly for bootstrap-only operations such
/// as creating scoped e2e identities. Do not pass this config to the agent.
fn build_gcp_target_config(config: &TestConfig) -> anyhow::Result<ClientConfig> {
    let target = config
        .gcp_target
        .as_ref()
        .context("Missing GCP target credentials")?;
    let credentials = if let Some(ref json) = target.credentials_json {
        alien_core::GcpCredentials::ServiceAccountKey { json: json.clone() }
    } else {
        anyhow::bail!("GCP target credentials must include service account key JSON");
    };
    Ok(ClientConfig::Gcp(Box::new(alien_core::GcpClientConfig {
        project_id: target.project_id.clone(),
        region: target.region.clone(),
        credentials,
        service_overrides: None,
        project_number: None,
    })))
}

/// Azure: Use target SP credentials directly for bootstrap-only operations such
/// as creating scoped e2e role assignments. Do not pass this config to the agent.
fn build_azure_target_config(config: &TestConfig) -> anyhow::Result<ClientConfig> {
    let target = config
        .azure_target
        .as_ref()
        .context("Missing Azure target credentials")?;
    Ok(ClientConfig::Azure(Box::new(
        alien_core::AzureClientConfig {
            subscription_id: target.subscription_id.clone(),
            tenant_id: target.tenant_id.clone(),
            region: Some(target.region.clone()),
            credentials: alien_core::AzureCredentials::ServicePrincipal {
                client_id: target.client_id.clone(),
                client_secret: target.client_secret.clone(),
            },
            service_overrides: None,
        },
    )))
}

/// Create scoped credentials and set them as environment variables.
///
/// This mirrors what a real customer would do: they have scoped credentials
/// in their environment, and `alien-deploy up` reads them via
/// `ClientConfig::from_std_env()`.
///
/// For AWS: creates a scoped IAM role with auto-generated permissions,
/// assumes it, and sets AWS_ACCESS_KEY_ID/SECRET/SESSION_TOKEN env vars.
/// For GCP/Azure: sets target credentials directly (scoped role creation TBD).
pub async fn set_scoped_credentials_env(
    config: &TestConfig,
    platform: Platform,
    _manager: &Arc<TestManager>,
    _management_config: Option<ManagementConfig>,
) -> anyhow::Result<()> {
    if !config.has_platform(platform) {
        anyhow::bail!(
            "Cannot set up scoped credentials for {}: missing credentials",
            platform.as_str()
        );
    }

    info!(
        platform = %platform.as_str(),
        "Setting scoped credentials as environment variables for alien-deploy up"
    );

    // Build the scoped config (creates IAM role for AWS, uses direct creds for GCP/Azure)
    let scoped_config = build_scoped_target_config(config, platform, "e2e-deploy", None)
        .await
        .context("Failed to build scoped target config")?;

    // Extract credentials and set as env vars so alien-deploy up inherits them
    match scoped_config {
        ClientConfig::Aws(aws_config) => {
            match &aws_config.credentials {
                alien_core::AwsCredentials::AccessKeys {
                    access_key_id,
                    secret_access_key,
                    session_token,
                } => {
                    std::env::set_var("AWS_ACCESS_KEY_ID", access_key_id);
                    std::env::set_var("AWS_SECRET_ACCESS_KEY", secret_access_key);
                    if let Some(token) = session_token {
                        std::env::set_var("AWS_SESSION_TOKEN", token);
                    }
                    std::env::set_var("AWS_REGION", &aws_config.region);
                    if !aws_config.account_id.is_empty() {
                        std::env::set_var("AWS_ACCOUNT_ID", &aws_config.account_id);
                    }
                }
                _ => anyhow::bail!("Expected AccessKeys credentials from scoped AWS config"),
            }
            info!("AWS scoped credentials set as environment variables");
        }
        ClientConfig::Gcp(gcp_config) => {
            std::env::set_var("GCP_PROJECT_ID", &gcp_config.project_id);
            std::env::set_var("GCP_REGION", &gcp_config.region);
            if let alien_core::GcpCredentials::ServiceAccountKey { json } = &gcp_config.credentials
            {
                std::env::set_var("GOOGLE_SERVICE_ACCOUNT_KEY", json);
                std::env::remove_var("GCP_ACCESS_TOKEN");
            } else if let alien_core::GcpCredentials::AccessToken { token } =
                &gcp_config.credentials
            {
                std::env::set_var("GCP_ACCESS_TOKEN", token);
                std::env::remove_var("GOOGLE_SERVICE_ACCOUNT_KEY");
            }
            info!("GCP credentials set as environment variables");
        }
        ClientConfig::Azure(azure_config) => {
            std::env::set_var("AZURE_SUBSCRIPTION_ID", &azure_config.subscription_id);
            std::env::set_var("AZURE_TENANT_ID", &azure_config.tenant_id);
            if let alien_core::AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            } = &azure_config.credentials
            {
                std::env::set_var("AZURE_CLIENT_ID", client_id);
                std::env::set_var("AZURE_CLIENT_SECRET", client_secret);
            }
            if let Some(ref region) = azure_config.region {
                std::env::set_var("AZURE_REGION", region);
            }
            info!("Azure credentials set as environment variables");
        }
        _ => anyhow::bail!("Unsupported platform for scoped credentials: {}", platform),
    }

    Ok(())
}

/// Tears down a deployment by running the deletion state machine locally
/// with target-environment credentials.
pub async fn teardown_target(
    config: &TestConfig,
    platform: Platform,
    deployment_id: &str,
    manager: &Arc<TestManager>,
) -> anyhow::Result<()> {
    info!(
        platform = %platform.as_str(),
        %deployment_id,
        "teardown_target: delegating to push_deletion"
    );

    let target_config = build_direct_target_config(config, platform)?;

    alien_deploy_cli::commands::push_deletion(
        manager.client(),
        deployment_id,
        platform,
        target_config,
    )
    .await
    .map_err(|e| anyhow::anyhow!("push_deletion failed: {}", e))?;

    info!(
        %deployment_id,
        "teardown_target complete — deployment resources deleted"
    );

    Ok(())
}

/// Build target config with direct credentials (no scoping).
/// Used for teardown where we need full permissions.
fn build_direct_target_config(
    config: &TestConfig,
    platform: Platform,
) -> anyhow::Result<ClientConfig> {
    match platform {
        Platform::Aws => {
            let target = config
                .aws_target
                .as_ref()
                .context("Missing AWS target credentials")?;
            Ok(ClientConfig::Aws(Box::new(alien_core::AwsClientConfig {
                account_id: target.account_id.clone().unwrap_or_default(),
                region: target.region.clone(),
                credentials: alien_core::AwsCredentials::AccessKeys {
                    access_key_id: target.access_key_id.clone(),
                    secret_access_key: target.secret_access_key.clone(),
                    session_token: target.session_token.clone(),
                },
                service_overrides: None,
            })))
        }
        Platform::Gcp => build_gcp_target_config(config),
        Platform::Azure => build_azure_target_config(config),
        other => anyhow::bail!("teardown not supported for platform: {}", other),
    }
}

/// Grant the deployment's management UAMI `managedEnvironments/join/action`
/// on the shared Container Apps Environment.
///
/// The shared environment is in a different resource group than the deployment,
/// so the management UAMI's RG-scoped role doesn't cover it. The role
/// definition is pre-created by Terraform; we just create the assignment here.
async fn grant_shared_env_join_permission(
    _config: &TestConfig,
    target_config: &ClientConfig,
    deployment: &TestDeployment,
    manager: &Arc<TestManager>,
    shared_env: &crate::config::SharedContainerEnvConfig,
) -> anyhow::Result<()> {
    use alien_azure_clients::authorization::{AuthorizationApi, Scope};
    use alien_azure_clients::models::authorization_role_assignments::{
        RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
    };
    use alien_azure_clients::AzureAuthorizationClient;

    let join_role_id = shared_env
        .join_role_definition_id
        .as_ref()
        .context("AZURE_SHARED_CONTAINER_ENV_JOIN_ROLE_ID not set — run terraform apply")?;

    let azure_config = target_config
        .azure_config()
        .context("Expected Azure config for shared env permission grant")?;

    // Get the management UAMI principal ID from the deployment's stack state
    let dep = manager
        .client()
        .get_deployment()
        .id(&deployment.id)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get deployment: {}", e))?
        .into_inner();

    let stack_state_json = dep
        .stack_state
        .context("Deployment has no stack_state after InitialSetup")?;
    let stack_state: alien_core::StackState =
        serde_json::from_value(stack_state_json).context("Failed to deserialize stack_state")?;

    // Find the remote-stack-management resource's controller state for the UAMI principal ID
    let mgmt_principal_id = stack_state
        .resources
        .iter()
        .find(|(_, s)| s.resource_type == "remote-stack-management")
        .and_then(|(_, s)| s.internal_state.as_ref())
        .and_then(|state| state.get("uamiPrincipalId"))
        .and_then(|v| v.as_str())
        .context("Could not find management UAMI principal ID in stack state")?
        .to_string();

    info!(
        principal_id = %mgmt_principal_id,
        shared_env = %shared_env.environment_name,
        "Granting managedEnvironments/join on shared environment for management UAMI"
    );

    // Create role assignment using the Terraform-provisioned role definition
    let token_cache = alien_azure_clients::AzureTokenCache::new(azure_config.clone());
    let http_client = reqwest::Client::new();
    let auth_client = AzureAuthorizationClient::new(http_client, token_cache);

    let env_scope = Scope::Resource {
        resource_group_name: shared_env.resource_group.clone(),
        resource_provider: "Microsoft.App".to_string(),
        parent_resource_path: None,
        resource_type: "managedEnvironments".to_string(),
        resource_name: shared_env.environment_name.clone(),
    };

    let assignment_id = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!(
            "alien:e2e:env-join-assign:{}:{}",
            deployment.name, mgmt_principal_id
        )
        .as_bytes(),
    )
    .to_string();

    let full_assignment_id = auth_client.build_role_assignment_id(&env_scope, assignment_id);

    auth_client
        .create_or_update_role_assignment_by_id(
            full_assignment_id,
            &RoleAssignment {
                id: None,
                name: None,
                type_: None,
                properties: Some(RoleAssignmentProperties {
                    principal_id: mgmt_principal_id.clone(),
                    role_definition_id: join_role_id.clone(),
                    scope: Some(env_scope.to_scope_string(azure_config)),
                    principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                    condition: None,
                    condition_version: None,
                    delegated_managed_identity_resource_id: None,
                    description: Some("E2E test: management UAMI join shared env".into()),
                    created_by: None,
                    created_on: None,
                    updated_by: None,
                    updated_on: None,
                }),
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to assign env join role: {}", e))?;

    info!(
        principal_id = %mgmt_principal_id,
        "Shared environment join permission granted"
    );

    Ok(())
}
