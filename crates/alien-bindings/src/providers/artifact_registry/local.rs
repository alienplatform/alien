use crate::{
    error::{ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, Binding, RegistryAuthMethod,
        CrossAccountAccess, CrossAccountPermissions, RepositoryResponse,
    },
};
use alien_core::bindings::ArtifactRegistryBinding;
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use async_trait::async_trait;
use oci_client::{
    client::{Client as OciClient, ClientConfig as OciClientConfig, ClientProtocol},
    errors::OciDistributionError,
    secrets::RegistryAuth,
    Reference,
};
use tracing::{debug, info};

/// Local artifact registry implementation that connects to an external container registry.
///
/// This is a **client** that connects to a local container registry server
/// (e.g., started by LocalArtifactRegistryManager in alien-local).
///
/// Unlike cloud providers that have explicit repository creation APIs, Docker registries
/// implicitly create repositories on first push. To provide a consistent interface,
/// this implementation pushes a minimal empty manifest when `create_repository()` is called,
/// ensuring the repository exists and can be queried immediately afterward.
#[derive(Debug)]
pub struct LocalArtifactRegistry {
    binding_name: String,
    registry_endpoint: String,
}

impl LocalArtifactRegistry {
    /// Creates a new local artifact registry instance from binding parameters.
    ///
    /// # Arguments
    /// * `binding_name` - The name of this binding
    /// * `binding` - The binding configuration containing registry settings
    pub async fn new(
        binding_name: String,
        binding: alien_core::bindings::ArtifactRegistryBinding,
    ) -> Result<Self> {
        // Extract fields from Local variant
        let config = match binding {
            ArtifactRegistryBinding::Local(config) => config,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name,
                    reason: "Expected Local artifact registry binding variant".to_string(),
                }));
            }
        };

        let registry_endpoint = config
            .registry_url
            .into_value(&binding_name, "registry_url")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract registry_url from binding".to_string(),
            })?;

        // Validate the registry endpoint format
        if registry_endpoint.is_empty() {
            return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Registry endpoint cannot be empty".to_string(),
            }));
        }

        info!(
            binding_name = %binding_name,
            endpoint = %registry_endpoint,
            "Local artifact registry client configured"
        );

        Ok(Self {
            binding_name,
            registry_endpoint,
        })
    }

    /// Gets the registry endpoint for this local registry
    pub fn registry_endpoint(&self) -> &str {
        &self.registry_endpoint
    }

    /// Creates an OCI client for communicating with the local registry
    fn create_oci_client(&self) -> OciClient {
        OciClient::new(OciClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        })
    }

    /// Creates an OCI Reference from a logical repository name (e.g. `"my-app"`).
    /// The reference points at `{registry}/{binding_name}/{logical}:latest`.
    fn create_reference(&self, logical: &str) -> Result<Reference> {
        // registry_endpoint is like "localhost:5000"
        // The container-registry crate requires a two-level path: /v2/:repository/:image/...
        // We use the binding name as :repository and the logical name as :image to match
        // the conceptual model: "artifacts registry contains alien-prj_xxx repository".
        // This also enables namespace separation for multiple ArtifactRegistry resources.
        let ref_string = format!(
            "{}/{}/{}:latest",
            self.registry_endpoint, self.binding_name, logical
        );
        Reference::try_from(ref_string.as_str())
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Invalid repository reference: {}", ref_string),
            })
    }

    /// Build the routable name (`{binding_name}/{logical}`) returned to
    /// callers. Per `traits::RepositoryResponse::name`, this is the value
    /// that round-trips through `get_repository`/`delete_repository`.
    fn routable_name(&self, logical: &str) -> String {
        if logical.is_empty() {
            self.binding_name.clone()
        } else {
            format!("{}/{}", self.binding_name, logical)
        }
    }

    /// Recover the logical name from a routable name passed back by a
    /// caller. Tolerates either form: a routable name like
    /// `"{binding_name}/{logical}"` is stripped, anything else is treated
    /// as already-logical.
    fn logical_from_routable<'a>(&self, repo_id: &'a str) -> &'a str {
        let prefix = format!("{}/", self.binding_name);
        repo_id.strip_prefix(prefix.as_str()).unwrap_or(repo_id)
    }
}

impl Binding for LocalArtifactRegistry {}

#[async_trait]
impl ArtifactRegistry for LocalArtifactRegistry {
    fn registry_endpoint(&self) -> String {
        let host = &self.registry_endpoint;
        if host.starts_with("http://") || host.starts_with("https://") {
            host.clone()
        } else {
            format!("http://{}", host)
        }
    }

