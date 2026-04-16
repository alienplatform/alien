//! Target account setup for cross-account E2E tests.
//!
//! Creates a scoped IAM role with auto-generated permissions (from
//! `alien-permissions`) and impersonates it during `push_initial_setup`.
//! This validates that the auto-generated permissions are sufficient for
//! initial setup — the real flow a customer admin would follow.

use std::sync::Arc;

use alien_core::{ClientConfig, ManagementConfig, Platform};
use alien_permissions::PermissionContext;
use anyhow::Context;
use tracing::info;

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

    let target_config = build_scoped_target_config(config, platform, &deployment.name)
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

/// Build a `ClientConfig` that impersonates a scoped role with auto-generated
/// permissions. For AWS, creates a temporary IAM role and returns credentials
/// that assume it. For GCP/Azure, uses target credentials directly for now
/// (scoped role creation for GCP/Azure is a follow-up).
async fn build_scoped_target_config(
    config: &TestConfig,
    platform: Platform,
    deployment_name: &str,
) -> anyhow::Result<ClientConfig> {
    match platform {
        Platform::Aws => build_aws_scoped_config(config, deployment_name).await,
        Platform::Gcp => build_gcp_target_config(config),
        Platform::Azure => build_azure_target_config(config),
        other => anyhow::bail!("setup_target not supported for platform: {}", other),
    }
}

/// AWS: Create a temporary IAM role with auto-generated permissions and
/// return a ClientConfig that assumes it.
async fn build_aws_scoped_config(
    config: &TestConfig,
    deployment_name: &str,
) -> anyhow::Result<ClientConfig> {
    use alien_aws_clients::{AwsClientConfigExt as _, AwsCredentialProvider, IamApi};
    use alien_core::{AwsClientConfig, AwsCredentials};

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

    // Use wildcard prefix — the actual resource_prefix is a random 8-char
    // string generated at deployment time (StackState::new()). We can't
    // predict it, so the scoped role allows any prefix in the target account.
    // In production, customers would scope to their specific stack prefix.
    let stack_prefix = "*";

    // Generate the scoped IAM policy from the stack definition
    let context = PermissionContext::new()
        .with_aws_account_id(&account_id)
        .with_aws_region(&target.region)
        .with_stack_prefix(stack_prefix)
        .with_managing_account_id(mgmt_account_id);

    let policy = alien_permissions::initial_setup::generate_aws_initial_setup_policy(&context)
        .map_err(|e| anyhow::anyhow!("Failed to generate initial setup policy: {}", e))?;

    let policy_json = serde_json::to_string(&policy).context("Failed to serialize IAM policy")?;

    info!(
        statements = policy.statement.len(),
        policy_size = policy_json.len(),
        "Generated scoped IAM policy for initial setup"
    );

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
    let role_name = format!("alien-e2e-setup-{}", role_suffix);

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
            description: Some(
                "Alien E2E scoped initial setup role (auto-generated permissions)".to_string(),
            ),
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

    // Attach the auto-generated policy as an inline policy
    iam_client
        .put_role_policy(&role_name, "alien-initial-setup", &policy_json)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to attach policy to scoped role: {}", e))?;

    info!(role_name = %role_name, "Attached auto-generated policy to scoped role");

    // Wait for IAM propagation
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    // Assume the scoped role
    let role_arn = format!("arn:aws:iam::{}:role/{}", account_id, role_name);
    let scoped_config = admin_config
        .impersonate(alien_aws_clients::AwsImpersonationConfig {
            role_arn: role_arn.clone(),
            session_name: Some("alien-e2e-initial-setup".to_string()),
            duration_seconds: Some(3600),
            external_id: None,
            target_region: None,
        })
        .await
        .map_err(|e| anyhow::anyhow!("Failed to assume scoped role {}: {}", role_arn, e))?;

    info!(role_arn = %role_arn, "Assumed scoped initial setup role");

    Ok(ClientConfig::Aws(Box::new(scoped_config)))
}

/// GCP: Use target SA credentials directly.
/// TODO: Create scoped custom role from auto-generated permissions.
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

/// Azure: Use target SP credentials directly.
/// TODO: Create scoped role definition from auto-generated permissions.
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
    let scoped_config = build_scoped_target_config(config, platform, "e2e-deploy")
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
    use alien_client_config::ClientConfigExt;

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
