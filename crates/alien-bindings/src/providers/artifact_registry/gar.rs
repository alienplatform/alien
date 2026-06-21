use crate::{
    error::{ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, Binding,
        ComputeServiceType, CrossAccountAccess, CrossAccountPermissions, GcpCrossAccountAccess,
        RegistryAuthMethod, RepositoryResponse,
    },
};
use alien_core::bindings::ArtifactRegistryBinding;
use alien_core::{GcpClientConfig, GcpCredentials, GcpImpersonationConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use chrono;
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use reqwest::{Client, Method, Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::future::Future;
use std::time::Duration;
use tracing::{debug, info, warn};

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const DEVSTORAGE_SCOPE: &str = "https://www.googleapis.com/auth/devstorage.read_write";
const ARTIFACT_REGISTRY_REST_BASE_URL: &str = "https://artifactregistry.googleapis.com/v1";

/// GCP Artifact Registry implementation of the ArtifactRegistry binding.
#[derive(Debug)]
pub struct GarArtifactRegistry {
    client: Client,
    credentials: Credentials,
    endpoint: String,
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
        let credentials = credentials_from_gcp_config(gcp_config)?;
        let endpoint = gcp_config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("artifactregistry"))
            .cloned()
            .unwrap_or_else(|| ARTIFACT_REGISTRY_REST_BASE_URL.to_string());

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
            client,
            credentials,
            endpoint,
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

    fn repository_resource_name(&self, repo_name: &str) -> String {
        format!(
            "projects/{}/locations/{}/repositories/{}",
            self.project_id, self.location, repo_name
        )
    }

    fn build_url(&self, resource: &str, suffix: &str) -> Result<Url> {
        Url::parse(&format!(
            "{}/{}{}",
            self.endpoint.trim_end_matches('/'),
            resource,
            suffix
        ))
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "artifactRegistry.gar".to_string(),
            reason: "Invalid Artifact Registry IAM URL".to_string(),
        })
    }

    async fn authed_request(&self, method: Method, url: Url) -> Result<reqwest::RequestBuilder> {
        let headers = match self
            .credentials
            .headers(Extensions::new())
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "artifactRegistry.gar".to_string(),
                reason: "Failed to get Google auth headers".to_string(),
            })? {
            CacheableResource::New { data, .. } => data,
            CacheableResource::NotModified => {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "artifactRegistry.gar".to_string(),
                    reason: "Google auth returned NotModified without cached headers".to_string(),
                }));
            }
        };

        Ok(self.client.request(method, url).headers(headers))
    }

    async fn get_repository_iam_policy(&self, repo_name: &str) -> Result<IamPolicy> {
        let resource = self.repository_resource_name(repo_name);
        let url = self.build_url(&resource, ":getIamPolicy")?;
        let response = self
            .authed_request(Method::GET, url.clone())
            .await?
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!(
                    "Failed to send Artifact Registry getIamPolicy request for '{}'",
                    repo_name
                ),
            })?;

        ensure_success(response, "getIamPolicy", repo_name, url)
            .await?
            .json::<IamPolicy>()
            .await
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "gcp".to_string(),
                binding_name: self.binding_name.clone(),
                field: "iamPolicy".to_string(),
                response_json: String::new(),
            })
    }

    async fn set_repository_iam_policy(
        &self,
        repo_name: &str,
        policy: IamPolicy,
    ) -> Result<IamPolicy> {
        let resource = self.repository_resource_name(repo_name);
        let url = self.build_url(&resource, ":setIamPolicy")?;
        let request = SetIamPolicyRequest { policy };
        let response = self
            .authed_request(Method::POST, url.clone())
            .await?
            .json(&request)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!(
                    "Failed to send Artifact Registry setIamPolicy request for '{}'",
                    repo_name
                ),
            })?;

        ensure_success(response, "setIamPolicy", repo_name, url)
            .await?
            .json::<IamPolicy>()
            .await
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "gcp".to_string(),
                binding_name: self.binding_name.clone(),
                field: "iamPolicy".to_string(),
                response_json: String::new(),
            })
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
                    current_policy.bindings.push(IamBinding {
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
        self.set_repository_iam_policy(repo_name, current_policy)
            .await?;

        let action = if add_members { "added" } else { "removed" };
        info!(
            repo_name = %repo_name,
            action = %action,
            "GCP Artifact Registry repository cross-account access updated successfully"
        );
        Ok(())
    }
}

