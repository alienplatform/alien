use crate::{
    error::{ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions,
        AwsCrossAccountAccess, Binding, ComputeServiceType, CrossAccountAccess,
        CrossAccountPermissions, RegistryAuthMethod, RepositoryResponse,
    },
};
use alien_core::{bindings::ArtifactRegistryBinding, AwsClientConfig, AwsImpersonationConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use aws_sdk_ecr::{error::ProvideErrorMetadata, primitives::DateTimeFormat};
use base64::engine::{general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::{json, Value};
use std::{fmt::Debug, sync::Arc};
use tokio::time::{sleep, Duration, Instant};
use tracing::{info, warn};

/// ECR repository details used by the artifact registry binding.
#[derive(Debug, Clone)]
pub struct EcrRepository {
    /// Repository name.
    pub name: String,
    /// Repository URI.
    pub uri: Option<String>,
    /// Repository creation timestamp.
    pub created_at: Option<String>,
}

/// ECR authorization token data.
#[derive(Debug, Clone)]
pub struct EcrAuthorizationData {
    /// Base64 encoded authorization token.
    pub authorization_token: String,
    /// Optional expiration timestamp.
    pub expires_at: Option<String>,
}

/// Result of creating an ECR repository.
#[derive(Debug, Clone)]
pub enum CreateEcrRepositoryResult {
    /// Repository was created.
    Created(EcrRepository),
    /// Repository already exists.
    AlreadyExists,
}

/// Minimal ECR operations required by the artifact registry binding.
#[async_trait]
pub trait EcrClient: Debug + Send + Sync {
    /// Create a repository.
    async fn create_repository(&self, repository_name: &str) -> Result<CreateEcrRepositoryResult>;

    /// Describe one repository. Returns None when it is not found.
    async fn describe_repository(&self, repository_name: &str) -> Result<Option<EcrRepository>>;

    /// Set a repository policy document.
    async fn set_repository_policy(&self, repository_name: &str, policy_text: String)
        -> Result<()>;

    /// Get a repository policy document.
    async fn get_repository_policy(&self, repository_name: &str) -> Result<Option<String>>;

    /// Get an ECR authorization token.
    async fn get_authorization_token(&self) -> Result<Option<EcrAuthorizationData>>;

    /// Delete a repository.
    async fn delete_repository(&self, repository_name: &str, force: bool) -> Result<()>;
}

#[async_trait]
impl EcrClient for aws_sdk_ecr::Client {
    async fn create_repository(&self, repository_name: &str) -> Result<CreateEcrRepositoryResult> {
        match self
            .create_repository()
            .repository_name(repository_name)
            .send()
            .await
        {
            Ok(response) => {
                let repository = response.repository().ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "CreateRepository response did not include repository '{}'",
                            repository_name
                        ),
                        resource_id: Some(repository_name.to_string()),
                    })
                })?;
                Ok(CreateEcrRepositoryResult::Created(repository_from_sdk(
                    repository,
                    repository_name,
                )?))
            }
            Err(error)
                if error
                    .as_service_error()
                    .map(|error| error.is_repository_already_exists_exception())
                    .unwrap_or(false) =>
            {
                Ok(CreateEcrRepositoryResult::AlreadyExists)
            }
            Err(error) => Err(error)
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create ECR repository '{}'", repository_name),
                    resource_id: Some(repository_name.to_string()),
                }),
        }
    }

    async fn describe_repository(&self, repository_name: &str) -> Result<Option<EcrRepository>> {
        match self
            .describe_repositories()
            .repository_names(repository_name)
            .send()
            .await
        {
            Ok(response) => response
                .repositories()
                .iter()
                .find(|repository| repository.repository_name() == Some(repository_name))
                .map(|repository| repository_from_sdk(repository, repository_name))
                .transpose(),
            Err(error)
                if error
                    .as_service_error()
                    .map(|error| error.is_repository_not_found_exception())
                    .unwrap_or(false) =>
            {
                Ok(None)
            }
            Err(error) => Err(error)
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to describe ECR repository '{}'", repository_name),
                    resource_id: Some(repository_name.to_string()),
                }),
        }
    }

    async fn set_repository_policy(
        &self,
        repository_name: &str,
        policy_text: String,
    ) -> Result<()> {
        self.set_repository_policy()
            .repository_name(repository_name)
            .policy_text(policy_text)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to set policy for ECR repository '{}'",
                    repository_name
                ),
                resource_id: Some(repository_name.to_string()),
            })?;

        Ok(())
    }

    async fn get_repository_policy(&self, repository_name: &str) -> Result<Option<String>> {
        match self
            .get_repository_policy()
            .repository_name(repository_name)
            .send()
            .await
        {
            Ok(response) => Ok(response.policy_text().map(ToString::to_string)),
            Err(error)
                if error
                    .as_service_error()
                    .map(|error| {
                        error.is_repository_not_found_exception()
                            || error.is_repository_policy_not_found_exception()
                    })
                    .unwrap_or(false) =>
            {
                Ok(None)
            }
            Err(error) => {
                warn!(
                    repository_name = %repository_name,
                    error_code = ?error.code(),
                    error = %error,
                    "Failed to get ECR repository policy"
                );
                Ok(None)
            }
        }
    }

    async fn get_authorization_token(&self) -> Result<Option<EcrAuthorizationData>> {
        let response = self
            .get_authorization_token()
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get ECR authorization token".to_string(),
                resource_id: None,
            })?;

        response
            .authorization_data()
            .first()
            .map(|auth_data| {
                let authorization_token = auth_data.authorization_token().ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: "ECR authorization response did not include a token".to_string(),
                        resource_id: None,
                    })
                })?;
                let expires_at = auth_data
                    .expires_at()
                    .map(|time| time.fmt(DateTimeFormat::DateTime))
                    .transpose()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to format ECR authorization token expiration".to_string(),
                        resource_id: None,
                    })?;

                Ok(EcrAuthorizationData {
                    authorization_token: authorization_token.to_string(),
                    expires_at,
                })
            })
            .transpose()
    }

    async fn delete_repository(&self, repository_name: &str, force: bool) -> Result<()> {
        self.delete_repository()
            .repository_name(repository_name)
            .force(force)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete ECR repository '{}'", repository_name),
                resource_id: Some(repository_name.to_string()),
            })?;

        Ok(())
    }
}

