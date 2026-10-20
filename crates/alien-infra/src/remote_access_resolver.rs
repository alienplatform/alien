//! Remote access resolver for establishing connections to cloud platforms
//!
//! This module provides functionality to resolve stack state into authenticated client
//! configurations by performing impersonation based on RemoteStackManagement outputs.

use crate::error::{ErrorData, Result};
use crate::ClientConfigExt as _;
#[cfg(feature = "aws")]
use alien_aws_clients::AwsImpersonationConfig;
use alien_core::{
    ClientConfig, EnvironmentInfo, ImpersonationConfig, ManagementExternalId, Platform,
    RemoteBindingsOutputs, RemoteStackManagement, RemoteStackManagementOutputs, StackState,
};
use alien_error::{AlienError, Context, IntoAlienError};
#[cfg(feature = "gcp")]
use alien_gcp_clients::GcpClientConfigExt;
use std::collections::HashMap;
use tracing::info;
use uuid::Uuid;

const GCP_CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Service for resolving stack state into authenticated client configurations
#[derive(Debug)]
pub struct RemoteAccessResolver {
    /// Environment variables to use for platform configuration
    env: HashMap<String, String>,
}

impl RemoteAccessResolver {
    /// Create a new remote access resolver with a specific environment
    pub fn new(env: HashMap<String, String>) -> Self {
        Self { env }
    }

    /// Resolve stack state into an authenticated client configuration by impersonating
    /// the RemoteStackManagement resource identity.
    ///
    /// This method extracts the RemoteStackManagement outputs from the stack state and
    /// uses them to configure impersonation from the base configuration (Manager's
    /// ServiceAccount) to the startup cloud identity.
    ///
    /// # Arguments
    ///
    /// * `base_config` - The Manager's ServiceAccount configuration
    /// * `stack_state` - The stack state containing RemoteStackManagement outputs
    /// * `target_environment` - Optional target environment info. When provided, the
    ///   impersonated config will use the target's region/account/project instead of
    ///   inheriting from the management configuration.
    /// * `external_id` - The deployment's management ExternalId. AWS sends it as
    ///   `sts:ExternalId`; a role whose trust has no such condition ignores it, so
    ///   callers pass it whenever they know it. Other platforms have no equivalent
    ///   and ignore it.
    ///
    /// # Returns
    ///
    /// An authenticated client configuration that can be used to access the agent's cloud environment
    pub async fn resolve(
        &self,
        base_config: ClientConfig,
        stack_state: &StackState,
        target_environment: Option<&EnvironmentInfo>,
        external_id: Option<&ManagementExternalId>,
    ) -> Result<ClientConfig> {
        let remote_mgmt_outputs = self.find_remote_stack_management_outputs(stack_state)?;

        // Determine platform and perform appropriate impersonation
        match base_config.platform() {
            Platform::Aws => {
                self.resolve_aws_impersonation(
                    base_config,
                    &remote_mgmt_outputs,
                    target_environment,
                    external_id,
                    None,
                )
                .await
            }
            Platform::Gcp => {
                Err(AlienError::new(ErrorData::RemoteAccessInvalid {
                    message: "GCP target access requires the manager's assertion source and authenticated installation scope".to_string(),
                    field_name: Some("federation".to_string()),
                }))
            }
            Platform::Azure => {
                self.resolve_azure_impersonation(
                    base_config,
                    &remote_mgmt_outputs,
                    target_environment,
                )
                .await
            }
            _ => Err(AlienError::new(ErrorData::RemoteAccessInvalid {
                message: format!(
                    "{:?} platform does not support remote access impersonation",
                    base_config.platform()
                ),
                field_name: Some("platform".to_string()),
            })),
        }
    }

