use crate::{
    error::{map_cloud_client_error, ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, Binding, RegistryAuthMethod,
        ComputeServiceType, CrossAccountAccess, CrossAccountPermissions, GcpCrossAccountAccess,
        RepositoryResponse,
    },
};
use alien_core::bindings::{ArtifactRegistryBinding, GarArtifactRegistryBinding};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_gcp_clients::iam::IamPolicy;
use alien_gcp_clients::{
    artifactregistry::{ArtifactRegistryApi, ArtifactRegistryClient, Repository, RepositoryFormat},
    iam::{GenerateAccessTokenRequest, IamApi, IamClient},
    GcpClientConfig, GcpClientConfigExt as _,
};
use async_trait::async_trait;
use chrono;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// GCP Artifact Registry implementation of the ArtifactRegistry binding.
#[derive(Debug)]
pub struct GarArtifactRegistry {
    client: ArtifactRegistryClient,
    binding_name: String,
    project_id: String,
    location: String,
    repository_name: String,
    pull_service_account_email: Option<String>,
    push_service_account_email: Option<String>,
    gcp_config: GcpClientConfig,
}

impl GarArtifactRegistry {
    /// Creates a new GCP Artifact Registry binding from binding parameters.
    pub async fn new(
        binding_name: String,
        binding: ArtifactRegistryBinding,
        gcp_config: &GcpClientConfig,
    ) -> Result<Self> {
        info!(
            binding_name = %binding_name,
            "Initializing GCP Artifact Registry"
        );

        let client = crate::http_client::create_http_client();
        let artifact_registry_client = ArtifactRegistryClient::new(client, gcp_config.clone());

        // Get project_id and location from GCP config instead of binding
        let project_id = gcp_config.project_id.clone();
        let location = gcp_config.region.clone();

        // Extract service account emails from binding
        let config = match binding {
            ArtifactRegistryBinding::Gar(config) => config,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Expected GAR binding, got different service type".to_string(),
                }));
            }
        };

        let repository_name = config
            .repository_name
            .into_value(&binding_name, "repository_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract repository_name from binding".to_string(),
            })?;

        let pull_service_account_email = config
            .pull_service_account_email
            .map(|v| {
                v.into_value(&binding_name, "pull_service_account_email")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.clone(),
                        reason: "Failed to extract pull_service_account_email from binding"
                            .to_string(),
                    })
            })
            .transpose()?;

        let push_service_account_email = config
            .push_service_account_email
            .map(|v| {
                v.into_value(&binding_name, "push_service_account_email")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.clone(),
                        reason: "Failed to extract push_service_account_email from binding"
                            .to_string(),
                    })
            })
            .transpose()?;

        Ok(Self {
            client: artifact_registry_client,
            binding_name,
            project_id,
            location,
            repository_name,
            pull_service_account_email,
            push_service_account_email,
            gcp_config: gcp_config.clone(),
        })
    }

    /// Extracts a name segment from a repo ID or routable name.
    /// If `repo_id` is empty, returns the binding's configured `repository_name`
    /// (the GAR repository resource, used for IAM operations).
    fn extract_repo_name(&self, repo_id: &str) -> Result<String> {
        if repo_id.is_empty() {
            return Ok(self.repository_name.clone());
        }
        if let Some(name) = repo_id.split('/').last() {
            Ok(name.to_string())
        } else {
            Err(AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: self.binding_name.clone(),
                reason: format!("Invalid repository ID format: {}", repo_id),
            }))
        }
    }

    /// Internal helper to add or remove members from IAM policy bindings
    async fn update_policy_members(
        &self,
        repo_name: &str,
        mut current_policy: IamPolicy,
        members: Vec<String>,
        add_members: bool, // true to add, false to remove
    ) -> Result<()> {
        let reader_role = "roles/artifactregistry.reader";

        // Find or create the artifactregistry.reader binding
        let mut binding_index = None;
        for (i, binding) in current_policy.bindings.iter().enumerate() {
            if binding.role == reader_role {
                binding_index = Some(i);
                break;
            }
        }

        if add_members {
            // Add members
            if members.is_empty() {
                info!(repo_name = %repo_name, "No new members to add");
                return Ok(());
            }

            match binding_index {
                Some(i) => {
                    // Add to existing binding
                    let binding = &mut current_policy.bindings[i];
                    for member in members {
                        if !binding.members.contains(&member) {
                            binding.members.push(member);
                        }
                    }
                }
                None => {
                    // Create new binding
                    current_policy
                        .bindings
                        .push(alien_gcp_clients::iam::Binding {
                            role: reader_role.to_string(),
                            members,
                            condition: None,
                        });
                }
            }
        } else {
            // Remove members
            if let Some(i) = binding_index {
                let binding = &mut current_policy.bindings[i];
                binding.members.retain(|member| !members.contains(member));

                // Remove empty binding
                if binding.members.is_empty() {
                    current_policy.bindings.remove(i);
                }
            }
            // If no binding exists, nothing to remove
        }

        // Set the updated policy with the original etag for optimistic concurrency control
        self.client.set_repository_iam_policy(
            self.project_id.clone(),
            self.location.clone(),
            repo_name.to_string(),
            current_policy,
        ).await
            .map_err(|e| map_cloud_client_error(
                e,
                format!("Failed to update cross-account access for GCP Artifact Registry repository '{}'", repo_name),
                Some(repo_name.to_string()),
            ))?;

        let action = if add_members { "added" } else { "removed" };
        info!(
            repo_name = %repo_name,
            action = %action,
            "GCP Artifact Registry repository cross-account access updated successfully"
        );
        Ok(())
    }
}