fn repository_from_sdk(
    repository: &aws_sdk_ecr::types::Repository,
    fallback_name: &str,
) -> Result<EcrRepository> {
    let created_at = repository
        .created_at()
        .map(|time| time.fmt(DateTimeFormat::DateTime))
        .transpose()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to format ECR repository '{}' creation timestamp",
                fallback_name
            ),
            resource_id: Some(fallback_name.to_string()),
        })?;

    Ok(EcrRepository {
        name: repository
            .repository_name()
            .unwrap_or(fallback_name)
            .to_string(),
        uri: repository.repository_uri().map(ToString::to_string),
        created_at,
    })
}

/// AWS ECR implementation of the ArtifactRegistry binding.
#[derive(Debug)]
pub struct EcrArtifactRegistry {
    base_config: AwsClientConfig,
    ecr_client: Arc<dyn EcrClient>,
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
        config: AwsClientConfig,
    ) -> Result<Self> {
        info!(
            binding_name = %binding_name,
            "Initializing AWS ECR artifact registry"
        );

        let ecr_client = Arc::new(crate::aws_sdk::ecr_client_from_alien_config(&config).await?);

        let config_values = match binding {
            ArtifactRegistryBinding::Ecr(config) => config,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Expected ECR binding, got different service type".to_string(),
                }));
            }
        };

        let repository_prefix = config_values
            .repository_prefix
            .into_value(&binding_name, "repository_prefix")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract repository_prefix from binding".to_string(),
            })?;

        let pull_role_arn = config_values
            .pull_role_arn
            .map(|value| {
                value.into_value(&binding_name, "pull_role_arn").context(
                    ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.clone(),
                        reason: "Failed to extract pull_role_arn from binding".to_string(),
                    },
                )
            })
            .transpose()?;

        let push_role_arn = config_values
            .push_role_arn
            .map(|value| {
                value.into_value(&binding_name, "push_role_arn").context(
                    ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.clone(),
                        reason: "Failed to extract push_role_arn from binding".to_string(),
                    },
                )
            })
            .transpose()?;

        Ok(Self {
            base_config: config,
            ecr_client,
            binding_name,
            repository_prefix,
            pull_role_arn,
            push_role_arn,
        })
    }

    fn account_id(&self) -> &str {
        &self.base_config.account_id
    }

    fn region(&self) -> &str {
        &self.base_config.region
    }

    /// Constructs the full repository name for ECR using the repository prefix.
    fn make_full_repo_name(&self, repo_name: &str) -> String {
        if repo_name.is_empty() {
            self.repository_prefix.clone()
        } else if !self.repository_prefix.is_empty() {
            format!("{}-{}", self.repository_prefix, repo_name)
        } else {
            repo_name.to_string()
        }
    }

    fn repository_lookup_names(&self, repo_id: &str) -> Vec<String> {
        let is_prefixed = !self.repository_prefix.is_empty()
            && repo_id.starts_with(&format!("{}-", self.repository_prefix));

        if is_prefixed || self.repository_prefix.is_empty() {
            vec![repo_id.to_string()]
        } else {
            vec![repo_id.to_string(), self.make_full_repo_name(repo_id)]
        }
    }

    fn repository_uri(&self, full_repo_name: &str) -> String {
        format!(
            "{}.dkr.ecr.{}.amazonaws.com/{}",
            self.account_id(),
            self.region(),
            full_repo_name
        )
    }

    async fn ecr_client_for_config(&self, config: &AwsClientConfig) -> Result<Arc<dyn EcrClient>> {
        Ok(Arc::new(
            crate::aws_sdk::ecr_client_from_alien_config(config).await?,
        ))
    }

    async fn config_for_role(
        &self,
        role_arn: &str,
        session_name: &str,
        duration_seconds: Option<i32>,
        target_region: Option<String>,
    ) -> Result<AwsClientConfig> {
        crate::aws_sdk::assume_role_config_from_alien_config(
            &self.base_config,
            AwsImpersonationConfig {
                role_arn: role_arn.to_string(),
                session_name: Some(session_name.to_string()),
                duration_seconds,
                external_id: None,
                target_region,
            },
        )
        .await
        .context(ErrorData::BindingSetupFailed {
            binding_type: "artifact_registry.ecr".to_string(),
            reason: format!("Failed to assume ECR role '{}'", role_arn),
        })
    }

    async fn push_client(&self, session_name: &str) -> Result<Arc<dyn EcrClient>> {
        if let Some(push_role_arn) = &self.push_role_arn {
            let config = self
                .config_for_role(push_role_arn, session_name, None, None)
                .await?;
            self.ecr_client_for_config(&config).await
        } else {
            Ok(Arc::clone(&self.ecr_client))
        }
    }

    async fn pull_client(&self, session_name: &str) -> Result<Arc<dyn EcrClient>> {
        let pull_role_arn = self.pull_role_arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: self.binding_name.clone(),
                reason: "Pull role ARN not available".to_string(),
            })
        })?;
        let config = self
            .config_for_role(pull_role_arn, session_name, None, None)
            .await?;
        self.ecr_client_for_config(&config).await
    }

    async fn access_client(
        &self,
        permissions: &ArtifactRegistryPermissions,
        ttl_seconds: Option<u32>,
    ) -> Result<Arc<dyn EcrClient>> {
        let role_arn = match permissions {
            ArtifactRegistryPermissions::Pull => self.pull_role_arn.as_ref(),
            ArtifactRegistryPermissions::PushPull => self.push_role_arn.as_ref(),
        };

        if let Some(role_arn) = role_arn {
            let config = self
                .config_for_role(
                    role_arn,
                    &format!("alien-ecr-access-{}", chrono::Utc::now().timestamp()),
                    ttl_seconds.map(|ttl| ttl.min(43200) as i32),
                    None,
                )
                .await?;
            self.ecr_client_for_config(&config).await
        } else {
            Ok(Arc::clone(&self.ecr_client))
        }
    }

    async fn regional_client(&self, region: &str) -> Result<Arc<dyn EcrClient>> {
        let mut config = self.base_config.clone();
        config.region = region.to_string();
        self.ecr_client_for_config(&config).await
    }

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
        ecr_client: &Arc<dyn EcrClient>,
        repo_name: &str,
        aws_access: &AwsCrossAccountAccess,
    ) -> Result<()> {
        let mut statements = Vec::new();

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
                "Principal": { "AWS": principals },
                "Action": [
                    "ecr:BatchCheckLayerAvailability",
                    "ecr:GetDownloadUrlForLayer",
                    "ecr:BatchGetImage",
                    "ecr:GetRepositoryPolicy",
                    "ecr:SetRepositoryPolicy"
                ]
            }));
        }

        for service_type in &aws_access.allowed_service_types {
            match service_type {
                ComputeServiceType::Worker => {
                    if !aws_access.account_ids.is_empty() {
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
                            "Principal": { "Service": "lambda.amazonaws.com" },
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

        let policy = json!({
            "Version": "2012-10-17",
            "Statement": statements
        });

        ecr_client
            .set_repository_policy(repo_name, policy.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to set cross-account access for ECR repository '{}'",
                    repo_name
                ),
                resource_id: Some(repo_name.to_string()),
            })?;

        info!(
            repo_name = %repo_name,
            "ECR repository cross-account access policy updated successfully"
        );
        Ok(())
    }

    async fn wait_for_repository_with_client(
        &self,
        ecr_client: &Arc<dyn EcrClient>,
        repo_name: &str,
        region: &str,
    ) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(300);

        loop {
            let current_status = match ecr_client.describe_repository(repo_name).await {
                Ok(Some(_)) => {
                    info!(
                        repo_name = %repo_name,
                        region = %region,
                        "Replicated ECR repository is ready"
                    );
                    return Ok(());
                }
                Ok(None) => {
                    "DescribeRepositories response did not include the repository".to_string()
                }
                Err(error) => error.to_string(),
            };

            if Instant::now() >= deadline {
                return Err(AlienError::new(ErrorData::Timeout {
                    operation_context: format!(
                        "Waiting for replicated ECR repository '{}' in {}",
                        repo_name, region
                    ),
                    details: format!(
                        "ECR did not make the replicated repository available within 300s; last status: {}",
                        current_status
                    ),
                }));
            }

            sleep(Duration::from_secs(5)).await;
        }
    }
}

