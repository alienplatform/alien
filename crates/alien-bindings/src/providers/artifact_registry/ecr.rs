use crate::{
    error::{map_cloud_client_error, Error, ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, RegistryAuthMethod,
        AwsCrossAccountAccess, Binding, ComputeServiceType, CrossAccountAccess,
        CrossAccountPermissions, RepositoryResponse,
    },
};
use alien_aws_clients::{
    ecr::{
        CreateRepositoryRequest, DescribeRepositoriesRequest, EcrApi, EcrClient,
        GetAuthorizationTokenRequest, GetRepositoryPolicyRequest,
        SetRepositoryPolicyRequest,
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
            .map(|v| {
                v.into_value(&binding_name, "pull_role_arn").context(
                    ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.clone(),
                        reason: "Failed to extract pull_role_arn from binding".to_string(),
                    },
                )
            })
            .transpose()?;

        let push_role_arn = config
            .push_role_arn
            .map(|v| {
                v.into_value(&binding_name, "push_role_arn").context(
                    ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.clone(),
                        reason: "Failed to extract push_role_arn from binding".to_string(),
                    },
                )
            })
            .transpose()?;

        Ok(Self {
            credentials: credentials.clone(),
            ecr_client,
            binding_name,
            repository_prefix,
            pull_role_arn,
            push_role_arn,
        })
    }

    /// Constructs the full repository name for ECR using the repository prefix.
    /// If `repo_name` is empty, returns just the prefix (shared-repo pattern).
    /// Uses `-` separator to match IAM policy wildcards (e.g., `alien-artifacts-prj_xxx`).
    fn make_full_repo_name(&self, repo_name: &str) -> String {
        if repo_name.is_empty() {
            self.repository_prefix.clone()
        } else if !self.repository_prefix.is_empty() {
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
        self.set_full_policy_with_client(&self.ecr_client, repo_name, aws_access)
            .await
    }

    async fn set_full_policy_with_client(
        &self,
        ecr_client: &EcrClient,
        repo_name: &str,
        aws_access: &AwsCrossAccountAccess,
    ) -> Result<()> {
        let mut statements = Vec::new();

        // Add cross-account access for target accounts + specific role ARNs.
        // Per AWS docs, Lambda cross-account ECR pulls require the account root
        // as a principal (arn:aws:iam::{account}:root), not just specific roles.
        // See: https://github.com/aws-samples/lambda-cross-account-ecr
        {
            let mut principals: Vec<String> = aws_access
                .account_ids
                .iter()
                .map(|id| format!("arn:aws:iam::{}:root", id))
                .collect();
            for arn in &aws_access.role_arns {
                if !principals.contains(arn) {
                    principals.push(arn.clone());
                }
            }
            if !principals.is_empty() {
                statements.push(json!({
                    "Sid": "CrossAccountRolePermission",
                    "Effect": "Allow",
                    "Principal": {
                        "AWS": principals
                    },
                    "Action": [
                        "ecr:BatchCheckLayerAvailability",
                        "ecr:GetDownloadUrlForLayer",
                        "ecr:BatchGetImage",
                        // Required for Lambda CreateFunction: Lambda internally
                        // verifies/sets the ECR repo policy when creating a
                        // function with a cross-account image. The calling
                        // principal needs these permissions on the ECR repo.
                        "ecr:GetRepositoryPolicy",
                        "ecr:SetRepositoryPolicy"
                    ]
                }));
            }
        }

        // Add service-specific access based on compute service types
        for service_type in &aws_access.allowed_service_types {
            match service_type {
                ComputeServiceType::Function => {
                    if !aws_access.account_ids.is_empty() {
                        // Build sourceArn patterns per AWS docs:
                        // https://docs.aws.amazon.com/lambda/latest/dg/images-create.html
                        // Pattern: arn:aws:lambda:{region}:{account_id}:function:*
                        let source_arns: Vec<String> = aws_access
                            .account_ids
                            .iter()
                            .flat_map(|account_id| {
                                if aws_access.regions.is_empty() {
                                    vec![format!("arn:aws:lambda:*:{}:function:*", account_id)]
                                } else {
                                    aws_access
                                        .regions
                                        .iter()
                                        .map(|region| {
                                            format!(
                                                "arn:aws:lambda:{}:{}:function:*",
                                                region, account_id
                                            )
                                        })
                                        .collect()
                                }
                            })
                            .collect();

                        statements.push(json!({
                            "Sid": "LambdaECRImageCrossAccountRetrievalPolicy",
                            "Effect": "Allow",
                            "Principal": {
                                "Service": "lambda.amazonaws.com"
                            },
                            "Action": [
                                "ecr:BatchGetImage",
                                "ecr:GetDownloadUrlForLayer"
                            ],
                            "Condition": {
                                "StringLike": {
                                    "aws:sourceArn": source_arns
                                }
                            }
                        }));
                    }
                }
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

        ecr_client
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
    fn registry_endpoint(&self) -> String {
        format!(
            "https://{}.dkr.ecr.{}.amazonaws.com",
            self.credentials.account_id(),
            self.credentials.region(),
        )
    }

    fn upstream_repository_prefix(&self) -> String {
        self.repository_prefix.clone()
    }

    async fn create_repository(&self, repo_name: &str) -> Result<RepositoryResponse> {
        let full_repo_name = self.make_full_repo_name(repo_name);

        info!(
            repo_name = %repo_name,
            full_repo_name = %full_repo_name,
            "Creating ECR repository"
        );

        // Use push role for cross-account, or direct credentials for single-account.
        let ecr_config = if let Some(push_role_arn) = &self.push_role_arn {
            self.credentials
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
                })?
        } else {
            self.credentials.config().clone()
        };
        let ecr_client = alien_aws_clients::ecr::EcrClient::new(
            crate::http_client::create_http_client(),
            AwsCredentialProvider::from_config(ecr_config)
                .await
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "artifact_registry.ecr".to_string(),
                    reason: "Failed to create credential provider for ECR access".to_string(),
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
            name: full_repo_name,
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
                regions: Vec::new(),
                allowed_service_types: Vec::new(),
                role_arns: Vec::new(),
            },
        };

        // Merge new permissions with existing ones
        let mut merged_account_ids = current_aws_access.account_ids;
        let mut merged_regions = current_aws_access.regions;
        let mut merged_service_types = current_aws_access.allowed_service_types;
        let mut merged_role_arns = current_aws_access.role_arns;

        for account_id in aws_access.account_ids {
            if !merged_account_ids.contains(&account_id) {
                merged_account_ids.push(account_id);
            }
        }

        for region in aws_access.regions {
            if !merged_regions.contains(&region) {
                merged_regions.push(region);
            }
        }

        for service_type in aws_access.allowed_service_types {
            if !merged_service_types.contains(&service_type) {
                merged_service_types.push(service_type);
            }
        }

        for role_arn in aws_access.role_arns {
            if !merged_role_arns.contains(&role_arn) {
                merged_role_arns.push(role_arn);
            }
        }

        let merged_access = AwsCrossAccountAccess {
            account_ids: merged_account_ids,
            regions: merged_regions.clone(),
            allowed_service_types: merged_service_types,
            role_arns: merged_role_arns,
        };

        // Set policy on the source region's repo (where images are pushed).
        self.set_full_policy(&full_repo_name, &merged_access)
            .await?;

        // Also set the policy on replicated repos in target regions.
        // ECR replication copies images cross-region but NOT repo policies.
        // Lambda in us-east-2 pulls from the us-east-2 replica, which needs
        // its own cross-account policy.
        let source_region = self.credentials.region().to_string();
        for region in &merged_access.regions {
            if *region == source_region {
                continue; // Already set on source region above.
            }
            match self.credentials.with_region(region).await {
                Ok(target_creds) => {
                    let http_client = crate::http_client::create_http_client();
                    let target_ecr = EcrClient::new(http_client, target_creds);
                    match self
                        .set_full_policy_with_client(&target_ecr, &full_repo_name, &merged_access)
                        .await
                    {
                        Ok(()) => {
                            info!(
                                repo_name = %full_repo_name,
                                region = %region,
                                "ECR cross-account policy set on replicated repo"
                            );
                        }
                        Err(e) => {
                            warn!(
                                repo_name = %full_repo_name,
                                region = %region,
                                error = %e,
                                "Failed to set ECR policy on replicated repo (may not exist yet)"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        region = %region,
                        error = %e,
                        "Failed to create credentials for target region"
                    );
                }
            }
        }

        Ok(())
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

        let mut filtered_account_ids = current_aws_access.account_ids;
        let mut filtered_regions = current_aws_access.regions;
        let mut filtered_service_types = current_aws_access.allowed_service_types;
        let mut filtered_role_arns = current_aws_access.role_arns;

        filtered_account_ids.retain(|id| !aws_access.account_ids.contains(id));
        filtered_regions.retain(|r| !aws_access.regions.contains(r));
        filtered_service_types
            .retain(|service_type| !aws_access.allowed_service_types.contains(service_type));
        filtered_role_arns.retain(|arn| !aws_access.role_arns.contains(arn));

        let filtered_access = AwsCrossAccountAccess {
            account_ids: filtered_account_ids,
            regions: filtered_regions,
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
                return Ok(CrossAccountPermissions {
                    access: CrossAccountAccess::Aws(AwsCrossAccountAccess {
                        account_ids: Vec::new(),
                        regions: Vec::new(),
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
                                // AWS replaces deleted role ARNs with role unique IDs (e.g. "AROA...")
                                // in existing policies. Filter these out to avoid "Principal not found"
                                // errors when rewriting the policy.
                                if !principal_str.starts_with("arn:") {
                                    warn!(
                                        principal = %principal_str,
                                        "Skipping stale principal in ECR policy (deleted role replaced by unique ID)"
                                    );
                                    continue;
                                }
                                role_arns.push(principal_str.to_string());
                                // Extract account ID from role ARN: arn:aws:iam::ACCOUNT_ID:role/RoleName
                                if let Some(account_id) = principal_str.split(':').nth(4) {
                                    account_ids.push(account_id.to_string());
                                }
                            }
                        }
                    } else if let Some(principal) = statement["Principal"]["AWS"].as_str() {
                        if !principal.starts_with("arn:") {
                            warn!(
                                principal = %principal,
                                "Skipping stale principal in ECR policy (deleted role replaced by unique ID)"
                            );
                        } else {
                            role_arns.push(principal.to_string());
                            if let Some(account_id) = principal.split(':').nth(4) {
                                account_ids.push(account_id.to_string());
                            }
                        }
                    }
                }

                // Check for Lambda service access (both old and new Sid names)
                if statement["Sid"] == "LambdaECRImageCrossAccountRetrievalPolicy"
                    || statement["Sid"] == "LambdaServiceAccess"
                {
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
                regions: Vec::new(),
                allowed_service_types,
                role_arns,
            }),
            last_updated: None,
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

        // Get the role ARN (optional for single-account deployments)
        let role_arn = match permissions {
            ArtifactRegistryPermissions::Pull => self.pull_role_arn.as_ref(),
            ArtifactRegistryPermissions::PushPull => self.push_role_arn.as_ref(),
        };

        // When a role ARN is configured, assume it for cross-account access.
        // When no role is configured (single-account), use base credentials directly.
        let ecr_config = if let Some(role_arn) = role_arn {
            info!(role_arn = %role_arn, "Assuming role for ECR access");
            self.credentials
                .config()
                .impersonate(alien_aws_clients::AwsImpersonationConfig {
                    role_arn: role_arn.clone(),
                    session_name: Some(format!(
                        "alien-ecr-access-{}",
                        chrono::Utc::now().timestamp()
                    )),
                    duration_seconds: ttl_seconds.map(|ttl| ttl.min(43200) as i32),
                    external_id: None,
                    target_region: None,
                })
                .await
                .map_err(|e| {
                    map_cloud_client_error(
                        e,
                        "Failed to assume ECR access role".to_string(),
                        Some(repo_id.to_string()),
                    )
                })?
        } else {
            info!("Using direct credentials for ECR access (no role configured)");
            self.credentials.config().clone()
        };

        // Create ECR client with resolved credentials
        let ecr_client = alien_aws_clients::ecr::EcrClient::new(
            crate::http_client::create_http_client(),
            AwsCredentialProvider::from_config(ecr_config)
                .await
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "artifact_registry.ecr".to_string(),
                    reason: "Failed to create credential provider for ECR access".to_string(),
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
                    auth_method: RegistryAuthMethod::Basic,
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

        // Use push role for cross-account, or direct credentials for single-account.
        let ecr_config = if let Some(push_role_arn) = &self.push_role_arn {
            self.credentials
                .config()
                .impersonate(alien_aws_clients::AwsImpersonationConfig {
                    role_arn: push_role_arn.clone(),
                    session_name: Some("alien-ecr-delete".to_string()),
                    duration_seconds: None,
                    external_id: None,
                    target_region: None,
                })
                .await
                .map_err(|e| {
                    map_cloud_client_error(
                        e,
                        "Failed to assume ECR push role".to_string(),
                        Some(repo_id.to_string()),
                    )
                })?
        } else {
            self.credentials.config().clone()
        };
        let ecr_client = alien_aws_clients::ecr::EcrClient::new(
            crate::http_client::create_http_client(),
            AwsCredentialProvider::from_config(ecr_config)
                .await
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "artifact_registry.ecr".to_string(),
                    reason: "Failed to create credential provider for ECR access".to_string(),
                })?,
        );

        let request = alien_aws_clients::ecr::DeleteRepositoryRequest::builder()
            .repository_name(full_repo_name.clone())
            .force(true)
            .build();

        ecr_client.delete_repository(request).await.map_err(|e| {
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