async fn ensure_success(
    response: Response,
    operation: &str,
    repo_name: &str,
    url: Url,
) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(AlienError::new(ErrorData::Other {
        message: format!(
            "Artifact Registry {operation} request for '{repo_name}' to {url} failed with status {status}: {body}"
        ),
    }))
}

#[derive(Debug, Clone)]
struct StaticAccessTokenCredentials {
    token: String,
    entity_tag: EntityTag,
}

impl StaticAccessTokenCredentials {
    fn new(token: String) -> Self {
        Self {
            token,
            entity_tag: EntityTag::new(),
        }
    }
}

impl CredentialsProvider for StaticAccessTokenCredentials {
    fn headers(
        &self,
        _extensions: Extensions,
    ) -> impl Future<Output = std::result::Result<CacheableResource<HeaderMap>, CredentialsError>> + Send
    {
        let token = self.token.clone();
        let entity_tag = self.entity_tag.clone();
        async move {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| CredentialsError::from_source(false, error))?;
            value.set_sensitive(true);

            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);

            Ok(CacheableResource::New {
                entity_tag,
                data: headers,
            })
        }
    }

    fn universe_domain(&self) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
}

fn credentials_from_gcp_config(config: &GcpClientConfig) -> Result<Credentials> {
    credentials_from_gcp_credentials(&config.credentials)
}

fn credentials_from_gcp_credentials(credentials: &GcpCredentials) -> Result<Credentials> {
    match credentials {
        GcpCredentials::AccessToken { token } => {
            Ok(Credentials::from(StaticAccessTokenCredentials::new(token.clone())))
        }
        GcpCredentials::ServiceAccountKey { json } => {
            let key = serde_json::from_str::<Value>(json).into_alien_error().context(
                ErrorData::BindingSetupFailed {
                    binding_type: "artifactRegistry.gar".to_string(),
                    reason: "Failed to parse GCP service account key JSON".to_string(),
                },
            )?;
            credentials::service_account::Builder::new(key)
                .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                    [CLOUD_PLATFORM_SCOPE],
                ))
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "artifactRegistry.gar".to_string(),
                    reason: "Failed to build official GCP service account credentials".to_string(),
                })
        }
        GcpCredentials::ServiceMetadata => credentials::mds::Builder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build()
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "artifactRegistry.gar".to_string(),
                reason: "Failed to build official GCP metadata credentials".to_string(),
            }),
        GcpCredentials::ExternalAccount {
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        } => {
            let external_account = external_account_json(
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url.as_deref(),
            );
            credentials::external_account::Builder::new(external_account)
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "artifactRegistry.gar".to_string(),
                    reason: "Failed to build official GCP external account credentials".to_string(),
                })
        }
        GcpCredentials::AuthorizedUser {
            client_id,
            client_secret,
            refresh_token,
        } => {
            let authorized_user = json!({
                "type": "authorized_user",
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
            });
            credentials::user_account::Builder::new(authorized_user)
                .with_scopes([CLOUD_PLATFORM_SCOPE])
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "artifactRegistry.gar".to_string(),
                    reason: "Failed to build official GCP authorized user credentials".to_string(),
                })
        }
        GcpCredentials::ImpersonatedServiceAccount { source, config } => {
            impersonated_credentials_from_gcp_config(source, config)
        }
        GcpCredentials::ProjectedServiceAccount { .. } => Err(AlienError::new(
            ErrorData::BindingSetupFailed {
                binding_type: "artifactRegistry.gar".to_string(),
                reason: "Projected service account token files are not a complete official Google auth credential configuration; use external_account credentials with an audience and credential source instead".to_string(),
            },
        )),
    }
}

fn impersonated_credentials_from_gcp_config(
    source: &GcpClientConfig,
    config: &GcpImpersonationConfig,
) -> Result<Credentials> {
    let source_credentials = credentials_from_gcp_config(source)?;
    let mut builder =
        credentials::impersonated::Builder::from_source_credentials(source_credentials)
            .with_target_principal(config.service_account_email.clone())
            .with_scopes(config.scopes.clone());

    if let Some(delegates) = &config.delegates {
        builder = builder.with_delegates(delegates.clone());
    }

    if let Some(lifetime) = &config.lifetime {
        builder = builder.with_lifetime(parse_google_duration(lifetime)?);
    }

    builder
        .build()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "artifactRegistry.gar".to_string(),
            reason: "Failed to build official GCP impersonated credentials".to_string(),
        })
}