impl Binding for EcrArtifactRegistry {}

#[async_trait]
impl ArtifactRegistry for EcrArtifactRegistry {
    fn registry_endpoint(&self) -> String {
        format!(
            "https://{}.dkr.ecr.{}.amazonaws.com",
            self.account_id(),
            self.region(),
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

        let ecr_client = self.push_client("alien-ecr-create").await?;
        match ecr_client.create_repository(&full_repo_name).await? {
            CreateEcrRepositoryResult::AlreadyExists => {
                info!(
                    repo_name = %repo_name,
                    full_repo_name = %full_repo_name,
                    "ECR repository already exists"
                );
                Ok(RepositoryResponse {
                    name: full_repo_name.clone(),
                    uri: Some(self.repository_uri(&full_repo_name)),
                    created_at: None,
                })
            }
            CreateEcrRepositoryResult::Created(repository) => {
                info!(
                    repo_name = %repo_name,
                    full_repo_name = %full_repo_name,
                    "ECR repository created successfully"
                );

                Ok(RepositoryResponse {
                    name: full_repo_name,
                    uri: repository.uri,
                    created_at: repository.created_at,
                })
            }
        }
    }

    async fn get_repository(&self, repo_id: &str) -> Result<RepositoryResponse> {
        let lookup_names = self.repository_lookup_names(repo_id);

        info!(
            repo_id = %repo_id,
            lookup_names = ?lookup_names,
            "Getting ECR repository details"
        );

        let ecr_client = self.pull_client("alien-ecr-describe").await?;
        for full_repo_name in &lookup_names {
            if let Some(repository) = ecr_client.describe_repository(full_repo_name).await? {
                info!(
                    repo_id = %repo_id,
                    full_repo_name = %full_repo_name,
                    repo_uri = ?repository.uri,
                    "ECR repository details retrieved"
                );

                return Ok(RepositoryResponse {
                    name: repository.name,
                    uri: repository.uri,
                    created_at: repository.created_at,
                });
            }
        }

        warn!(
            repo_id = %repo_id,
            lookup_names = ?lookup_names,
            "ECR repository not found"
        );

        Err(AlienError::new(ErrorData::ResourceNotFound {
            resource_id: repo_id.to_string(),
        }))
    }

    async fn add_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<()> {
        let full_repo_name = repo_id.to_string();

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

        self.set_full_policy(&full_repo_name, &merged_access)
            .await?;

        let source_region = self.region().to_string();
        for region in &merged_access.regions {
            if *region == source_region {
                continue;
            }

            let target_ecr = self.regional_client(region).await?;
            self.wait_for_repository_with_client(&target_ecr, &full_repo_name, region)
                .await?;
            self.set_full_policy_with_client(&target_ecr, &full_repo_name, &merged_access)
                .await?;

            info!(
                repo_name = %full_repo_name,
                region = %region,
                "ECR cross-account policy set on replicated repo"
            );
        }

        Ok(())
    }

    async fn remove_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<()> {
        let full_repo_name = repo_id.to_string();

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

        let current_permissions = self.get_cross_account_access(repo_id).await?;
        let current_aws_access = match current_permissions.access {
            CrossAccountAccess::Aws(aws_access) => aws_access,
            _ => {
                info!(repo_id = %repo_id, full_repo_name = %full_repo_name, "No existing AWS cross-account permissions to remove");
                return Ok(());
            }
        };

        let mut filtered_account_ids = current_aws_access.account_ids;
        let mut filtered_regions = current_aws_access.regions;
        let mut filtered_service_types = current_aws_access.allowed_service_types;
        let mut filtered_role_arns = current_aws_access.role_arns;

        filtered_account_ids.retain(|id| !aws_access.account_ids.contains(id));
        filtered_regions.retain(|region| !aws_access.regions.contains(region));
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
        let full_repo_name = repo_id.to_string();

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            "Getting ECR repository cross-account access"
        );

        let Some(policy_text) = self
            .ecr_client
            .get_repository_policy(&full_repo_name)
            .await?
        else {
            return Ok(CrossAccountPermissions {
                access: CrossAccountAccess::Aws(AwsCrossAccountAccess {
                    account_ids: Vec::new(),
                    regions: Vec::new(),
                    allowed_service_types: Vec::new(),
                    role_arns: Vec::new(),
                }),
                last_updated: None,
            });
        };

        let policy: Value = serde_json::from_str(&policy_text)
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "aws".to_string(),
                binding_name: "artifact_registry".to_string(),
                field: "policy_text".to_string(),
                response_json: policy_text.clone(),
            })?;

        let mut account_ids = Vec::new();
        let mut role_arns = Vec::new();
        let mut allowed_service_types = Vec::new();

        if let Some(statements) = policy["Statement"].as_array() {
            for statement in statements {
                if statement["Sid"] == "CrossAccountRolePermission" {
                    if let Some(principals) = statement["Principal"]["AWS"].as_array() {
                        for principal in principals {
                            if let Some(principal_str) = principal.as_str() {
                                collect_principal(principal_str, &mut account_ids, &mut role_arns);
                            }
                        }
                    } else if let Some(principal) = statement["Principal"]["AWS"].as_str() {
                        collect_principal(principal, &mut account_ids, &mut role_arns);
                    }
                }

                if (statement["Sid"] == "LambdaECRImageCrossAccountRetrievalPolicy"
                    || statement["Sid"] == "LambdaServiceAccess")
                    && statement["Principal"]["Service"] == "lambda.amazonaws.com"
                {
                    allowed_service_types.push(ComputeServiceType::Worker);
                }
            }
        }

        account_ids.sort();
        account_ids.dedup();
        role_arns.sort();
        role_arns.dedup();
        allowed_service_types.sort_by_key(|service_type| format!("{:?}", service_type));
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
            "Generating ECR credentials"
        );

        let ecr_client = self.access_client(&permissions, ttl_seconds).await?;
        let auth_data = ecr_client.get_authorization_token().await?.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "ECR authorization response did not contain authorization data"
                    .to_string(),
                resource_id: Some(repo_id.to_string()),
            })
        })?;

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

        let Some((username, password)) = token_str.split_once(':') else {
            return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "aws".to_string(),
                binding_name: "artifact_registry".to_string(),
                field: "authorization_token".to_string(),
                response_json: token_str,
            }));
        };

        info!(
            permissions = ?permissions,
            "ECR authorization token generated successfully"
        );

        Ok(ArtifactRegistryCredentials {
            auth_method: RegistryAuthMethod::Basic,
            username: username.to_string(),
            password: password.to_string(),
            expires_at: auth_data.expires_at,
        })
    }

    async fn delete_repository(&self, repo_id: &str) -> Result<()> {
        let full_repo_name = repo_id.to_string();

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            "Deleting ECR repository"
        );

        let ecr_client = self.push_client("alien-ecr-delete").await?;
        ecr_client.delete_repository(&full_repo_name, true).await?;

        info!(
            repo_id = %repo_id,
            full_repo_name = %full_repo_name,
            "ECR repository deleted successfully"
        );
        Ok(())
    }
}

fn collect_principal(principal: &str, account_ids: &mut Vec<String>, role_arns: &mut Vec<String>) {
    if !principal.starts_with("arn:") {
        warn!(
            principal = %principal,
            "Skipping stale principal in ECR policy (deleted role replaced by unique ID)"
        );
        return;
    }

    role_arns.push(principal.to_string());
    if let Some(account_id) = principal.split(':').nth(4) {
        account_ids.push(account_id.to_string());
    }
}