    /// Resolve the dedicated Remote Bindings identity without borrowing the
    /// management resource's output slot. Its AWS role trusts the same
    /// ExternalId as the management role.
    pub async fn resolve_remote_bindings(
        &self,
        base_config: ClientConfig,
        outputs: &RemoteBindingsOutputs,
        target_environment: Option<&EnvironmentInfo>,
        session_name: Option<String>,
        external_id: Option<&ManagementExternalId>,
    ) -> Result<ClientConfig> {
        let access = RemoteStackManagementOutputs {
            management_resource_id: outputs.resource_id.clone(),
            access_configuration: outputs.access_configuration.clone(),
            legacy_remote_bindings_access: None,
        };
        match base_config.platform() {
            Platform::Aws => {
                self.resolve_aws_impersonation(
                    base_config,
                    &access,
                    target_environment,
                    external_id,
                    session_name,
                )
                .await
            }
            Platform::Gcp => {
                Err(AlienError::new(ErrorData::RemoteAccessInvalid {
                    message: "GCP target access requires the manager's assertion source and authenticated installation scope".to_string(),
                    field_name: Some("federation".to_string()),
                }))
            }
            Platform::Azure => {
                self.resolve_azure_impersonation(base_config, &access, target_environment)
                    .await
            }
            platform => Err(AlienError::new(ErrorData::RemoteAccessInvalid {
                message: format!("{platform:?} platform does not support Remote Bindings"),
                field_name: Some("platform".to_string()),
            })),
        }
    }

    /// Find RemoteStackManagement outputs in the stack state
    fn find_remote_stack_management_outputs(
        &self,
        stack_state: &StackState,
    ) -> Result<RemoteStackManagementOutputs> {
        // Look for RemoteStackManagement resource in the stack state
        for (_resource_id, resource_state) in &stack_state.resources {
            if resource_state.resource_type == RemoteStackManagement::RESOURCE_TYPE.to_string() {
                if let Some(outputs) = &resource_state.outputs {
                    // Try to downcast to RemoteStackManagementOutputs
                    if let Some(remote_mgmt_outputs) =
                        outputs.downcast_ref::<RemoteStackManagementOutputs>()
                    {
                        return Ok(remote_mgmt_outputs.clone());
                    }
                }
            }
        }

        Err(AlienError::new(ErrorData::InfrastructureError {
            message: "RemoteStackManagement resource not found in stack state or missing outputs"
                .to_string(),
            operation: Some("find_remote_stack_management".to_string()),
            resource_id: None,
        }))
    }

    /// Resolve AWS impersonation using RemoteStackManagement outputs
    async fn resolve_aws_impersonation(
        &self,
        base_config: ClientConfig,
        outputs: &RemoteStackManagementOutputs,
        target_environment: Option<&EnvironmentInfo>,
        external_id: Option<&ManagementExternalId>,
        session_name: Option<String>,
    ) -> Result<ClientConfig> {
        let role_arn = &outputs.access_configuration;
        info!(
            external_id_sent = external_id.is_some(),
            "Resolving AWS impersonation for role: {}", role_arn
        );

        // Extract target region from environment info if available.
        let target_region = target_environment.and_then(|env| match env {
            EnvironmentInfo::Aws(info) => Some(info.region.clone()),
            _ => None,
        });

        let impersonation_config = ImpersonationConfig::Aws(AwsImpersonationConfig {
            role_arn: role_arn.clone(),
            session_name: Some(session_name.unwrap_or_else(|| {
                format!("deployment-remote-access-{}", Uuid::new_v4().simple())
            })),
            duration_seconds: Some(3600),
            external_id: external_id.map(|value| value.expose().to_string()),
            target_region,
        });

        base_config.impersonate(impersonation_config).await.context(
            ErrorData::AuthenticationFailed {
                message: format!("Failed to assume AWS role: {}", role_arn),
                method: Some("role_assumption".to_string()),
            },
        )
    }

    /// Resolve GCP target access using a trusted local assertion source and the
    /// authenticated installation record. Neither comes from import outputs.
    pub async fn resolve_gcp_federation(
        &self,
        source: alien_core::GcpFederationSource,
        scope: &alien_core::GcpFederationScope,
        stack_state: &StackState,
        target_environment: Option<&EnvironmentInfo>,
    ) -> Result<ClientConfig> {
        let outputs = self.find_remote_stack_management_outputs(stack_state)?;
        self.gcp_federated_config(
            &outputs,
            &RemoteStackManagement::RESOURCE_TYPE,
            source,
            scope,
            target_environment,
        )
        .await
    }

