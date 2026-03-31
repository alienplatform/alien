use crate::{
    error::{map_cloud_client_error, Error, ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions,
        AwsCrossAccountAccess, Binding, ComputeServiceType, CrossAccountAccess,
        CrossAccountPermissions, RepositoryResponse,
    },
};
use alien_aws_clients::{
    ecr::{
        CreateRepositoryRequest, DescribeRepositoriesRequest, EcrApi, EcrClient,
        GetAuthorizationTokenRequest, GetRepositoryPolicyRequest, SetRepositoryPolicyRequest,
    },
    sts::{AssumeRoleRequest, StsApi, StsClient},
    AwsClientConfigExt as _, AwsCredentialProvider,
};
use alien_core::bindings::{ArtifactRegistryBinding, EcrArtifactRegistryBinding};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use base64::engine::{general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{info, warn};

/// AWS ECR implementation of the ArtifactRegistry binding.
#[derive(Debug)]
pub struct EcrArtifactRegistry {
    credentials: AwsCredentialProvider,
    ecr_client: EcrClient,
    binding_name: String,
    repository_prefix: String,
    pull_role_arn: Option<String>,
    push_role_arn: Option<String>,
}

impl EcrArtifactRegistry {
    /// Creates a new AWS ECR artifact registry binding from binding parameters.
    pub async fn new(
        binding_name: String,
        binding: ArtifactRegistryBinding,
        credentials: &AwsCredentialProvider,
    ) -> Result<Self> {
        info!(
            binding_name = %binding_name,
            "Initializing AWS ECR artifact registry"
        );

        let client = crate::http_client::create_http_client();
        let ecr_client = EcrClient::new(client, credentials.clone());

        // Extract values from binding
        let config = match binding {
            ArtifactRegistryBinding::Ecr(config) => config,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Expected ECR binding, got different service type".to_string(),
                }));
            }
        };

        let repository_prefix = config
            .repository_prefix
            .into_value(&binding_name, "repository_prefix")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract repository_prefix from binding".to_string(),
            })?;

        let pull_role_arn = config
            .pull_role_arn
            .into_value(&binding_name, "pull_role_arn")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract pull_role_arn from binding".to_string(),
            })?;

        let push_role_arn = config
            .push_role_arn
            .into_value(&binding_name, "push_role_arn")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract push_role_arn from binding".to_string(),
            })?;

        Ok(Self {
            credentials: credentials.clone(),
            ecr_client,
            binding_name,
            repository_prefix,
            pull_role_arn,
            push_role_arn,
        })
    }

    /// Constructs the full repository name for ECR using the repository prefix
    fn make_full_repo_name(&self, repo_name: &str) -> String {
        if !self.repository_prefix.is_empty() {
            format!("{}-{}", self.repository_prefix, repo_name)
        } else {
            repo_name.to_string()
        }
    }

    /// Internal helper to set the complete ECR policy from an AwsCrossAccountAccess configuration
    async fn set_full_policy(
        &self,
        repo_name: &str,
        aws_access: &AwsCrossAccountAccess,
    ) -> Result<()> {
        let mut statements = Vec::new();

        // Add cross-account access for specific role ARNs
        if !aws_access.role_arns.is_empty() {
            statements.push(json!({
                "Sid": "CrossAccountRolePermission",
                "Effect": "Allow",
                "Principal": {
                    "AWS": aws_access.role_arns
                },
                "Action": [
                    "ecr:BatchCheckLayerAvailability",
                    "ecr:GetDownloadUrlForLayer",
                    "ecr:BatchGetImage"
                ]
            }));
        }

        // Add service-specific access based on compute service types
        for service_type in &aws_access.allowed_service_types {
            match service_type {
                ComputeServiceType::Function => {
                    // Add Lambda service access if Function service type is specified
                    if !aws_access.account_ids.is_empty() {
                        let source_arns: Vec<String> = aws_access
                            .account_ids
                            .iter()
                            .map(|account_id| format!("arn:aws:lambda:*:{}:function:*", account_id))
                            .collect();

                        statements.push(json!({
                            "Sid": "LambdaServiceAccess",
                            "Effect": "Allow",
                            "Principal": {
                                "Service": "lambda.amazonaws.com"
                            },
                            "Action": [
                                "ecr:BatchGetImage",
                                "ecr:GetDownloadUrlForLayer"
                            ],
                            "Condition": {
                                "ArnLike": {
                                    "aws:sourceARN": source_arns
                                }
                            }
                        }));
                    }
                } // Future service types would be handled here
            }
        }

        // Create ECR policy JSON
        let policy = json!({
            "Version": "2012-10-17",
            "Statement": statements
        });

        let request = SetRepositoryPolicyRequest::builder()
            .repository_name(repo_name.to_string())
            .policy_text(policy.to_string())
            .build();

        self.ecr_client
            .set_repository_policy(request)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!(
                        "Failed to set cross-account access for ECR repository '{}'",
                        repo_name
                    ),
                    Some(repo_name.to_string()),
                )
            })?;

        info!(
            repo_name = %repo_name,
            "ECR repository cross-account access policy updated successfully"
        );
        Ok(())
    }
}