async fn bearer_token_from_credentials(credentials: &Credentials) -> Result<String> {
    let headers = match credentials
        .headers(Extensions::new())
        .await
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "artifactRegistry.gar".to_string(),
            reason: "Failed to get Google auth headers".to_string(),
        })? {
        CacheableResource::New { data, .. } => data,
        CacheableResource::NotModified => {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "artifactRegistry.gar".to_string(),
                reason: "Google auth returned NotModified without cached headers".to_string(),
            }));
        }
    };

    let value = headers
        .get(AUTHORIZATION)
        .ok_or_else(|| {
            AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "artifactRegistry.gar".to_string(),
                reason: "Google auth headers missing Authorization".to_string(),
            })
        })?
        .to_str()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "artifactRegistry.gar".to_string(),
            reason: "Google auth Authorization header is not valid UTF-8".to_string(),
        })?;

    value
        .strip_prefix("Bearer ")
        .map(str::to_string)
        .ok_or_else(|| {
            AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "artifactRegistry.gar".to_string(),
                reason: "Google auth Authorization header is not a bearer token".to_string(),
            })
        })
}

fn external_account_json(
    audience: &str,
    subject_token_type: &str,
    token_url: &str,
    credential_source_file: &str,
    service_account_impersonation_url: Option<&str>,
) -> Value {
    let mut value = json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": subject_token_type,
        "token_url": token_url,
        "credential_source": {
            "file": credential_source_file,
        },
        "scopes": [CLOUD_PLATFORM_SCOPE],
    });

    if let Some(url) = service_account_impersonation_url {
        value["service_account_impersonation_url"] = Value::String(url.to_string());
    }

    value
}

fn parse_google_duration(value: &str) -> Result<Duration> {
    let seconds = value
        .strip_suffix('s')
        .ok_or_else(|| {
            AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "artifactRegistry.gar".to_string(),
                reason: format!("Invalid Google duration '{}': missing 's' suffix", value),
            })
        })?
        .parse::<u64>()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "artifactRegistry.gar".to_string(),
            reason: format!("Invalid Google duration '{}'", value),
        })?;

    Ok(Duration::from_secs(seconds))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SetIamPolicyRequest {
    policy: IamPolicy,
}

/// Represents an IAM policy.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct IamPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bindings: Vec<IamBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    etag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct IamBinding {
    role: String,
    #[serde(default)]
    members: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    condition: Option<Expr>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Expr {
    expression: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
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
        let current_policy = self.get_repository_iam_policy(&repo_name).await
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
                ComputeServiceType::Worker => {
                    // Add serverless robot service accounts for Worker service type
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
        let current_policy = match self.get_repository_iam_policy(&repo_name).await {
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
                ComputeServiceType::Worker => {
                    // Add serverless robot service accounts for Worker service type
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

        let policy = match self.get_repository_iam_policy(&repo_name).await {
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
                                // If we found a serverless robot, we can infer Worker resource type
                                if !allowed_service_types.contains(&ComputeServiceType::Worker) {
                                    allowed_service_types.push(ComputeServiceType::Worker);
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
        let _project_id = &self.project_id;
        let _location = &self.location;

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
        let scopes = vec![
            CLOUD_PLATFORM_SCOPE.to_string(),
            DEVSTORAGE_SCOPE.to_string(),
        ];

        let lifetime = ttl_seconds.map(|ttl| format!("{}s", ttl.min(3600))); // Max 1 hour

        let impersonation_config = GcpImpersonationConfig {
            service_account_email: service_account_email.clone(),
            scopes,
            delegates: None,
            lifetime,
            target_project_id: None,
            target_region: None,
        };

        let impersonated_credentials =
            impersonated_credentials_from_gcp_config(&self.gcp_config, &impersonation_config)?;
        let access_token = bearer_token_from_credentials(&impersonated_credentials)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "artifactRegistry.gar".to_string(),
                reason: "Failed to get OAuth token from impersonated service account".to_string(),
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