    /// Published-resource access uses its separate target identity and purpose.
    pub async fn resolve_gcp_remote_bindings(
        &self,
        source: alien_core::GcpFederationSource,
        scope: &alien_core::GcpFederationScope,
        outputs: &RemoteBindingsOutputs,
        target_environment: Option<&EnvironmentInfo>,
    ) -> Result<ClientConfig> {
        let access = RemoteStackManagementOutputs {
            management_resource_id: outputs.resource_id.clone(),
            access_configuration: outputs.access_configuration.clone(),
            legacy_remote_bindings_access: None,
        };
        self.gcp_federated_config(
            &access,
            &alien_core::RemoteBindings::RESOURCE_TYPE,
            source,
            scope,
            target_environment,
        )
        .await
    }

    async fn gcp_federated_config(
        &self,
        outputs: &RemoteStackManagementOutputs,
        purpose: &alien_core::ResourceType,
        source: alien_core::GcpFederationSource,
        scope: &alien_core::GcpFederationScope,
        target_environment: Option<&EnvironmentInfo>,
    ) -> Result<ClientConfig> {
        let access: alien_core::GcpFederationAccess =
            serde_json::from_str(&outputs.access_configuration)
                .into_alien_error()
                .context(ErrorData::RemoteAccessInvalid {
                    message: "GCP access requires a federation descriptor; regenerate setup"
                        .to_string(),
                    field_name: Some("accessConfiguration".to_string()),
                })?;
        scope
            .validate_access(&access, purpose)
            .context(ErrorData::RemoteAccessInvalid {
                message: "GCP access does not match the authorized installation and purpose"
                    .to_string(),
                field_name: Some("assertionAudience".to_string()),
            })?;
        let Some(EnvironmentInfo::Gcp(environment)) = target_environment else {
            return Err(AlienError::new(ErrorData::RemoteAccessInvalid {
                message: "GCP federation requires the recorded target project and region"
                    .to_string(),
                field_name: Some("environmentInfo".to_string()),
            }));
        };
        if outputs.management_resource_id != access.service_account_email
            || environment.project_number != access.project_number
            || !access.service_account_email.ends_with(&format!(
                "@{}.iam.gserviceaccount.com",
                environment.project_id
            ))
        {
            return Err(AlienError::new(ErrorData::RemoteAccessInvalid {
                message: "GCP federation must identify the installed target account and project"
                    .to_string(),
                field_name: Some("accessConfiguration".to_string()),
            }));
        }
        let config = alien_core::GcpClientConfig {
            project_id: environment.project_id.clone(),
            region: environment.region.clone(),
            project_number: Some(access.project_number.clone()),
            service_overrides: None,
            credentials: alien_core::GcpCredentials::Federated {
                access,
                source,
                lifetime_seconds: None,
            },
        };
        // Keep the existing handoff proof before the deployment loop starts.
        // The returned configuration renews through the same source thereafter.
        config
            .get_bearer_token(GCP_CLOUD_PLATFORM_SCOPE)
            .await
            .context(ErrorData::AuthenticationFailed {
                message: "Cannot acquire deployment credentials through its federation trust"
                    .to_string(),
                method: Some("workload_identity_federation".to_string()),
            })?;
        Ok(ClientConfig::Gcp(Box::new(config)))
    }