impl Binding for EcrArtifactRegistry {}

#[async_trait]
impl ArtifactRegistry for EcrArtifactRegistry {
    async fn create_repository(&self, repo_name: &str) -> Result<RepositoryResponse> {
        let full_repo_name = self.make_full_repo_name(repo_name);

        info!(
            repo_name = %repo_name,
            full_repo_name = %full_repo_name,
            "Creating ECR repository"
        );

        // Assume the push role for repository creation
        let push_role_arn = self.push_role_arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: self.binding_name.clone(),
                reason: "Push role ARN not available".to_string(),
            })
        })?;
        let impersonated = self
            .credentials
            .config()
            .impersonate(alien_aws_clients::AwsImpersonationConfig {
                role_arn: push_role_arn.clone(),
                session_name: Some("alien-ecr-create".to_string()),
                duration_seconds: None,
                external_id: None,
                target_region: None,
            })
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    "Failed to assume ECR push role".to_string(),
                    Some(repo_name.to_string()),
                )
            })?;
        let ecr_client = alien_aws_clients::ecr::EcrClient::new(
            crate::http_client::create_http_client(),
            AwsCredentialProvider::from_config(impersonated)
                .await
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "artifact_registry.ecr".to_string(),
                    reason: "Failed to create credential provider for impersonated role"
                        .to_string(),
                })?,
        );

        let request = CreateRepositoryRequest::builder()
            .repository_name(full_repo_name.clone())
            .build();

        let response = ecr_client.create_repository(request).await.map_err(|e| {
            map_cloud_client_error(
                e,
                format!("Failed to create ECR repository '{}'", full_repo_name),
                Some(repo_name.to_string()),
            )
        })?;

        info!(
            repo_name = %repo_name,
            full_repo_name = %full_repo_name,
            "ECR repository created successfully"
        );

        // ECR repositories are ready immediately after creation
        let repository = &response.repository;
        let created_at = if repository.created_at > 0.0 {
            DateTime::from_timestamp(repository.created_at as i64, 0).map(|dt| dt.to_rfc3339())
        } else {
            None
        };

        Ok(RepositoryResponse {
            name: repo_name.to_string(),
            uri: Some(repository.repository_uri.clone()),
            created_at,
        })
    }

    async fn get_repository(&self, repo_id: &str) -> Result<RepositoryResponse> {
        let full_repo_name = self.make_full_repo_name(repo_id);

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            "Getting ECR repository details"
        );

        // Assume the pull role for repository reads
        let pull_role_arn = self.pull_role_arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: self.binding_name.clone(),
                reason: "Pull role ARN not available".to_string(),
            })
        })?;
        let impersonated = self
            .credentials
            .config()
            .impersonate(alien_aws_clients::AwsImpersonationConfig {
                role_arn: pull_role_arn.clone(),
                session_name: Some("alien-ecr-describe".to_string()),
                duration_seconds: None,
                external_id: None,
                target_region: None,
            })
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    "Failed to assume ECR pull role".to_string(),
                    Some(repo_id.to_string()),
                )
            })?;
        let ecr_client = alien_aws_clients::ecr::EcrClient::new(
            crate::http_client::create_http_client(),
            AwsCredentialProvider::from_config(impersonated)
                .await
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "artifact_registry.ecr".to_string(),
                    reason: "Failed to create credential provider for impersonated role"
                        .to_string(),
                })?,
        );

        let request = DescribeRepositoriesRequest::builder()
            .repository_names(vec![full_repo_name.clone()])
            .build();

        let response = ecr_client
            .describe_repositories(request)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!(
                        "Failed to get ECR repository details for '{}'",
                        full_repo_name
                    ),
                    Some(repo_id.to_string()),
                )
            })?;

        if response.repositories.is_empty() {
            warn!(
                repo_id = %repo_id,
                full_repo_name = %full_repo_name,
                "ECR repository not found"
            );

            return Err(AlienError::new(ErrorData::ResourceNotFound {
                resource_id: repo_id.to_string(),
            }));
        }

        let repository = &response.repositories[0];
        let created_at = if repository.created_at > 0.0 {
            DateTime::from_timestamp(repository.created_at as i64, 0).map(|dt| dt.to_rfc3339())
        } else {
            None
        };

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            repo_uri = %repository.repository_uri,
            "ECR repository details retrieved"
        );

        Ok(RepositoryResponse {
            name: repository.repository_name.clone(),
            uri: Some(repository.repository_uri.clone()),
            created_at,
        })
    }

    async fn add_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<()> {
        let full_repo_name = self.make_full_repo_name(repo_id);

        let aws_access = match access {
            CrossAccountAccess::Aws(aws_access) => aws_access,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: self.binding_name.clone(),
                    reason: "AWS artifact registry can only accept AWS cross-account access configuration".to_string(),
                }));
            }
        };

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            account_ids = ?aws_access.account_ids,
            allowed_service_types = ?aws_access.allowed_service_types,
            role_arns = ?aws_access.role_arns,
            "Adding ECR repository cross-account access"
        );

        // Get current permissions
        let current_permissions = self.get_cross_account_access(repo_id).await?;
        let current_aws_access = match current_permissions.access {
            CrossAccountAccess::Aws(aws_access) => aws_access,
            _ => AwsCrossAccountAccess {
                account_ids: Vec::new(),
                allowed_service_types: Vec::new(),
                role_arns: Vec::new(),
            },
        };

        // Merge new permissions with existing ones
        let mut merged_account_ids = current_aws_access.account_ids;
        let mut merged_service_types = current_aws_access.allowed_service_types;
        let mut merged_role_arns = current_aws_access.role_arns;

        // Add new account IDs
        for account_id in aws_access.account_ids {
            if !merged_account_ids.contains(&account_id) {
                merged_account_ids.push(account_id);
            }
        }

        // Add new service types
        for service_type in aws_access.allowed_service_types {
            if !merged_service_types.contains(&service_type) {
                merged_service_types.push(service_type);
            }
        }

        // Add new role ARNs
        for role_arn in aws_access.role_arns {
            if !merged_role_arns.contains(&role_arn) {
                merged_role_arns.push(role_arn);
            }
        }

        // Build the combined access configuration
        let merged_access = AwsCrossAccountAccess {
            account_ids: merged_account_ids,
            allowed_service_types: merged_service_types,
            role_arns: merged_role_arns,
        };

        self.set_full_policy(&full_repo_name, &merged_access).await
    }

    async fn remove_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<()> {
        let full_repo_name = self.make_full_repo_name(repo_id);

        let aws_access = match access {
            CrossAccountAccess::Aws(aws_access) => aws_access,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: self.binding_name.clone(),
                    reason: "AWS artifact registry can only accept AWS cross-account access configuration".to_string(),
                }));
            }
        };

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            account_ids = ?aws_access.account_ids,
            allowed_service_types = ?aws_access.allowed_service_types,
            role_arns = ?aws_access.role_arns,
            "Removing ECR repository cross-account access"
        );

        // Get current permissions
        let current_permissions = self.get_cross_account_access(repo_id).await?;
        let current_aws_access = match current_permissions.access {
            CrossAccountAccess::Aws(aws_access) => aws_access,
            _ => {
                // No existing permissions to remove from
                info!(repo_id = %repo_id, full_repo_name = %full_repo_name, "No existing AWS cross-account permissions to remove");
                return Ok(());
            }
        };

        // Remove specified permissions from existing ones
        let mut filtered_account_ids = current_aws_access.account_ids;
        let mut filtered_service_types = current_aws_access.allowed_service_types;
        let mut filtered_role_arns = current_aws_access.role_arns;

        // Remove account IDs
        filtered_account_ids.retain(|id| !aws_access.account_ids.contains(id));

        // Remove service types
        filtered_service_types
            .retain(|service_type| !aws_access.allowed_service_types.contains(service_type));

        // Remove role ARNs
        filtered_role_arns.retain(|arn| !aws_access.role_arns.contains(arn));

        // Build the filtered access configuration
        let filtered_access = AwsCrossAccountAccess {
            account_ids: filtered_account_ids,
            allowed_service_types: filtered_service_types,
            role_arns: filtered_role_arns,
        };

        self.set_full_policy(&full_repo_name, &filtered_access)
            .await
    }

    async fn get_cross_account_access(&self, repo_id: &str) -> Result<CrossAccountPermissions> {
        let full_repo_name = self.make_full_repo_name(repo_id);

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            "Getting ECR repository cross-account access"
        );

        let request = GetRepositoryPolicyRequest::builder()
            .repository_name(full_repo_name.clone())
            .build();

        let response = self
            .ecr_client
            .get_repository_policy(request)
            .await
            .map_err(|e| {
                warn!(
                    repo_id = %repo_id,
                    full_repo_name = %full_repo_name,
                    error = %e,
                    "Failed to get ECR repository policy (repository may not have a policy)"
                );
                e
            });

        let response = match response {
            Ok(response) => response,
            Err(_) => {
                // If no policy exists, return empty permissions
                return Ok(CrossAccountPermissions {
                    access: CrossAccountAccess::Aws(AwsCrossAccountAccess {
                        account_ids: Vec::new(),
                        allowed_service_types: Vec::new(),
                        role_arns: Vec::new(),
                    }),
                    last_updated: None,
                });
            }
        };

        // Parse the policy JSON to extract role ARNs, account IDs, and resource types
        let policy: Value = serde_json::from_str(&response.policy_text)
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "aws".to_string(),
                binding_name: "artifact_registry".to_string(),
                field: "policy_text".to_string(),
                response_json: response.policy_text.clone(),
            })?;

        let mut account_ids = Vec::new();
        let mut role_arns = Vec::new();
        let mut allowed_service_types = Vec::new();

        if let Some(statements) = policy["Statement"].as_array() {
            for statement in statements {
                // Check for cross-account role permissions
                if statement["Sid"] == "CrossAccountRolePermission" {
                    if let Some(principals) = statement["Principal"]["AWS"].as_array() {
                        for principal in principals {
                            if let Some(principal_str) = principal.as_str() {
                                role_arns.push(principal_str.to_string());
                                // Extract account ID from role ARN: arn:aws:iam::ACCOUNT_ID:role/RoleName
                                if let Some(account_id) = principal_str.split(':').nth(4) {
                                    account_ids.push(account_id.to_string());
                                }
                            }
                        }
                    } else if let Some(principal) = statement["Principal"]["AWS"].as_str() {
                        role_arns.push(principal.to_string());
                        if let Some(account_id) = principal.split(':').nth(4) {
                            account_ids.push(account_id.to_string());
                        }
                    }
                }

                // Check for Lambda service access
                if statement["Sid"] == "LambdaServiceAccess" {
                    if statement["Principal"]["Service"] == "lambda.amazonaws.com" {
                        allowed_service_types.push(ComputeServiceType::Function);
                    }
                }
            }
        }

        // Remove duplicates
        account_ids.sort();
        account_ids.dedup();
        role_arns.sort();
        role_arns.dedup();
        allowed_service_types.sort_by_key(|rt| format!("{:?}", rt));
        allowed_service_types.dedup();

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            account_ids = ?account_ids,
            role_arns = ?role_arns,
            allowed_service_types = ?allowed_service_types,
            "Retrieved ECR repository cross-account access"
        );

        Ok(CrossAccountPermissions {
            access: CrossAccountAccess::Aws(AwsCrossAccountAccess {
                account_ids,
                allowed_service_types,
                role_arns,
            }),
            last_updated: None, // ECR doesn't provide policy modification time
        })
    }

    async fn generate_credentials(
        &self,
        repo_id: &str,
        permissions: ArtifactRegistryPermissions,
        ttl_seconds: Option<u32>,
    ) -> Result<ArtifactRegistryCredentials> {
        info!(
            repo_id = %repo_id,
            permissions = ?permissions,
            ttl_seconds = ?ttl_seconds,
            "Generating ECR credentials by assuming role"
        );

        // Get the role ARN from stored fields
        let role_arn = match permissions {
            ArtifactRegistryPermissions::Pull => {
                self.pull_role_arn.as_ref().ok_or_else(|| AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: "artifact_registry".to_string(),
                    reason: "Pull role ARN not available - ensure the artifact registry resource is properly linked".to_string(),
                }))?
            }
            ArtifactRegistryPermissions::PushPull => {
                self.push_role_arn.as_ref().ok_or_else(|| AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: "artifact_registry".to_string(),
                    reason: "Push role ARN not available - ensure the artifact registry resource is properly linked".to_string(),
                }))?
            }
        };

        info!(
            role_arn = %role_arn,
            "Using stored role ARN for ECR access"
        );

        let impersonation_config = alien_aws_clients::AwsImpersonationConfig {
            role_arn: role_arn.clone(),
            session_name: Some(format!(
                "alien-ecr-access-{}",
                chrono::Utc::now().timestamp()
            )),
            duration_seconds: ttl_seconds.map(|ttl| ttl.min(43200) as i32), // Max 12 hours
            external_id: None,
            target_region: None,
        };

        // Assume the role
        let impersonated_config = self
            .credentials
            .config()
            .impersonate(impersonation_config)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    "Failed to assume ECR access role".to_string(),
                    Some(repo_id.to_string()),
                )
            })?;

        // Create ECR client with impersonated credentials
        let ecr_client = alien_aws_clients::ecr::EcrClient::new(
            crate::http_client::create_http_client(),
            AwsCredentialProvider::from_config(impersonated_config)
                .await
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "artifact_registry.ecr".to_string(),
                    reason: "Failed to create credential provider for impersonated role"
                        .to_string(),
                })?,
        );

        // Get ECR authorization token
        let request = alien_aws_clients::ecr::GetAuthorizationTokenRequest::builder().build();

        let response = ecr_client
            .get_authorization_token(request)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    "Failed to get ECR authorization token with assumed role".to_string(),
                    Some(repo_id.to_string()),
                )
            })?;

        if let Some(auth_data) = response.authorization_data.first() {
            // Decode the base64 authorization token
            let token_bytes = BASE64
                .decode(&auth_data.authorization_token)
                .into_alien_error()
                .context(ErrorData::UnexpectedResponseFormat {
                    provider: "aws".to_string(),
                    binding_name: "artifact_registry".to_string(),
                    field: "authorization_token".to_string(),
                    response_json: auth_data.authorization_token.clone(),
                })?;

            let token_str = String::from_utf8(token_bytes.clone())
                .into_alien_error()
                .context(ErrorData::UnexpectedResponseFormat {
                    provider: "aws".to_string(),
                    binding_name: "artifact_registry".to_string(),
                    field: "authorization_token".to_string(),
                    response_json: format!("{:?}", token_bytes),
                })?;

            // Token format is "username:password"
            if let Some((username, password)) = token_str.split_once(':') {
                let expires_at = if ttl_seconds.is_some() || auth_data.expires_at > 0.0 {
                    DateTime::from_timestamp(auth_data.expires_at as i64, 0)
                        .map(|dt| dt.to_rfc3339())
                } else {
                    None
                };

                info!(
                    permissions = ?permissions,
                    "ECR authorization token generated successfully with assumed role"
                );

                Ok(ArtifactRegistryCredentials {
                    username: username.to_string(),
                    password: password.to_string(),
                    expires_at,
                })
            } else {
                Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "aws".to_string(),
                    binding_name: "artifact_registry".to_string(),
                    field: "authorization_token".to_string(),
                    response_json: token_str.to_string(),
                }))
            }
        } else {
            Err(AlienError::new(ErrorData::CloudPlatformError {
                message: "ECR authorization response did not contain authorization data"
                    .to_string(),
                resource_id: Some(repo_id.to_string()),
            }))
        }
    }

    async fn delete_repository(&self, repo_id: &str) -> Result<()> {
        let full_repo_name = self.make_full_repo_name(repo_id);

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            "Deleting ECR repository"
        );

        let request = alien_aws_clients::ecr::DeleteRepositoryRequest::builder()
            .repository_name(full_repo_name.clone())
            .force(true)
            .build();

        self.ecr_client
            .delete_repository(request)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to delete ECR repository '{}'", full_repo_name),
                    Some(repo_id.to_string()),
                )
            })?;

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            "ECR repository deleted successfully"
        );
        Ok(())
    }
}