    fn upstream_repository_prefix(&self) -> String {
        // The embedded local registry accepts two-segment repo paths (e.g.,
        // "namespace/repo"). We use "artifacts/default" as the canonical prefix
        // — this matches what the CLI hardcodes in dev mode and what the proxy
        // routing table uses to route pushes to this local registry.
        "artifacts/default".to_string()
    }

    async fn create_repository(&self, repo_name: &str) -> Result<RepositoryResponse> {
        info!(
            binding_name = %self.binding_name,
            repo_name = %repo_name,
            "Creating local Docker repository"
        );

        // For Docker registries, repositories are created implicitly on first manifest push
        // We push a minimal empty OCI Image Manifest to make the repository exist
        // This ensures consistent behavior with cloud providers where create_repository
        // makes the repository immediately queryable

        let client = self.create_oci_client();
        let reference = self.create_reference(repo_name)?;

        // Create a minimal OCI Image Manifest with inline config and no layers
        use oci_client::manifest::{OciDescriptor, OciImageManifest, OciManifest};

        // Create minimal empty config
        let config_json = serde_json::json!({
            "architecture": "amd64",
            "os": "linux",
            "rootfs": {
                "type": "layers",
                "diff_ids": []
            },
            "config": {}
        });

        let config_bytes = serde_json::to_vec(&config_json)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to serialize config".to_string(),
            })?;

        // Calculate SHA256 digest for config
        use sha2::{Digest as Sha2Digest, Sha256};
        let config_digest = format!("sha256:{:x}", Sha256::digest(&config_bytes));

        // Create config descriptor
        let config_descriptor = OciDescriptor {
            media_type: "application/vnd.oci.image.config.v1+json".to_string(),
            size: config_bytes.len() as i64,
            digest: config_digest.clone(),
            urls: None,
            annotations: None,
        };

        // Create minimal manifest with just the config (no layers)
        let manifest = OciImageManifest {
            schema_version: 2,
            media_type: Some("application/vnd.oci.image.manifest.v1+json".to_string()),
            config: config_descriptor,
            layers: vec![], // Empty - no layers
            annotations: Some({
                let mut map = std::collections::BTreeMap::new();
                map.insert(
                    "dev.alien.marker".to_string(),
                    "empty-repository-created-by-alien".to_string(),
                );
                map
            }),
            subject: None,
            artifact_type: None,
        };

        // Push the config blob first (OCI spec requires all referenced blobs to exist before
        // pushing a manifest), then push the manifest to create the repository.
        let auth = RegistryAuth::Anonymous;
        client
            .store_auth_if_needed(&self.registry_endpoint, &auth)
            .await;

        client
            .push_blob(&reference, &config_bytes, &config_digest)
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to push config blob for repository '{}'", repo_name),
            })?;

        client
            .push_manifest(&reference, &OciManifest::Image(manifest))
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!(
                    "Failed to push marker manifest for repository '{}'",
                    repo_name
                ),
            })?;

        // Repository URI uses binding name as first component for namespace separation.
        // Format: registry/binding-name/repository (e.g., localhost:5000/artifacts/alien-prj_xxx)
        // This satisfies container-registry's two-level requirement and provides semantic clarity.
        let repository_uri = format!(
            "{}/{}/{}",
            self.registry_endpoint, self.binding_name, repo_name
        );

        info!(
            binding_name = %self.binding_name,
            repo_name = %repo_name,
            uri = %repository_uri,
            "Local Docker repository created successfully"
        );

        // Return the routable name (`{binding_name}/{logical}`) — matches
        // both the on-disk OCI path and the docs at
        // `alien.dev/content/docs/infrastructure/artifact-registry/behavior.mdx`.
        // The manager proxy routes via `upstream_repository_prefix()`, which
        // is a separate concern from this binding-level identifier.
        Ok(RepositoryResponse {
            name: self.routable_name(repo_name),
            uri: Some(repository_uri),
            created_at: None,
        })
    }

    async fn get_repository(&self, repo_id: &str) -> Result<RepositoryResponse> {
        debug!(
            binding_name = %self.binding_name,
            repo_id = %repo_id,
            "Checking local repository existence via OCI API"
        );

        // Per the trait contract, `repo_id` is the routable name returned by
        // `create_repository` (`{binding_name}/{logical}`). Recover the
        // logical segment so we can build the OCI reference.
        let logical = self.logical_from_routable(repo_id);

        // Use oci-client to check if repository exists by trying to fetch a manifest
        let client = self.create_oci_client();
        let reference = self.create_reference(logical)?;

        // Store auth credentials for this registry
        let auth = RegistryAuth::Anonymous;
        client
            .store_auth_if_needed(&self.registry_endpoint, &auth)
            .await;

        // Try to pull the manifest we created (or any manifest with :latest tag)
        // This hits /v2/<repository>/<image>/manifests/<reference> endpoint
        match client.pull_manifest(&reference, &auth).await {
            Ok(_) => {
                // Repository exists and has at least one manifest.
                // URI format matches create_repository: registry/binding-name/logical.
                let repository_uri = format!(
                    "{}/{}/{}",
                    self.registry_endpoint, self.binding_name, logical
                );

                debug!(
                    binding_name = %self.binding_name,
                    repo_id = %repo_id,
                    repo_uri = %repository_uri,
                    "Local repository exists"
                );

                Ok(RepositoryResponse {
                    name: self.routable_name(logical),
                    uri: Some(repository_uri),
                    created_at: None,
                })
            }
            Err(OciDistributionError::ServerError { code: 404, .. }) => {
                // Repository or manifest doesn't exist (404 from registry)
                debug!(
                    binding_name = %self.binding_name,
                    repo_id = %repo_id,
                    "Local repository not found (404)"
                );

                Err(AlienError::new(ErrorData::ResourceNotFound {
                    resource_id: repo_id.to_string(),
                }))
            }
            Err(OciDistributionError::ImageManifestNotFoundError(_)) => {
                // Manifest doesn't exist - treat as repository not found
                debug!(
                    binding_name = %self.binding_name,
                    repo_id = %repo_id,
                    "Local repository not found (manifest not found)"
                );

                Err(AlienError::new(ErrorData::ResourceNotFound {
                    resource_id: repo_id.to_string(),
                }))
            }
            Err(OciDistributionError::RegistryError { envelope, .. })
                if envelope.errors.iter().any(|e| {
                    matches!(
                        e.code,
                        oci_client::errors::OciErrorCode::BlobUnknown
                            | oci_client::errors::OciErrorCode::ManifestUnknown
                            | oci_client::errors::OciErrorCode::NameUnknown
                    )
                }) =>
            {
                // Blob/manifest/repository doesn't exist - expected "not found" case
                debug!(
                    binding_name = %self.binding_name,
                    repo_id = %repo_id,
                    "Local repository not found (OCI error: blob/manifest/name unknown)"
                );

                Err(AlienError::new(ErrorData::ResourceNotFound {
                    resource_id: repo_id.to_string(),
                }))
            }
            Err(e) => {
                // Actual unexpected errors (connection issues, auth failures, etc.)
                // Fail fast - don't silently treat these as "not found"
                Err(e.into_alien_error().context(ErrorData::Other {
                    message: "Failed to check repository existence".to_string(),
                }))
            }
        }
    }

    async fn add_cross_account_access(
        &self,
        repo_id: &str,
        _access: CrossAccountAccess,
    ) -> Result<()> {
        info!(
            binding_name = %self.binding_name,
            repo_id = %repo_id,
            "Local artifact registry cross-account access not supported"
        );

        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "add_cross_account_access".to_string(),
            reason: "Local artifact registry does not support cross-account access".to_string(),
        }))
    }

    async fn remove_cross_account_access(
        &self,
        repo_id: &str,
        _access: CrossAccountAccess,
    ) -> Result<()> {
        info!(
            binding_name = %self.binding_name,
            repo_id = %repo_id,
            "Local artifact registry cross-account access not supported"
        );

        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "remove_cross_account_access".to_string(),
            reason: "Local artifact registry does not support cross-account access".to_string(),
        }))
    }

    async fn get_cross_account_access(&self, repo_id: &str) -> Result<CrossAccountPermissions> {
        info!(
            binding_name = %self.binding_name,
            repo_id = %repo_id,
            "Local artifact registry cross-account access not supported"
        );

        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "get_cross_account_access".to_string(),
            reason: "Local artifact registry does not support cross-account access".to_string(),
        }))
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
            "Generating local artifact registry credentials"
        );

        // Local registry runs on localhost without auth.
        // Return empty credentials — callers should use anonymous access.
        Ok(ArtifactRegistryCredentials {
            auth_method: RegistryAuthMethod::Basic,
            username: String::new(),
            password: String::new(),
            expires_at: None,
        })
    }

    async fn delete_repository(&self, repo_id: &str) -> Result<()> {
        info!(
            binding_name = %self.binding_name,
            repo_id = %repo_id,
            "Deleting local repository (stateless - no-op)"
        );

        // For local registries, deletion is a no-op since we don't track state.
        // The actual registry server handles storage.
        info!(
            binding_name = %self.binding_name,
            repo_id = %repo_id,
            "Local repository deletion acknowledged (no-op for stateless client)"
        );

        Ok(())
    }
}