    /// Resolve Azure impersonation using target-side UAMI Workload Identity.
    ///
    /// The access_configuration from RSM outputs is JSON:
    ///   { "uamiClientId": "<client-id>", "tenantId": "<customer-tenant-id>" }
    ///
    /// The manager process must expose AZURE_FEDERATED_TOKEN_FILE. The target
    /// subscription trusts that token through the Federated Identity Credential
    /// created on the RemoteStackManagement UAMI during setup.
    async fn resolve_azure_impersonation(
        &self,
        base_config: ClientConfig,
        outputs: &RemoteStackManagementOutputs,
        target_environment: Option<&EnvironmentInfo>,
    ) -> Result<ClientConfig> {
        let access_config: serde_json::Value = serde_json::from_str(&outputs.access_configuration)
            .into_alien_error()
            .context(ErrorData::RemoteAccessInvalid {
                message: "Failed to parse Azure access configuration JSON".to_string(),
                field_name: Some("access_configuration".to_string()),
            })?;

        let uami_client_id = access_config["uamiClientId"].as_str().ok_or_else(|| {
            AlienError::new(ErrorData::RemoteAccessInvalid {
                message: "Azure access configuration missing 'uamiClientId'".to_string(),
                field_name: Some("uamiClientId".to_string()),
            })
        })?;

        let customer_tenant_id = access_config["tenantId"].as_str().ok_or_else(|| {
            AlienError::new(ErrorData::RemoteAccessInvalid {
                message: "Azure access configuration missing 'tenantId'".to_string(),
                field_name: Some("tenantId".to_string()),
            })
        })?;

        // Extract target subscription/region from environment info
        let (target_subscription, target_region) = match target_environment {
            Some(EnvironmentInfo::Azure(info)) => {
                (info.subscription_id.clone(), Some(info.location.clone()))
            }
            _ => match &base_config {
                ClientConfig::Azure(cfg) => (cfg.subscription_id.clone(), cfg.region.clone()),
                _ => {
                    return Err(AlienError::new(ErrorData::RemoteAccessInvalid {
                        message: "Expected Azure base config for Azure impersonation".to_string(),
                        field_name: Some("platform".to_string()),
                    }))
                }
            },
        };

        let token_file = self.env.get("AZURE_FEDERATED_TOKEN_FILE").ok_or_else(|| {
            AlienError::new(ErrorData::AuthenticationFailed {
                message: "AZURE_FEDERATED_TOKEN_FILE is required for Azure remote stack access"
                    .to_string(),
                method: Some("azure_workload_identity".to_string()),
            })
        })?;

        info!(
            uami_client_id = %uami_client_id,
            customer_tenant_id = %customer_tenant_id,
            "Resolving Azure access via OIDC WorkloadIdentity"
        );

        let authority_host = self
            .env
            .get("AZURE_AUTHORITY_HOST")
            .cloned()
            .unwrap_or_else(|| "https://login.microsoftonline.com/".to_string());

        Ok(ClientConfig::Azure(Box::new(
            alien_azure_clients::AzureClientConfig {
                subscription_id: target_subscription,
                tenant_id: customer_tenant_id.to_string(),
                region: target_region,
                credentials: alien_azure_clients::AzureCredentials::WorkloadIdentity {
                    client_id: uami_client_id.to_string(),
                    tenant_id: customer_tenant_id.to_string(),
                    federated_token_file: token_file.clone(),
                    authority_host,
                },
                service_overrides: None,
            },
        )))
    }

    /// Create a base client configuration for the specified platform
    ///
    /// This is a convenience method to create a base configuration from environment variables
    /// that can then be used with `resolve()` to establish remote access.
    pub async fn create_base_config(&self, platform: Platform) -> Result<ClientConfig> {
        info!("Creating base client config for platform: {}", platform);

        ClientConfig::from_env(platform, &self.env)
            .await
            .context(ErrorData::ClientConfigInvalid {
                platform,
                message: "Failed to load platform configuration from environment".to_string(),
            })
    }
}