impl Binding for GarArtifactRegistry {}

#[async_trait]
impl ArtifactRegistry for GarArtifactRegistry {
    fn registry_endpoint(&self) -> String {
        format!("https://{}-docker.pkg.dev", self.location)
    }

    fn upstream_repository_prefix(&self) -> String {
        format!("{}/{}", self.project_id, self.repository_name)
    }

    async fn create_repository(&self, repo_name: &str) -> Result<RepositoryResponse> {
        // In GAR, the binding's GAR repository (e.g., "alien-artifacts") IS the registry.
        // Image paths within it (e.g., "prj_abc123") are created automatically on first push.
        // No GAR API call needed — the GAR repository is created by alien-infra at provisioning time.
        // Underscores are valid in image paths (OCI distribution spec allows [a-z0-9._-/]).
        let routable_name = format!("{}/{}", self.upstream_repository_prefix(), repo_name);
        Ok(RepositoryResponse {
            name: routable_name,
            uri: None,
            created_at: None,
        })
    }

    async fn get_repository(&self, repo_id: &str) -> Result<RepositoryResponse> {
        // Image paths within a GAR repository are implicit — no GAR API call needed.
        // Just return the routable name, same as create_repository.
        let image_path = self.extract_repo_name(repo_id)?;
        let routable_name = format!("{}/{}", self.upstream_repository_prefix(), image_path);
        let repository_uri = format!(
            "{}-docker.pkg.dev/{}/{}",
            self.location, self.project_id, image_path
        );

        Ok(RepositoryResponse {
            name: routable_name,
            uri: Some(repository_uri),
            created_at: None,
        })
    }

