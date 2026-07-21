//! Target account setup for cross-account E2E tests.
//!
//! AWS CLI push uses scoped credentials with auto-generated permissions from
//! `alien-permissions`. GCP and Azure CLI push use target credentials for
//! initial setup; scoped cloud deployment identities are covered by the
//! Terraform/Helm pull flows, not the legacy Docker cloud-pull path.

use std::sync::Arc;

use alien_aws_clients::{ErrorData as AwsErrorData, IamApi};
use alien_core::{ClientConfig, ManagementConfig, Platform, Stack};
use alien_permissions::generators::{AwsIamPolicy, AwsIamStatement};
use alien_permissions::PermissionContext;
use anyhow::Context;
use tracing::info;

use crate::config::TestConfig;
use crate::deployment::TestDeployment;
use crate::manager::TestManager;

/// Run the target-side setup for cross-account deployment.
///
/// 1. Builds the target-side credentials for the selected push flow.
/// 2. Runs `push_initial_setup` with those credentials.
/// 3. If initial setup succeeds, the deployment loop resumes using the
///    manager's own management identity.
///
/// After this function returns, the manager's deployment loop will resume
/// from `Provisioning` using its own management SA impersonation chain.
pub async fn setup_target(
    config: &TestConfig,
    platform: Platform,
    deployment: &TestDeployment,
    _stack: &Stack,
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
        "setup_target: preparing target credentials and running initial setup"
    );

    let target_config = build_initial_setup_target_config(config, platform, &deployment.name)
        .await
        .context("Failed to build initial setup target config")?;
    let has_remote_management = management_config.is_some();

    if let Err(error) = alien_deploy_cli::commands::push_initial_setup(
        manager.client(),
        &deployment.id,
        platform,
        None,
        target_config.clone(),
        management_config,
        &manager.public_url,
        &deployment.token,
        None, // no network override from tests
        None,
    )
    .await
    {
        let manager_error = manager
            .client()
            .get_deployment()
            .id(&deployment.id)
            .send()
            .await
            .ok()
            .and_then(|state| state.error.clone())
            .map(|state_error| state_error.to_string())
            .unwrap_or_else(|| "manager returned no deployment error details".to_string());
        anyhow::bail!(
            "push_initial_setup failed: {error}; manager deployment error: {manager_error}"
        );
    }

    // For Azure with shared (external) Container Apps Environment: when remote
    // stack management is configured, the management UAMI now exists but lacks
    // permissions on the shared environment (which is in a different resource
    // group). Grant it before the manager's Provisioning phase starts.
    if platform == Platform::Azure && has_remote_management {
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

/// Build a `ClientConfig` for initial setup.
async fn build_initial_setup_target_config(
    config: &TestConfig,
    platform: Platform,
    deployment_name: &str,
) -> anyhow::Result<ClientConfig> {
    match platform {
        Platform::Aws => build_aws_scoped_config(config, deployment_name).await,
        Platform::Gcp => {
            // GCP CLI push uses target credentials for initial setup. The
            // scoped identity path belonged to the removed legacy Docker
            // cloud-pull flow; pull is now covered by Terraform+Helm.
            build_gcp_target_config(config)
        }
        Platform::Azure => {
            // Azure CLI push uses the target credentials for initial setup for now.
            // The removed scoped identity path only modeled the legacy cloud-pull
            // Docker flow; pull is now covered by Terraform+Helm on Kubernetes.
            build_azure_target_config(config)
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

/// GCP: Use target SA credentials directly for CLI push initial setup.
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

/// Azure: Use target SP credentials directly for CLI push initial setup.
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

/// Grant the deployment's management UAMI permissions on the shared Container
/// Apps Environment.
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
        "Granting shared environment permissions for management UAMI"
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
                    description: Some("E2E test: management UAMI shared env access".into()),
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
        "Shared environment permissions granted"
    );

    Ok(())
}