impl Default for RemoteAccessResolver {
    fn default() -> Self {
        Self::new(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        GcpClientConfig, GcpCredentials, GcpEnvironmentInfo, GcpImpersonationConfig,
        GcpServiceOverrides, Resource, ResourceLifecycle, ResourceOutputs, ResourceStatus,
        StackResourceState,
    };
    use httpmock::{Method::POST, MockServer};
    use serde_json::json;

    fn gcp_stack_state(service_account_email: &str, access_configuration: &str) -> StackState {
        let config = RemoteStackManagement::new("management".to_string()).build();
        let mut state = StackState::new(Platform::Gcp);
        state.resources.insert(
            config.id.clone(),
            StackResourceState::builder()
                .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(config))
                .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                    management_resource_id: service_account_email.to_string(),
                    access_configuration: access_configuration.to_string(),
                    legacy_remote_bindings_access: None,
                }))
                .lifecycle(ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .build(),
        );
        state
    }

    fn gcp_base_config(iam_credentials_url: String) -> ClientConfig {
        ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "managing-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "source-token".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([("iamcredentials".to_string(), iam_credentials_url)]),
            }),
            project_number: Some("123456789".to_string()),
        }))
    }

    fn gcp_two_hop_base_config(iam_credentials_url: String) -> ClientConfig {
        let ClientConfig::Gcp(base) = gcp_base_config(iam_credentials_url) else {
            unreachable!("GCP test config must be GCP")
        };
        let service_overrides = base.service_overrides.clone();
        let project_number = base.project_number.clone();
        ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: base.project_id.clone(),
            region: base.region.clone(),
            credentials: GcpCredentials::ImpersonatedServiceAccount {
                source: base,
                config: GcpImpersonationConfig {
                    service_account_email: "management@managing-project.iam.gserviceaccount.com"
                        .to_string(),
                    scopes: vec![GCP_CLOUD_PLATFORM_SCOPE.to_string()],
                    delegates: None,
                    lifetime: Some("3600s".to_string()),
                    target_project_id: None,
                    target_region: None,
                },
            },
            service_overrides,
            project_number,
        }))
    }

    fn gcp_target_environment() -> EnvironmentInfo {
        EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_id: "target-project".to_string(),
            project_number: "987654321".to_string(),
            region: "us-east1".to_string(),
        })
    }

    #[test]
    fn test_remote_access_resolver_creation() {
        let resolver = RemoteAccessResolver::new(HashMap::new());
        assert!(resolver.env.is_empty());

        let mut env = HashMap::new();
        env.insert("TEST_VAR".to_string(), "test_value".to_string());
        let resolver = RemoteAccessResolver::new(env);
        assert_eq!(
            resolver.env.get("TEST_VAR"),
            Some(&"test_value".to_string())
        );
    }

    #[test]
    fn test_default_resolver() {
        let resolver = RemoteAccessResolver::default();
        assert!(resolver.env.is_empty());
    }

    #[tokio::test]
    async fn gcp_remote_access_cannot_fall_back_to_direct_impersonation() {
        let server = MockServer::start_async().await;
        let exchange = server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(200)
                    .json_body(json!({"accessToken": "unexpected-token"}));
            })
            .await;
        let email = "deployment@target-project.iam.gserviceaccount.com";
        for base in [
            gcp_base_config(server.url("/v1")),
            gcp_two_hop_base_config(server.url("/v1")),
        ] {
            let error = RemoteAccessResolver::default()
                .resolve(
                    base,
                    &gcp_stack_state(email, email),
                    Some(&gcp_target_environment()),
                    None,
                )
                .await
                .expect_err("GCP requires an authenticated federation scope and source");
            assert_eq!(error.code, "REMOTE_ACCESS_INVALID");
        }
        assert_eq!(exchange.hits_async().await, 0);
    }

    #[tokio::test]
    async fn gcp_federation_rejects_imported_scope_or_target_changes_before_exchange() {
        use alien_core::{
            GcpFederationAccess, GcpFederationScope, GcpFederationSource, GcpFederationTrust,
        };
        let server = MockServer::start_async().await;
        let exchange = server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(200)
                    .json_body(json!({"accessToken": "unexpected-token"}));
            })
            .await;
        let scope = GcpFederationScope {
            deployment_id: "dep_authorized".to_string(),
        };
        let access = GcpFederationAccess {
            project_number: "987654321".to_string(),
            pool_id: scope
                .pool_id(&RemoteStackManagement::RESOURCE_TYPE)
                .unwrap(),
            provider_id: "manager".to_string(),
            service_account_email: "deployment@target-project.iam.gserviceaccount.com".to_string(),
            assertion_audience: scope
                .assertion_audience(&RemoteStackManagement::RESOURCE_TYPE)
                .unwrap(),
            trust: GcpFederationTrust {
                issuer: "https://accounts.google.com".to_string(),
                subject: "123456789".to_string(),
            },
        };
        let ClientConfig::Gcp(base) = gcp_base_config(server.url("/v1")) else {
            unreachable!()
        };
        let source = GcpFederationSource::GoogleServiceAccount {
            source: base,
            service_account_email: "manager@managing-project.iam.gserviceaccount.com".to_string(),
            service_account_unique_id: access.trust.subject.clone(),
        };
        let mut other_scope = access.clone();
        other_scope.assertion_audience = GcpFederationScope {
            deployment_id: "dep_other".to_string(),
        }
        .assertion_audience(&RemoteStackManagement::RESOURCE_TYPE)
        .unwrap();
        let mut other_purpose = access.clone();
        other_purpose.assertion_audience = scope
            .assertion_audience(&alien_core::RemoteBindings::RESOURCE_TYPE)
            .unwrap();
        let mut other_project = access.clone();
        other_project.project_number = "111111111".to_string();
        let mut other_account = access.clone();
        other_account.service_account_email =
            "deployment@other-project.iam.gserviceaccount.com".to_string();
        for descriptor in [other_scope, other_purpose, other_project, other_account] {
            let state = gcp_stack_state(
                &descriptor.service_account_email,
                &serde_json::to_string(&descriptor).unwrap(),
            );
            let error = RemoteAccessResolver::default()
                .resolve_gcp_federation(
                    source.clone(),
                    &scope,
                    &state,
                    Some(&gcp_target_environment()),
                    None,
                )
                .await
                .expect_err(
                    "import outputs cannot select another installation, purpose, or project",
                );
            assert_eq!(error.code, "REMOTE_ACCESS_INVALID");
        }
        assert_eq!(exchange.hits_async().await, 0);
    }

    #[cfg(feature = "aws")]
    mod aws {
        use super::*;
        use alien_core::{
            AwsClientConfig, AwsCredentials, AwsEnvironmentInfo, AwsServiceOverrides,
        };

        const TARGET_ROLE: &str = "arn:aws:iam::210987654321:role/customer-management";
        const EXTERNAL_ID: &str = "4be1d6f0c2a94e1b8f3d7a6c5e2b1f0a";

        fn base_config(sts_endpoint: String) -> ClientConfig {
            ClientConfig::Aws(Box::new(AwsClientConfig {
                account_id: "123456789012".to_string(),
                region: "us-east-2".to_string(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: "AKIATEST".to_string(),
                    secret_access_key: "test-secret".to_string(),
                    session_token: None,
                },
                service_overrides: Some(AwsServiceOverrides {
                    endpoints: HashMap::from([("sts".to_string(), sts_endpoint)]),
                }),
            }))
        }

        fn stack_state() -> StackState {
            let config = RemoteStackManagement::new("management".to_string()).build();
            let mut state = StackState::new(Platform::Aws);
            state.resources.insert(
                config.id.clone(),
                StackResourceState::builder()
                    .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
                    .status(ResourceStatus::Running)
                    .config(Resource::new(config))
                    .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                        management_resource_id: TARGET_ROLE.to_string(),
                        access_configuration: TARGET_ROLE.to_string(),
                        legacy_remote_bindings_access: None,
                    }))
                    .lifecycle(ResourceLifecycle::Frozen)
                    .dependencies(vec![])
                    .build(),
            );
            state
        }

        fn target() -> EnvironmentInfo {
            EnvironmentInfo::Aws(AwsEnvironmentInfo {
                account_id: "210987654321".to_string(),
                region: "us-east-2".to_string(),
            })
        }

        fn assume_role_response() -> String {
            r#"<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleResult>
    <AssumedRoleUser>
      <Arn>arn:aws:sts::210987654321:assumed-role/customer-management/session</Arn>
      <AssumedRoleId>AROA:session</AssumedRoleId>
    </AssumedRoleUser>
    <Credentials>
      <AccessKeyId>ASIATARGET</AccessKeyId>
      <SecretAccessKey>target-secret</SecretAccessKey>
      <SessionToken>target-session-token</SessionToken>
      <Expiration>2026-05-27T11:00:00Z</Expiration>
    </Credentials>
  </AssumeRoleResult>
  <ResponseMetadata><RequestId>request-1</RequestId></ResponseMetadata>
</AssumeRoleResponse>"#
                .to_string()
        }

        /// The mock STS only answers an AssumeRole of the customer's role that
        /// carries the deployment's ExternalId, the way a trust policy with an
        /// `sts:ExternalId` condition behaves. Everything else is denied.
        async fn sts_requiring_external_id(server: &MockServer) -> httpmock::Mock<'_> {
            let role = urlencoding::encode(TARGET_ROLE).into_owned();
            server
                .mock_async(|when, then| {
                    when.method(POST)
                        .body_contains("Action=AssumeRole")
                        .body_contains(format!("RoleArn={role}"))
                        .body_contains(format!("ExternalId={EXTERNAL_ID}"));
                    then.status(200)
                        .header("content-type", "text/xml")
                        .body(assume_role_response());
                })
                .await
        }

        async fn sts_denying_everything_else(server: &MockServer) -> httpmock::Mock<'_> {
            server
                .mock_async(|when, then| {
                    when.method(POST);
                    then.status(403).header("content-type", "text/xml").body(
                        r#"<ErrorResponse><Error><Code>AccessDenied</Code><Message>not authorized to perform sts:AssumeRole</Message></Error></ErrorResponse>"#,
                    );
                })
                .await
        }

        #[tokio::test]
        async fn management_access_sends_the_deployment_external_id() {
            let server = MockServer::start_async().await;
            let protected = sts_requiring_external_id(&server).await;
            let external_id = ManagementExternalId::new(EXTERNAL_ID).unwrap();

            let resolved = RemoteAccessResolver::default()
                .resolve(
                    base_config(server.base_url()),
                    &stack_state(),
                    Some(&target()),
                    Some(&external_id),
                )
                .await
                .expect("a role that requires the ExternalId accepts the deployment's value");

            protected.assert_async().await;
            let aws = resolved.aws_config().expect("resolved access stays AWS");
            assert_eq!(aws.account_id, "210987654321");
            assert!(matches!(
                &aws.credentials,
                AwsCredentials::SessionCredentials { access_key_id, .. } if access_key_id == "ASIATARGET"
            ));
        }

        #[tokio::test]
        async fn management_access_without_the_external_id_is_denied_by_a_protected_role() {
            let server = MockServer::start_async().await;
            let protected = sts_requiring_external_id(&server).await;
            let denied = sts_denying_everything_else(&server).await;

            let error = RemoteAccessResolver::default()
                .resolve(
                    base_config(server.base_url()),
                    &stack_state(),
                    Some(&target()),
                    None,
                )
                .await
                .expect_err("a protected role must refuse a request without the ExternalId");

            assert_eq!(protected.hits_async().await, 0);
            assert!(denied.hits_async().await >= 1, "the request must reach STS");
            assert_eq!(error.code, "AUTHENTICATION_FAILED");
        }

        #[tokio::test]
        async fn remote_bindings_access_sends_the_same_external_id() {
            let server = MockServer::start_async().await;
            let protected = sts_requiring_external_id(&server).await;
            let external_id = ManagementExternalId::new(EXTERNAL_ID).unwrap();
            let outputs = alien_core::RemoteBindingsOutputs {
                resource_id: TARGET_ROLE.to_string(),
                access_configuration: TARGET_ROLE.to_string(),
            };

            RemoteAccessResolver::default()
                .resolve_remote_bindings(
                    base_config(server.base_url()),
                    &outputs,
                    Some(&target()),
                    Some("remote-bindings-session".to_string()),
                    Some(&external_id),
                )
                .await
                .expect("the remote-bindings role trusts the same ExternalId");

            protected.assert_async().await;
        }
    }
}