    async fn add_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<()> {
        // Cross-account access in GAR is enforced via IAM bindings on the
        // *parent* repository (the GAR resource provisioned by alien-infra),
        // not on individual image paths. `repo_id` may name an image path
        // within the parent (it's the routable name from `create_repository`),
        // but IAM ops always target the parent registry.
        let _ = repo_id; // The image path doesn't have its own IAM scope.
        let repo_name = self.repository_name.clone();

        let gcp_access = match access {
            CrossAccountAccess::Gcp(gcp_access) => gcp_access,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: self.binding_name.clone(),
                    reason: "GCP artifact registry can only accept GCP cross-account access configuration".to_string(),
                }));
            }
        };

        info!(
            repo_name = %repo_name,
            project_numbers = ?gcp_access.project_numbers,
            allowed_service_types = ?gcp_access.allowed_service_types,
            service_account_emails = ?gcp_access.service_account_emails,
            "Adding GCP Artifact Registry repository cross-account access"
        );

        // Get current policy with etag
        let current_policy = self.client.get_repository_iam_policy(
            self.project_id.clone(),
            self.location.clone(),
            repo_name.clone(),
        ).await
            .map_err(|e| {
                warn!(
                    repo_name = %repo_name,
                    error = %e,
                    "Failed to get current GCP Artifact Registry repository IAM policy, creating new policy"
                );
                e
            })
            .unwrap_or_else(|_| IamPolicy {
                version: Some(1),
                kind: None,
                resource_id: None,
                bindings: vec![],
                etag: None,
            });

        // Build new members to add
        let mut new_members = Vec::new();

        // Add service accounts based on compute service types and project numbers
        for service_type in &gcp_access.allowed_service_types {
            match service_type {
                ComputeServiceType::Function => {
                    // Add serverless robot service accounts for Function service type
                    for project_number in &gcp_access.project_numbers {
                        let serverless_robot_email = format!(
                            "service-{}@serverless-robot-prod.iam.gserviceaccount.com",
                            project_number
                        );
                        new_members.push(format!("serviceAccount:{}", serverless_robot_email));
                    }
                } // Future service types would be handled here
            }
        }

        // Add additional service account emails
        for service_account_email in &gcp_access.service_account_emails {
            new_members.push(format!("serviceAccount:{}", service_account_email));
        }

        self.update_policy_members(&repo_name, current_policy, new_members, true)
            .await
    }

    async fn remove_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<()> {
        // IAM ops target the parent registry. See `add_cross_account_access`.
        let _ = repo_id;
        let repo_name = self.repository_name.clone();

        let gcp_access = match access {
            CrossAccountAccess::Gcp(gcp_access) => gcp_access,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: self.binding_name.clone(),
                    reason: "GCP artifact registry can only accept GCP cross-account access configuration".to_string(),
                }));
            }
        };

        info!(
            repo_name = %repo_name,
            project_numbers = ?gcp_access.project_numbers,
            allowed_service_types = ?gcp_access.allowed_service_types,
            service_account_emails = ?gcp_access.service_account_emails,
            "Removing GCP Artifact Registry repository cross-account access"
        );

        // Get current policy with etag
        let current_policy = match self
            .client
            .get_repository_iam_policy(
                self.project_id.clone(),
                self.location.clone(),
                repo_name.clone(),
            )
            .await
        {
            Ok(policy) => policy,
            Err(_) => {
                // No existing policy, nothing to remove
                info!(repo_name = %repo_name, "No existing GCP IAM policy to remove permissions from");
                return Ok(());
            }
        };

        // Build members to remove
        let mut members_to_remove = Vec::new();

        // Add service accounts based on compute service types and project numbers
        for service_type in &gcp_access.allowed_service_types {
            match service_type {
                ComputeServiceType::Function => {
                    // Add serverless robot service accounts for Function service type
                    for project_number in &gcp_access.project_numbers {
                        let serverless_robot_email = format!(
                            "service-{}@serverless-robot-prod.iam.gserviceaccount.com",
                            project_number
                        );
                        members_to_remove
                            .push(format!("serviceAccount:{}", serverless_robot_email));
                    }
                } // Future service types would be handled here
            }
        }

        // Add additional service account emails
        for service_account_email in &gcp_access.service_account_emails {
            members_to_remove.push(format!("serviceAccount:{}", service_account_email));
        }

        self.update_policy_members(&repo_name, current_policy, members_to_remove, false)
            .await
    }

    async fn get_cross_account_access(&self, repo_id: &str) -> Result<CrossAccountPermissions> {
        // IAM ops target the parent registry. See `add_cross_account_access`.
        let _ = repo_id;
        let repo_name = self.repository_name.clone();

        info!(
            repo_name = %repo_name,
            "Getting GCP Artifact Registry repository cross-account access"
        );

        let policy = match self
            .client
            .get_repository_iam_policy(
                self.project_id.clone(),
                self.location.clone(),
                repo_name.clone(),
            )
            .await
        {
            Ok(policy) => policy,
            Err(e) => {
                warn!(
                    repo_name = %repo_name,
                    error = %e,
                    "Failed to get GCP Artifact Registry repository IAM policy"
                );
                // If no policy exists, return empty permissions
                return Ok(CrossAccountPermissions {
                    access: CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                        project_numbers: Vec::new(),
                        allowed_service_types: Vec::new(),
                        service_account_emails: Vec::new(),
                    }),
                    last_updated: None,
                });
            }
        };

        let mut project_numbers = Vec::new();
        let mut service_account_emails = Vec::new();
        let mut allowed_service_types = Vec::new();

        for binding in policy.bindings {
            // Look for reader roles or artifact registry roles
            if binding.role.contains("reader") || binding.role.contains("artifactregistry") {
                for member in binding.members {
                    // Parse service account members only
                    if let Some(service_account) = member.strip_prefix("serviceAccount:") {
                        // Check if this is a serverless robot service account
                        if service_account
                            .contains("@serverless-robot-prod.iam.gserviceaccount.com")
                        {
                            // Extract project number from: service-{project_number}@serverless-robot-prod.iam.gserviceaccount.com
                            if let Some(project_number) =
                                service_account.strip_prefix("service-").and_then(|s| {
                                    s.strip_suffix("@serverless-robot-prod.iam.gserviceaccount.com")
                                })
                            {
                                project_numbers.push(project_number.to_string());
                                // If we found a serverless robot, we can infer Function resource type
                                if !allowed_service_types.contains(&ComputeServiceType::Function) {
                                    allowed_service_types.push(ComputeServiceType::Function);
                                }
                            }
                        } else {
                            // Regular service account
                            service_account_emails.push(service_account.to_string());
                        }
                    }
                }
            }
        }

        // Remove duplicates and sort
        project_numbers.sort();
        project_numbers.dedup();
        service_account_emails.sort();
        service_account_emails.dedup();
        allowed_service_types.sort_by_key(|rt| format!("{:?}", rt));
        allowed_service_types.dedup();

        info!(
            repo_name = %repo_name,
            project_numbers = ?project_numbers,
            allowed_service_types = ?allowed_service_types,
            service_account_emails = ?service_account_emails,
            "Retrieved GCP Artifact Registry repository cross-account access"
        );

        Ok(CrossAccountPermissions {
            access: CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                project_numbers,
                allowed_service_types,
                service_account_emails,
            }),
            last_updated: None, // GCP IAM doesn't provide policy modification time
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
            "Generating GCP Artifact Registry credentials by impersonating service account"
        );

        // Parse repo_id to extract project and location info if it's in full format
        // Just use the configured project/location from this client since they come from the binding
        let project_id = &self.project_id;
        let location = &self.location;

        // Get the service account email from stored fields
        let service_account_email = match permissions {
            ArtifactRegistryPermissions::Pull => {
                self.pull_service_account_email.clone()
                    .ok_or_else(|| AlienError::new(ErrorData::BindingConfigInvalid {
                        binding_name: self.binding_name.clone(),
                        reason: "Pull service account email not available - ensure the artifact registry resource is properly linked".to_string(),
                    }))?
            }
            ArtifactRegistryPermissions::PushPull => {
                self.push_service_account_email.clone()
                    .ok_or_else(|| AlienError::new(ErrorData::BindingConfigInvalid {
                        binding_name: self.binding_name.clone(),
                        reason: "Push service account email not available - ensure the artifact registry resource is properly linked".to_string(),
                    }))?
            }
        };

        info!(
            service_account_email = %service_account_email,
            "Using stored service account email for GCP Artifact Registry access"
        );

        // Use the stored GCP configuration for impersonation
        let gcp_config = &self.gcp_config;

        let scopes = vec![
            "https://www.googleapis.com/auth/cloud-platform".to_string(),
            "https://www.googleapis.com/auth/devstorage.read_write".to_string(),
        ];

        let lifetime = ttl_seconds.map(|ttl| format!("{}s", ttl.min(3600))); // Max 1 hour

        let impersonation_config = alien_gcp_clients::GcpImpersonationConfig {
            service_account_email: service_account_email.clone(),
            scopes,
            delegates: None,
            lifetime,
            target_project_id: None,
            target_region: None,
        };

        // Impersonate the service account
        let impersonated_config =
            gcp_config
                .impersonate(impersonation_config)
                .await
                .map_err(|e| {
                    map_cloud_client_error(
                        e,
                        "Failed to impersonate GCP service account for artifact registry access"
                            .to_string(),
                        Some(repo_id.to_string()),
                    )
                })?;

        // Get the access token from the impersonated config
        let access_token = impersonated_config
            .get_bearer_token("https://www.googleapis.com/")
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    "Failed to get OAuth token from impersonated service account".to_string(),
                    Some(repo_id.to_string()),
                )
            })?;

        // Calculate expiration time
        let expires_at = if let Some(ttl) = ttl_seconds {
            Some(
                (chrono::Utc::now() + chrono::Duration::seconds(ttl.min(3600) as i64)).to_rfc3339(),
            )
        } else {
            Some((chrono::Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339())
            // Default 1 hour
        };

        info!(
            permissions = ?permissions,
            service_account = %service_account_email,
            "GCP Artifact Registry OAuth token generated successfully with impersonated service account"
        );

        // For GCP Artifact Registry, the username is 'oauth2accesstoken' and password is the OAuth token
        Ok(ArtifactRegistryCredentials {
            auth_method: RegistryAuthMethod::Basic,
            username: "oauth2accesstoken".to_string(),
            password: access_token,
            expires_at,
        })
    }

    async fn delete_repository(&self, repo_id: &str) -> Result<()> {
        // No-op, mirroring `create_repository`/`get_repository`.
        //
        // In GAR, image paths within the parent repository are implicit —
        // they materialise on first push, and there's no "delete a path"
        // operation in the GAR API. The parent repository itself is owned
        // by `alien-infra` (provisioned at deployment time), not by the
        // binding; deleting it would tear down the whole registry every
        // user shares.
        //
        // Garbage collection of unused image data is a separate concern,
        // achievable via `deletePackage` if/when needed. Not done here
        // because the test pattern `create → ... → delete` should be
        // idempotent and side-effect-free at the registry-resource level.
        debug!(
            repo_id = %repo_id,
            "GCP Artifact Registry delete_repository: no-op (image paths are implicit)"
        );
        Ok(())
    }
}
