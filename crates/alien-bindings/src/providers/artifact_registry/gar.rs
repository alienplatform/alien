use crate::{
    error::{binding_env_var, map_cloud_client_error, ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, Binding,
        ComputeServiceType, CrossAccountAccess, CrossAccountPermissions, GcpCrossAccountAccess,
        RegistryAuthMethod, RepositoryResponse,
    },
};
use alien_core::bindings::ArtifactRegistryBinding;
use alien_error::{AlienError, Context};
use alien_gcp_clients::iam::IamPolicy;
use alien_gcp_clients::{
    artifactregistry::{ArtifactRegistryApi, ArtifactRegistryClient, Repository, RepositoryFormat},
    longrunning::{Operation, OperationResult},
    GcpClientConfig, GcpClientConfigExt as _,
};
use async_trait::async_trait;
use chrono;
use tracing::{debug, info, warn};

/// How long to wait between reads of a repository operation; 60 reads bound the wait.
#[cfg(not(test))]
const OPERATION_POLL: std::time::Duration = std::time::Duration::from_secs(1);
// The mocked API settles an operation on the next read, so a unit test only pays for the reads.
#[cfg(test)]
const OPERATION_POLL: std::time::Duration = std::time::Duration::from_millis(5);

/// The Google-managed service agent a compute service pulls as.
///
/// Google's default grant covers a same-project pull; naming this agent is what reaches a
/// repository in another project. Three adjacent Vertex agents exist, so naming the wrong one 403s.
fn agent_domain(service_type: &ComputeServiceType) -> &'static str {
    match service_type {
        ComputeServiceType::Worker => "serverless-robot-prod",
        ComputeServiceType::Sandbox => "gcp-sa-vertex-sandbox",
    }
}

/// Whether Google creates this service agent only once the service first runs, so a binding
/// naming it is refused until then and its member has to go on in a write of its own.
fn is_agent_created_on_first_use(service_type: &ComputeServiceType) -> bool {
    match service_type {
        ComputeServiceType::Sandbox => true,
        ComputeServiceType::Worker => false,
    }
}

/// The service type and project a member names, if the write path emitted it.
///
/// Reads through the same table the write used, so a sandbox agent doesn't decode as an ordinary
/// service account and report no grant while its member still sits on the policy.
fn agent_member(service_account: &str) -> Option<(ComputeServiceType, &str)> {
    ComputeServiceType::ALL.iter().find_map(|service_type| {
        let suffix = format!("@{}.iam.gserviceaccount.com", agent_domain(service_type));
        service_account
            .strip_prefix("service-")
            .and_then(|rest| rest.strip_suffix(&suffix))
            .map(|project_number| (service_type.clone(), project_number))
    })
}

/// The IAM members a cross-account grant names, for both the add and the remove path.
///
/// Each compute service pulls as its own service agent, so the project number alone doesn't say
/// who may pull; grant and revoke share this list so a member the revoke misses doesn't strand.
fn cross_account_members(access: &GcpCrossAccountAccess) -> Vec<String> {
    let mut members = Vec::new();
    for service_type in &access.allowed_service_types {
        let agent_domain = agent_domain(service_type);
        for project_number in &access.project_numbers {
            members.push(format!(
                "serviceAccount:service-{project_number}@{agent_domain}.iam.gserviceaccount.com"
            ));
        }
    }
    for service_account_email in &access.service_account_emails {
        members.push(format!("serviceAccount:{service_account_email}"));
    }
    members
}

/// The GAR repository a routable name resolves to.
#[derive(Debug, PartialEq, Eq)]
enum GarTarget {
    /// The binding's own repository. `path` is the image path inside it, when the name has one.
    Binding { path: Option<String> },
    /// One Alien project's repository, from [`ArtifactRegistry::create_project_repository`].
    Project(String),
}

/// GCP Artifact Registry implementation of the ArtifactRegistry binding.
///
/// The binding names one GAR repository. [`ArtifactRegistry::create_repository`] returns image
/// paths inside it and creates nothing, so a stack needs no permission beyond that repository.
/// GAR has no IAM below a repository, so every name shares the repository's access: per-name
/// grants to other principals are refused rather than widened to the whole repository. A
/// registry shared by many Alien projects, such as a manager's, uses
/// [`ArtifactRegistry::create_project_repository`] for one repository per project instead.
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
                    env_var: binding_env_var(&binding_name),
                    binding_name: binding_name.clone(),
                    reason: "Expected GAR binding, got different service type".to_string(),
                }));
            }
        };

        let repository_name = config
            .repository_name
            .into_value(&binding_name, "repository_name")
            .context(ErrorData::BindingConfigInvalid {
                env_var: binding_env_var(&binding_name),
                binding_name: binding_name.clone(),
                reason: "Failed to extract repository_name from binding".to_string(),
            })?;

        // Terraform setups pass the repository's full resource ID; IAM and image paths use its name.
        let repository_name = repository_name
            .rsplit_once("/repositories/")
            .map_or(repository_name.clone(), |(_, name)| name.to_string());

        let pull_service_account_email = config
            .pull_service_account_email
            .map(|v| {
                v.into_value(&binding_name, "pull_service_account_email")
                    .context(ErrorData::BindingConfigInvalid {
                        env_var: binding_env_var(&binding_name),
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
                        env_var: binding_env_var(&binding_name),
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

    fn project_repository_name(&self, project_id: &str) -> Result<String> {
        alien_core::gar_repository::project_repository(&self.repository_name, project_id)
            .ok_or_else(|| AlienError::new(ErrorData::BindingConfigInvalid {
                env_var: binding_env_var(&self.binding_name),
                binding_name: self.binding_name.clone(),
                reason: "GAR project repository name must fit 63 lowercase letters, digits and hyphens".to_string(),
            }))
    }

    /// IAM operations name GAR resources, never paths inside a mixed repository.
    fn iam_repository_name(&self, repo_id: &str) -> Result<String> {
        let path = repo_id
            .strip_prefix(&format!("{}/", self.project_id))
            .unwrap_or(repo_id);
        let repository = path.split('/').next().unwrap_or_default();
        let logical_id = repository
            .strip_prefix(&format!("{}-prj-", self.repository_name))
            .map(|suffix| format!("prj_{suffix}"))
            .or_else(|| {
                (repository == format!("{}-default", self.repository_name))
                    .then(|| "default".to_string())
            });
        if let Some(logical_id) = logical_id {
            let valid_path = path
                .split_once('/')
                .is_none_or(|(_, rest)| rest.split('/').next() == Some(logical_id.as_str()));
            if valid_path && self.project_repository_name(&logical_id)? == repository {
                return Ok(repository.to_string());
            }
        }
        Err(AlienError::new(ErrorData::BindingConfigInvalid {
            env_var: binding_env_var(&self.binding_name),
            binding_name: self.binding_name.clone(),
            reason: "IAM repository is outside this binding's namespace".to_string(),
        }))
    }

    /// Resolves a routable name to the binding's own repository or to a project repository.
    ///
    /// Deployments granted before per-project repositories recorded their grant on the binding's
    /// repository itself, so that name resolves too: a revoke there must not fail teardown.
    fn target(&self, repo_id: &str) -> Result<GarTarget> {
        let path = repo_id
            .strip_prefix(&format!("{}/", self.project_id))
            .unwrap_or(repo_id);
        let (repository, image_path) = match path.split_once('/') {
            Some((repository, image_path)) => (repository, Some(image_path)),
            None => (path, None),
        };
        if repository == self.repository_name {
            return Ok(GarTarget::Binding {
                path: image_path
                    .filter(|image_path| !image_path.is_empty())
                    .map(str::to_string),
            });
        }
        self.iam_repository_name(repo_id).map(GarTarget::Project)
    }

    /// Whether a grant names nothing but this binding's own pull and push accounts.
    fn names_only_own_accounts(&self, access: &GcpCrossAccountAccess) -> bool {
        let own: Vec<String> = [
            &self.pull_service_account_email,
            &self.push_service_account_email,
        ]
        .into_iter()
        .flatten()
        .map(|email| format!("serviceAccount:{email}"))
        .collect();
        let members = cross_account_members(access);
        !members.is_empty() && members.iter().all(|member| own.contains(member))
    }

    /// A grant on one name would reach every name's images, so it is refused, never widened.
    fn per_name_access_refused(&self, operation: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: operation.to_string(),
            reason: format!(
                "per-repository access grants aren't supported on GCP: the names of binding '{}' are image paths in the one GAR repository '{}', so a grant would cover every name's images. Use a separate artifact-registry binding per tenant",
                self.binding_name, self.repository_name
            ),
        })
    }

    /// Observe GAR's asynchronous operation; failed reads return immediately.
    async fn await_repository_operation(
        &self,
        repository: &str,
        mut operation: Operation,
        operation_context: &str,
    ) -> Result<()> {
        for _ in 0..60 {
            if operation.done == Some(true) {
                if let Some(OperationResult::Error { error }) = operation.result {
                    return Err(AlienError::new(ErrorData::RemoteResourceConflict {
                        operation_context: operation_context.to_string(),
                        resource_type: "repository".to_string(),
                        resource_name: repository.to_string(),
                        conflict_reason: format!(
                            "GAR repository operation failed with code {}",
                            error.code
                        ),
                    }));
                }
                return Ok(());
            }
            let name = operation.name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::RemoteResourceConflict {
                    operation_context: operation_context.to_string(),
                    resource_type: "repository".to_string(),
                    resource_name: repository.to_string(),
                    conflict_reason: "repository operation has no name".to_string(),
                })
            })?;
            tokio::time::sleep(OPERATION_POLL).await;
            operation = self
                .client
                .get_operation(self.project_id.clone(), self.location.clone(), name)
                .await
                .map_err(|error| {
                    map_cloud_client_error(
                        error,
                        format!("Failed to observe {operation_context}"),
                        Some(repository.to_string()),
                    )
                })?;
        }
        Err(AlienError::new(ErrorData::RemoteResourceConflict {
            operation_context: operation_context.to_string(),
            resource_type: "repository".to_string(),
            resource_name: repository.to_string(),
            conflict_reason: "repository operation did not finish within 60 seconds".to_string(),
        }))
    }

    /// The proxy accounts may have access only to previously provisioned repositories.
    async fn ensure_proxy_access(&self, repository: &str) -> Result<()> {
        if self.pull_service_account_email.is_none() && self.push_service_account_email.is_none() {
            return Ok(());
        }
        let mut policy = self
            .client
            .get_repository_iam_policy(
                self.project_id.clone(),
                self.location.clone(),
                repository.to_string(),
            )
            .await
            .map_err(|error| {
                map_cloud_client_error(
                    error,
                    "Failed to read image repository proxy access".to_string(),
                    Some(repository.to_string()),
                )
            })?;
        let mut changed = false;
        for (role, account) in [
            (
                "roles/artifactregistry.reader",
                &self.pull_service_account_email,
            ),
            (
                "roles/artifactregistry.writer",
                &self.push_service_account_email,
            ),
        ] {
            let Some(account) = account else { continue };
            let member = format!("serviceAccount:{account}");
            if let Some(binding) = policy
                .bindings
                .iter_mut()
                .find(|binding| binding.role == role && binding.condition.is_none())
            {
                if !binding.members.contains(&member) {
                    binding.members.push(member);
                    changed = true;
                }
            } else {
                policy.bindings.push(alien_gcp_clients::iam::Binding {
                    role: role.to_string(),
                    members: vec![member],
                    condition: None,
                });
                changed = true;
            }
        }
        if changed {
            self.client
                .set_repository_iam_policy(
                    self.project_id.clone(),
                    self.location.clone(),
                    repository.to_string(),
                    policy,
                )
                .await
                .map_err(|error| {
                    map_cloud_client_error(
                        error,
                        "Failed to grant image repository proxy access".to_string(),
                        Some(repository.to_string()),
                    )
                })?;
        }
        Ok(())
    }

    /// Whether reading the repository itself says it does not exist. Any other answer, a denial
    /// included, is not proof that it is gone.
    async fn repository_is_gone(&self, repository: &str) -> bool {
        matches!(
            self.client
                .get_repository(
                    self.project_id.clone(),
                    self.location.clone(),
                    repository.to_string(),
                )
                .await,
            Err(error) if error.http_status_code == Some(404)
        )
    }

    /// Internal helper to add or remove members from IAM policy bindings
    async fn update_policy_members(
        &self,
        repo_name: &str,
        mut current_policy: IamPolicy,
        members: Vec<String>,
        add_members: bool, // true to add, false to remove
    ) -> Result<IamPolicy> {
        let reader_role = "roles/artifactregistry.reader";

        // Find or create the artifactregistry.reader binding
        let mut binding_index = None;
        for (i, binding) in current_policy.bindings.iter().enumerate() {
            if binding.role == reader_role && binding.condition.is_none() {
                binding_index = Some(i);
                break;
            }
        }

        if add_members {
            // Add members
            if members.is_empty() {
                info!(repo_name = %repo_name, "No new members to add");
                return Ok(current_policy);
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
            let member_count = |policy: &IamPolicy| -> usize {
                policy
                    .bindings
                    .iter()
                    .map(|binding| binding.members.len())
                    .sum()
            };
            let members_before = member_count(&current_policy);
            // Remove the selected members from every reader binding, retaining other
            // members and their conditions. A conditional binding is not a new grant target.
            for binding in &mut current_policy.bindings {
                if binding.role == reader_role {
                    binding.members.retain(|member| {
                        // IAM rewrites an account deleted during setup teardown with its UID.
                        let principal = member
                            .strip_prefix("deleted:")
                            .and_then(|deleted| deleted.split_once("?uid="))
                            .map_or(member.as_str(), |(principal, _)| principal);
                        !members.iter().any(|selected| selected == principal)
                    });
                }
            }
            current_policy
                .bindings
                .retain(|binding| !binding.members.is_empty());
            // Nothing to remove writes nothing: every GCP teardown asks the shared repository,
            // and most hold none of its members.
            if member_count(&current_policy) == members_before {
                info!(repo_name = %repo_name, "No members to remove from GCP Artifact Registry repository");
                return Ok(current_policy);
            }
        }

        // Set the updated policy with the original etag for optimistic concurrency control
        let updated = self.client.set_repository_iam_policy(
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
        Ok(updated)
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
        // An image path inside the binding's repository: GAR creates it on first push, so
        // nothing is created here and the stack needs no permission outside that repository.
        // OCI path components: lowercase alphanumerics joined by '.', '_' or '-'.
        let valid = repo_name.split('/').all(|component| {
            let bytes = component.as_bytes();
            let alphanumeric = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
            bytes.first().is_some_and(alphanumeric)
                && bytes.last().is_some_and(alphanumeric)
                && bytes
                    .iter()
                    .all(|byte| alphanumeric(byte) || b"._-".contains(byte))
        });
        if !valid {
            return Err(AlienError::new(ErrorData::InvalidInput {
                operation_context: "Creating a GAR image repository".to_string(),
                details: format!(
                    "'{repo_name}' is not a valid image path: use lowercase letters, digits, '.', '_', '-' and '/'"
                ),
                field_name: Some("repo_name".to_string()),
            }));
        }
        Ok(RepositoryResponse {
            name: format!("{}/{repo_name}", self.upstream_repository_prefix()),
            uri: None,
            created_at: None,
        })
    }

    async fn create_project_repository(&self, project_id: &str) -> Result<RepositoryResponse> {
        let repository = self.project_repository_name(project_id)?;
        let existing = self
            .client
            .get_repository(
                self.project_id.clone(),
                self.location.clone(),
                repository.clone(),
            )
            .await;
        match existing {
            Ok(_) => {}
            Err(error) if error.http_status_code == Some(404) => {
                let result = self
                    .client
                    .create_repository(
                        self.project_id.clone(),
                        self.location.clone(),
                        repository.clone(),
                        Repository {
                            format: Some(RepositoryFormat::Docker),
                            ..Default::default()
                        },
                    )
                    .await;
                match result {
                    Ok(operation) => {
                        self.await_repository_operation(
                            &repository,
                            operation,
                            "creating image repository",
                        )
                        .await?
                    }
                    Err(error) if error.http_status_code == Some(409) => {}
                    Err(error) => {
                        return Err(map_cloud_client_error(
                            error,
                            "Failed to create project image repository".to_string(),
                            Some(repository.clone()),
                        ))
                    }
                }
                // A concurrent creator may still be finishing. Never return an unusable push path.
                self.client
                    .get_repository(
                        self.project_id.clone(),
                        self.location.clone(),
                        repository.clone(),
                    )
                    .await
                    .map_err(|error| {
                        map_cloud_client_error(
                            error,
                            "Project image repository is not ready".to_string(),
                            Some(repository.clone()),
                        )
                    })?;
            }
            Err(error) => {
                return Err(map_cloud_client_error(
                    error,
                    "Failed to read project image repository".to_string(),
                    Some(repository.clone()),
                ))
            }
        }
        self.ensure_proxy_access(&repository).await?;
        Ok(RepositoryResponse {
            name: format!("{}/{repository}/{project_id}", self.project_id),
            uri: None,
            created_at: None,
        })
    }

    async fn get_repository(&self, repo_id: &str) -> Result<RepositoryResponse> {
        let name = match self.target(repo_id)? {
            GarTarget::Project(name) => name,
            GarTarget::Binding { path } => {
                // Image paths are implicit; the routable name and its pull URI are all there is.
                let path = path.map(|path| format!("/{path}")).unwrap_or_default();
                return Ok(RepositoryResponse {
                    name: format!("{}{path}", self.upstream_repository_prefix()),
                    uri: Some(format!(
                        "{}-docker.pkg.dev/{}/{}{path}",
                        self.location, self.project_id, self.repository_name
                    )),
                    created_at: None,
                });
            }
        };
        let repository = self
            .client
            .get_repository(self.project_id.clone(), self.location.clone(), name.clone())
            .await
            .map_err(|error| {
                map_cloud_client_error(
                    error,
                    "Failed to read image repository".to_string(),
                    Some(name.clone()),
                )
            })?;
        // The image path the caller named, e.g. `prj_a` in `vendor/artifacts-prj-a/prj_a`.
        let image_path = repo_id
            .strip_prefix(&format!("{}/", self.project_id))
            .unwrap_or(repo_id)
            .split_once('/')
            .map(|(_, path)| format!("/{path}"))
            .unwrap_or_default();
        Ok(RepositoryResponse {
            name: repo_id.to_string(),
            uri: Some(format!(
                "{}-docker.pkg.dev/{}/{name}{image_path}",
                self.location, self.project_id
            )),
            created_at: repository.create_time,
        })
    }

    async fn add_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<()> {
        let gcp_access = match access {
            CrossAccountAccess::Gcp(gcp_access) => gcp_access,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    env_var: binding_env_var(&self.binding_name),
                    binding_name: self.binding_name.clone(),
                    reason: "GCP artifact registry can only accept GCP cross-account access configuration".to_string(),
                }));
            }
        };
        let repo_name = match self.target(repo_id)? {
            GarTarget::Project(repository) => repository,
            GarTarget::Binding { .. } if self.names_only_own_accounts(&gcp_access) => {
                self.repository_name.clone()
            }
            GarTarget::Binding { .. } => {
                return Err(self.per_name_access_refused("add_cross_account_access"))
            }
        };

        info!(
            repo_name = %repo_name,
            project_numbers = ?gcp_access.project_numbers,
            allowed_service_types = ?gcp_access.allowed_service_types,
            service_account_emails = ?gcp_access.service_account_emails,
            "Adding GCP Artifact Registry repository cross-account access"
        );

        // Only a repository that is gone has no policy. Any other failure read as an empty one
        // writes back a policy holding this deployment's members alone, with no etag to lose the
        // race on — dropping every other binding on a repository all deployments share.
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
            Err(error) if error.http_status_code == Some(404) => IamPolicy {
                version: Some(1),
                kind: None,
                resource_id: None,
                bindings: vec![],
                etag: None,
            },
            Err(error) => {
                return Err(map_cloud_client_error(
                    error,
                    format!(
                        "Failed to read the IAM policy of GCP Artifact Registry repository '{repo_name}' to grant access"
                    ),
                    Some(repo_name.to_string()),
                ))
            }
        };

        // Two writes, not one: a sandbox agent that has never run doesn't exist yet, and Google
        // refuses a binding naming an unresolvable principal. One update naming both would lose
        // the worker grant to the sandbox agent's absence; splitting keeps a refusal there off it.
        let (deferred, present): (Vec<ComputeServiceType>, Vec<ComputeServiceType>) = gcp_access
            .allowed_service_types
            .iter()
            .cloned()
            .partition(is_agent_created_on_first_use);

        let policy = self
            .update_policy_members(
                &repo_name,
                current_policy,
                cross_account_members(&GcpCrossAccountAccess {
                    allowed_service_types: present,
                    ..gcp_access.clone()
                }),
                true,
            )
            .await?;

        if deferred.is_empty() {
            return Ok(());
        }
        // The second write is only safe behind the etag the first one returned. Without it the
        // write carries no concurrency check and replaces a policy every deployment shares, so the
        // reconcile retries instead.
        if policy.etag.is_none() {
            return Err(AlienError::new(ErrorData::RemoteResourceConflict {
                operation_context: "adding cross-account access".to_string(),
                resource_type: "repository".to_string(),
                resource_name: repo_name.clone(),
                conflict_reason:
                    "the updated IAM policy carries no etag, so the sandbox agent's member cannot be added without replacing the policy"
                        .to_string(),
            }));
        }
        self.update_policy_members(
            &repo_name,
            policy,
            cross_account_members(&GcpCrossAccountAccess {
                allowed_service_types: deferred,
                service_account_emails: Vec::new(),
                ..gcp_access
            }),
            true,
        )
        .await?;
        Ok(())
    }

    async fn remove_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<()> {
        let gcp_access = match access {
            CrossAccountAccess::Gcp(gcp_access) => gcp_access,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    env_var: binding_env_var(&self.binding_name),
                    binding_name: self.binding_name.clone(),
                    reason: "GCP artifact registry can only accept GCP cross-account access configuration".to_string(),
                }));
            }
        };
        // Revoking on one name would take the members off every name, so only the whole
        // repository, or the binding's own accounts, can be named.
        let repo_name = match self.target(repo_id)? {
            GarTarget::Project(repository) => repository,
            GarTarget::Binding { path: None } => self.repository_name.clone(),
            GarTarget::Binding { .. } if self.names_only_own_accounts(&gcp_access) => {
                self.repository_name.clone()
            }
            GarTarget::Binding { .. } => {
                return Err(self.per_name_access_refused("remove_cross_account_access"))
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
            // Only a repository that is gone carries no members. Reading a 403 or a timeout as
            // "nothing to remove" reports the revoke as done and strands every member on it, with
            // the state that named them deleted right after.
            Err(error) if error.http_status_code == Some(404) => {
                info!(repo_name = %repo_name, "No existing GCP IAM policy to remove permissions from");
                return Ok(());
            }
            Err(error) => {
                // GAR answers getIamPolicy on a repository that does not exist with 403, even to
                // the project owner. Every GCP teardown revokes on the shared repository, which a
                // manager set up after per-project repositories never had, so only a 404 from
                // reading the repository itself tells a gone repository from a denied read.
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteAccessDenied { .. })
                ) && self.repository_is_gone(&repo_name).await
                {
                    info!(repo_name = %repo_name, "GCP Artifact Registry repository is gone; no access to remove");
                    return Ok(());
                }
                return Err(map_cloud_client_error(
                    error,
                    format!(
                        "Failed to read the IAM policy of GCP Artifact Registry repository '{repo_name}' to revoke access"
                    ),
                    Some(repo_name.to_string()),
                ));
            }
        };

        let members_to_remove = cross_account_members(&gcp_access);
        self.update_policy_members(&repo_name, current_policy, members_to_remove, false)
            .await?;
        Ok(())
    }

    async fn get_cross_account_access(&self, repo_id: &str) -> Result<CrossAccountPermissions> {
        let repo_name = match self.target(repo_id)? {
            GarTarget::Project(repository) => repository,
            GarTarget::Binding { .. } => self.repository_name.clone(),
        };

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
                        match agent_member(service_account) {
                            Some((service_type, project_number)) => {
                                project_numbers.push(project_number.to_string());
                                if !allowed_service_types.contains(&service_type) {
                                    allowed_service_types.push(service_type);
                                }
                            }
                            None => service_account_emails.push(service_account.to_string()),
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
                        env_var: binding_env_var(&self.binding_name),
                        binding_name: self.binding_name.clone(),
                        reason: "Pull service account email not available - ensure the artifact registry resource is properly linked".to_string(),
                    }))?
            }
            ArtifactRegistryPermissions::PushPull => {
                self.push_service_account_email.clone()
                    .ok_or_else(|| AlienError::new(ErrorData::BindingConfigInvalid {
                        env_var: binding_env_var(&self.binding_name),
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
        let repository = match self.target(repo_id)? {
            GarTarget::Project(repository) => repository,
            GarTarget::Binding { .. } => {
                // An image path is not a GAR resource, and the binding's repository belongs to
                // alien-infra, which deletes it with the stack.
                debug!(repo_id = %repo_id, "GAR image path has no repository to delete");
                return Ok(());
            }
        };
        match self
            .client
            .delete_repository(
                self.project_id.clone(),
                self.location.clone(),
                repository.clone(),
            )
            .await
        {
            Ok(operation) => {
                self.await_repository_operation(&repository, operation, "deleting image repository")
                    .await
            }
            Err(error) if error.http_status_code == Some(404) => Ok(()),
            Err(error) => Err(map_cloud_client_error(
                error,
                "Failed to delete project image repository".to_string(),
                Some(repository),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{GcpCredentials, GcpServiceOverrides};
    use axum::{
        extract::State,
        http::{Method, StatusCode, Uri},
        response::IntoResponse,
        Json, Router,
    };
    use serde_json::{json, Value};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    /// Pins the member spelling, not the agent domains themselves: both sides of this assertion
    /// come from the same constants, so a wrong domain fails against GCP rather than here. What it
    /// does catch is a member built for the wrong project, in the wrong order, or silently dropped.
    #[test]
    fn each_service_type_resolves_to_its_own_service_agent() {
        let members = cross_account_members(&GcpCrossAccountAccess {
            project_numbers: vec!["123456789012".to_string()],
            allowed_service_types: vec![ComputeServiceType::Worker, ComputeServiceType::Sandbox],
            service_account_emails: vec![
                "management@test-project.iam.gserviceaccount.com".to_string()
            ],
        });

        assert_eq!(
            members,
            vec![
                "serviceAccount:service-123456789012@serverless-robot-prod.iam.gserviceaccount.com",
                "serviceAccount:service-123456789012@gcp-sa-vertex-sandbox.iam.gserviceaccount.com",
                "serviceAccount:management@test-project.iam.gserviceaccount.com",
            ]
        );
    }

    /// Granting and revoking read one list, so every service type survives a write then a read.
    /// The read path reports what the write path emitted. Without this a sandbox member decodes as
    /// an ordinary service account, so a caller reading current permissions is told the sandbox has
    /// no grant while its member sits on the policy.
    #[test]
    fn every_service_type_survives_a_write_then_read() {
        for service_type in [ComputeServiceType::Worker, ComputeServiceType::Sandbox] {
            let written = cross_account_members(&GcpCrossAccountAccess {
                project_numbers: vec!["123456789012".to_string()],
                allowed_service_types: vec![service_type.clone()],
                service_account_emails: Vec::new(),
            });
            let member = written
                .first()
                .expect("a project and a type name one member");
            let account = member
                .strip_prefix("serviceAccount:")
                .expect("members carry the IAM principal prefix");
            // Through the parser the read path uses, so dropping a type there fails this.
            assert_eq!(
                agent_member(account),
                Some((service_type.clone(), "123456789012")),
                "{service_type:?} must decode back to the project that earned it"
            );
        }
    }

    /// The sandbox agent does not exist until a project's first session, and Google refuses a
    /// binding naming a principal it cannot resolve. The two writes keep that refusal off the
    /// worker's grant, so the member sets must not overlap.
    #[test]
    fn the_worker_and_sandbox_members_are_written_apart() {
        let access = GcpCrossAccountAccess {
            project_numbers: vec!["123456789012".to_string()],
            allowed_service_types: vec![ComputeServiceType::Worker, ComputeServiceType::Sandbox],
            service_account_emails: vec![
                "management@test-project.iam.gserviceaccount.com".to_string()
            ],
        };
        let first = cross_account_members(&GcpCrossAccountAccess {
            allowed_service_types: vec![ComputeServiceType::Worker],
            ..access.clone()
        });
        let second = cross_account_members(&GcpCrossAccountAccess {
            allowed_service_types: vec![ComputeServiceType::Sandbox],
            service_account_emails: Vec::new(),
            ..access.clone()
        });

        assert!(second
            .iter()
            .all(|member| member.contains("gcp-sa-vertex-sandbox")));
        assert!(first
            .iter()
            .all(|member| !member.contains("gcp-sa-vertex-sandbox")));
        // Together they are the whole grant, so splitting the write loses no member.
        let mut both = [first, second].concat();
        both.sort();
        let mut whole = cross_account_members(&access);
        whole.sort();
        assert_eq!(both, whole);
    }

    /// A service type that names no project contributes no member to either list.
    #[test]
    fn a_service_type_grants_nothing_for_a_project_it_does_not_name() {
        // The service account is what separates this from the whole function returning nothing:
        // with it empty too, deleting the per-type loop would leave the assertion passing.
        let members = cross_account_members(&GcpCrossAccountAccess {
            project_numbers: Vec::new(),
            allowed_service_types: vec![ComputeServiceType::Sandbox],
            service_account_emails: vec![
                "management@test-project.iam.gserviceaccount.com".to_string()
            ],
        });

        assert_eq!(
            members,
            vec!["serviceAccount:management@test-project.iam.gserviceaccount.com"]
        );
    }
    /// A stack's binding names one repository its setup already granted. Its names are image
    /// paths there: creating one makes no GAR call, its pull path is in that repository, and its
    /// own accounts can be granted on it. A grant to anyone else on one name would open every
    /// name's images, so it is refused and nothing is written.
    #[tokio::test]
    async fn binding_names_are_image_paths_in_its_one_repository() {
        async fn api(
            State(calls): State<Arc<Mutex<Vec<String>>>>,
            method: Method,
            uri: Uri,
            body: String,
        ) -> impl IntoResponse {
            let path = uri.path().to_string();
            calls.lock().unwrap().push(format!("{method} {path}"));
            if path.ends_with(":getIamPolicy") {
                return (
                    StatusCode::OK,
                    Json(json!({"etag":"original", "bindings":[]})),
                );
            }
            if path.ends_with(":setIamPolicy") {
                let input: Value = serde_json::from_str(&body).unwrap();
                return (StatusCode::OK, Json(input["policy"].clone()));
            }
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error":{"code":404,"message":"no such repository"}})),
            )
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(api).with_state(calls.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = GcpClientConfig {
            project_id: "customer".to_string(),
            region: "region".to_string(),
            project_number: None,
            credentials: GcpCredentials::AccessToken {
                token: "synthetic-local-token".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([("artifactregistry".to_string(), endpoint)]),
            }),
        };
        let binding = |repository_name: &str| {
            ArtifactRegistryBinding::gar(
                repository_name.to_string(),
                Some("stack-pull@customer.iam.gserviceaccount.com".to_string()),
                Some("stack-push@customer.iam.gserviceaccount.com".to_string()),
            )
        };
        let registry =
            GarArtifactRegistry::new("images".to_string(), binding("stack-images"), &config)
                .await
                .unwrap();

        let created = registry.create_repository("prj_tenant1").await;
        assert!(
            calls.lock().unwrap().is_empty(),
            "creating a name calls no GAR API: {:?}",
            calls.lock().unwrap()
        );
        let created = created.unwrap();
        assert_eq!(created.name, "customer/stack-images/prj_tenant1");
        // Terraform setups pass the repository's full resource ID; the names are the same.
        let from_terraform = GarArtifactRegistry::new(
            "images".to_string(),
            binding("projects/customer/locations/region/repositories/stack-images"),
            &config,
        )
        .await
        .unwrap();
        assert_eq!(
            from_terraform
                .create_repository("prj_tenant1")
                .await
                .unwrap()
                .name,
            created.name
        );
        let repository = registry.get_repository(&created.name).await.unwrap();
        assert_eq!(repository.name, created.name);
        assert_eq!(
            repository.uri.as_deref(),
            Some("region-docker.pkg.dev/customer/stack-images/prj_tenant1")
        );
        assert!(registry.create_repository("Tenant").await.is_err());
        for invalid in ["../other", "a//b", "/a", "a/", "a/-b", ""] {
            assert!(
                registry.create_repository(invalid).await.is_err(),
                "{invalid:?}"
            );
        }
        assert!(registry.create_repository("team-a/app.v2_x").await.is_ok());

        // The stack's own accounts may be granted on the repository that holds the name.
        let own = CrossAccountAccess::Gcp(GcpCrossAccountAccess {
            project_numbers: Vec::new(),
            allowed_service_types: Vec::new(),
            service_account_emails: vec!["stack-pull@customer.iam.gserviceaccount.com".to_string()],
        });
        registry
            .add_cross_account_access(&created.name, own.clone())
            .await
            .unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "GET /projects/customer/locations/region/repositories/stack-images:getIamPolicy",
                "POST /projects/customer/locations/region/repositories/stack-images:setIamPolicy",
            ]
        );
        registry
            .remove_cross_account_access(&created.name, own)
            .await
            .unwrap();
        calls.lock().unwrap().clear();

        // Anyone else, alone or beside the stack's accounts, is refused on a name and nothing is
        // written: GAR would grant the whole repository.
        for access in [
            GcpCrossAccountAccess {
                project_numbers: vec!["123456789012".to_string()],
                allowed_service_types: vec![ComputeServiceType::Worker],
                service_account_emails: Vec::new(),
            },
            GcpCrossAccountAccess {
                project_numbers: Vec::new(),
                allowed_service_types: Vec::new(),
                service_account_emails: vec![
                    "stack-pull@customer.iam.gserviceaccount.com".to_string(),
                    "tenant@elsewhere.iam.gserviceaccount.com".to_string(),
                ],
            },
        ] {
            let access = CrossAccountAccess::Gcp(access);
            for result in [
                registry
                    .add_cross_account_access(&created.name, access.clone())
                    .await,
                registry
                    .add_cross_account_access("customer/stack-images", access.clone())
                    .await,
                registry
                    .remove_cross_account_access(&created.name, access.clone())
                    .await,
            ] {
                let error = result.unwrap_err();
                assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
                assert!(!error.retryable);
                assert!(error
                    .to_string()
                    .contains("per-repository access grants aren't supported on GCP"));
            }
        }
        assert!(
            calls.lock().unwrap().is_empty(),
            "{:?}",
            calls.lock().unwrap()
        );

        // Deleting a name deletes no repository.
        registry.delete_repository(&created.name).await.unwrap();
        assert!(
            calls.lock().unwrap().is_empty(),
            "{:?}",
            calls.lock().unwrap()
        );
        server.abort();
    }

    /// GAR answers getIamPolicy on a repository that does not exist with 403 PERMISSION_DENIED,
    /// even to the project owner (checked against the real API); only reading the repository
    /// itself returns 404. A manager without the shared repository revokes there on every GCP
    /// teardown, so that 403 must not fail the delete, while a real denial still must.
    #[tokio::test]
    async fn a_revoke_on_a_missing_repository_succeeds_and_a_denied_read_still_fails() {
        #[derive(Default)]
        struct ApiState {
            policy_writes: usize,
        }
        async fn api(State(state): State<Arc<Mutex<ApiState>>>, uri: Uri) -> impl IntoResponse {
            let repository = uri.path().rsplit('/').next().unwrap();
            if repository.ends_with(":setIamPolicy") {
                state.lock().unwrap().policy_writes += 1;
                return (StatusCode::OK, Json(json!({})));
            }
            if repository.ends_with(":getIamPolicy") {
                return (
                    StatusCode::FORBIDDEN,
                    Json(
                        json!({"error":{"code":403,"message":"The caller does not have permission","status":"PERMISSION_DENIED"}}),
                    ),
                );
            }
            if repository == "artifacts-prj-a" {
                return (
                    StatusCode::OK,
                    Json(json!({"name": repository, "format": "DOCKER"})),
                );
            }
            (
                StatusCode::NOT_FOUND,
                Json(
                    json!({"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND"}}),
                ),
            )
        }
        let state = Arc::new(Mutex::new(ApiState::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(api).with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = GcpClientConfig {
            project_id: "vendor".to_string(),
            region: "region".to_string(),
            project_number: None,
            credentials: GcpCredentials::AccessToken {
                token: "synthetic-local-token".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([("artifactregistry".to_string(), endpoint)]),
            }),
        };
        let registry = GarArtifactRegistry {
            client: ArtifactRegistryClient::new(
                reqwest::Client::builder().no_proxy().build().unwrap(),
                config.clone(),
            ),
            binding_name: "artifacts".to_string(),
            project_id: "vendor".to_string(),
            location: "region".to_string(),
            repository_name: "artifacts".to_string(),
            pull_service_account_email: None,
            push_service_account_email: None,
            gcp_config: config,
        };
        let access = CrossAccountAccess::Gcp(GcpCrossAccountAccess {
            project_numbers: vec!["123456789012".to_string()],
            allowed_service_types: vec![ComputeServiceType::Worker],
            service_account_emails: vec![],
        });

        // The shared repository was never created.
        registry
            .remove_cross_account_access("vendor/artifacts", access.clone())
            .await
            .expect("a repository that does not exist holds nothing to revoke");

        // The project repository exists, so its 403 is a real denial.
        let denied = registry
            .remove_cross_account_access("vendor/artifacts-prj-a/prj_a", access)
            .await
            .expect_err("a denied policy read must not count as revoked");
        assert_eq!(denied.code, "REMOTE_ACCESS_DENIED");
        assert_eq!(state.lock().unwrap().policy_writes, 0);
        server.abort();
    }

    #[tokio::test]
    async fn project_provisioning_and_reader_changes_use_distinct_gar_resources() {
        #[derive(Default)]
        struct ApiState {
            repositories: HashMap<String, Value>,
            policies: HashMap<String, Value>,
            writes: Vec<String>,
            pending_deletion: Option<String>,
            deletion_fails: bool,
            policy_write_fails: bool,
        }
        async fn api(
            State(state): State<Arc<Mutex<ApiState>>>,
            method: Method,
            uri: Uri,
            body: String,
        ) -> impl IntoResponse {
            let mut state = state.lock().unwrap();
            let path = uri.path();
            if path.contains("/operations/") {
                if path.ends_with("delete-project-repository") {
                    let repository = state.pending_deletion.take().expect("delete was requested");
                    if state.deletion_fails {
                        return (
                            StatusCode::OK,
                            Json(
                                json!({"name":path, "done":true, "error":{"code":9,"message":"repository busy"}}),
                            ),
                        );
                    }
                    state.repositories.remove(&repository);
                }
                return (StatusCode::OK, Json(json!({"name":path, "done":true})));
            }
            if method == Method::POST && path.ends_with("/repositories") {
                let name = uri
                    .query()
                    .unwrap()
                    .strip_prefix("repositoryId=")
                    .unwrap()
                    .to_string();
                let input: Value = serde_json::from_str(&body).unwrap();
                assert_eq!(input["format"], "DOCKER");
                state
                    .repositories
                    .insert(name.clone(), json!({"name": name, "format": "DOCKER"}));
                return (
                    StatusCode::OK,
                    Json(
                        json!({"name":"projects/vendor/locations/region/operations/create-project-repository", "done":false}),
                    ),
                );
            }
            let repository = path.rsplit('/').next().unwrap();
            if let Some(name) = repository.strip_suffix(":getIamPolicy") {
                assert_eq!(uri.query(), Some("options.requestedPolicyVersion=3"));
                return (StatusCode::OK, Json(state.policies.get(name).cloned().unwrap_or(json!({"etag":"original", "bindings":[{"role":"roles/viewer", "members":["user:keep@example.com"]}]}))));
            }
            if let Some(name) = repository.strip_suffix(":setIamPolicy") {
                if state.policy_write_fails {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(json!({"error":{"code":403,"message":"IAM write denied"}})),
                    );
                }
                let input: Value = serde_json::from_str(&body).unwrap();
                let mut policy = input["policy"].clone();
                let expected = state
                    .policies
                    .get(name)
                    .map(|p| p["etag"].clone())
                    .unwrap_or(json!("original"));
                assert_eq!(
                    policy["etag"], expected,
                    "IAM writes preserve concurrency checks"
                );
                policy["etag"] = json!(format!("revision-{}", state.writes.len()));
                state.policies.insert(name.to_string(), policy.clone());
                state.writes.push(name.to_string());
                return (StatusCode::OK, Json(policy));
            }
            if method == Method::DELETE && state.repositories.contains_key(repository) {
                state.pending_deletion = Some(repository.to_string());
                return (
                    StatusCode::OK,
                    Json(
                        json!({"name":"projects/vendor/locations/region/operations/delete-project-repository", "done":false}),
                    ),
                );
            }
            match state.repositories.get(repository) {
                Some(repository) => (StatusCode::OK, Json(repository.clone())),
                None => (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error":{"code":404,"message":"repository absent"}})),
                ),
            }
        }
        let state = Arc::new(Mutex::new(ApiState::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(api).with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = GcpClientConfig {
            project_id: "vendor".to_string(),
            region: "region".to_string(),
            project_number: None,
            credentials: GcpCredentials::AccessToken {
                token: "synthetic-local-token".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([("artifactregistry".to_string(), endpoint)]),
            }),
        };
        let registry = GarArtifactRegistry {
            client: ArtifactRegistryClient::new(
                reqwest::Client::builder().no_proxy().build().unwrap(),
                config.clone(),
            ),
            binding_name: "artifacts".to_string(),
            project_id: "vendor".to_string(),
            location: "region".to_string(),
            repository_name: "artifacts".to_string(),
            pull_service_account_email: None,
            push_service_account_email: None,
            gcp_config: config,
        };
        assert!(registry.create_project_repository("prj_A").await.is_err());
        assert_eq!(
            registry
                .create_project_repository("default")
                .await
                .unwrap()
                .name,
            "vendor/artifacts-default/default"
        );
        assert!(state
            .lock()
            .unwrap()
            .repositories
            .contains_key("artifacts-default"));
        let own = registry.create_project_repository("prj_b").await.unwrap();
        let other = registry.create_project_repository("prj_a").await.unwrap();
        assert_ne!(own.name, other.name);
        assert_eq!(
            registry
                .create_project_repository("prj_b")
                .await
                .unwrap()
                .name,
            own.name
        );
        let access = CrossAccountAccess::Gcp(GcpCrossAccountAccess {
            project_numbers: vec!["123456789012".to_string()],
            allowed_service_types: vec![ComputeServiceType::Worker, ComputeServiceType::Sandbox],
            service_account_emails: vec!["management@target.iam.gserviceaccount.com".to_string()],
        });
        registry
            .add_cross_account_access(&own.name, access.clone())
            .await
            .unwrap();
        let own_resource = own.name.split('/').nth(1).unwrap();
        {
            let state = state.lock().unwrap();
            assert!(state.writes.iter().all(|name| name == own_resource));
            assert_eq!(state.policies.len(), 1);
            let readers = state.policies[own_resource]["bindings"][1]["members"]
                .as_array()
                .unwrap();
            assert_eq!(readers.len(), 3);
            assert!(readers.contains(&json!(
                "serviceAccount:service-123456789012@serverless-robot-prod.iam.gserviceaccount.com"
            )));
            assert!(readers.contains(&json!(
                "serviceAccount:service-123456789012@gcp-sa-vertex-sandbox.iam.gserviceaccount.com"
            )));
        }
        registry
            .remove_cross_account_access(&own.name, access.clone())
            .await
            .unwrap();
        registry
            .remove_cross_account_access(&own.name, access.clone())
            .await
            .unwrap();
        {
            let state = state.lock().unwrap();
            assert_eq!(
                state.policies[own_resource]["bindings"],
                json!([{"role":"roles/viewer", "members":["user:keep@example.com"]}])
            );
            assert!(!state
                .policies
                .contains_key(other.name.split('/').nth(1).unwrap()));
        }
        for invalid in ["vendor/artifacts-shared", "vendor/artifacts-prj-b/prj_a"] {
            assert!(registry
                .add_cross_account_access(invalid, access.clone())
                .await
                .is_err());
            assert!(registry.get_repository(invalid).await.is_err());
            assert!(registry.delete_repository(invalid).await.is_err());
        }
        registry.delete_repository(&own.name).await.unwrap();
        assert!(!state
            .lock()
            .unwrap()
            .repositories
            .contains_key(own_resource));
        assert_eq!(
            registry
                .get_repository(&own.name)
                .await
                .unwrap_err()
                .http_status_code,
            Some(404)
        );
        registry.delete_repository(&own.name).await.unwrap();
        assert_eq!(
            registry
                .create_project_repository("prj_b")
                .await
                .unwrap()
                .name,
            own.name
        );
        state.lock().unwrap().deletion_fails = true;
        assert_eq!(
            registry
                .delete_repository(&other.name)
                .await
                .unwrap_err()
                .code,
            "REMOTE_RESOURCE_CONFLICT"
        );
        // The URI keeps the image path inside the project repository, where pushes land.
        assert_eq!(
            registry
                .get_repository(&other.name)
                .await
                .unwrap()
                .uri
                .as_deref(),
            Some("region-docker.pkg.dev/vendor/artifacts-prj-a/prj_a")
        );
        {
            let mut state = state.lock().unwrap();
            state.policies.insert("artifacts-prj-a".to_string(), json!({"etag":"legacy", "bindings":[
                {"role":"roles/artifactregistry.reader", "members":["serviceAccount:service-123456789012@serverless-robot-prod.iam.gserviceaccount.com", "user:keep@example.com", "deleted:serviceAccount:management@test-project.iam.gserviceaccount.com?uid=123", "deleted:serviceAccount:other@test-project.iam.gserviceaccount.com?uid=456"]},
                {"role":"roles/artifactregistry.reader", "members":["serviceAccount:service-123456789012@serverless-robot-prod.iam.gserviceaccount.com", "user:conditional@example.com"], "condition":{"title":"limited", "expression":"true"}},
                {"role":"roles/viewer", "members":["serviceAccount:service-123456789012@serverless-robot-prod.iam.gserviceaccount.com"]}
            ]}));
        }
        let cleanup = CrossAccountAccess::Gcp(GcpCrossAccountAccess {
            project_numbers: vec!["123456789012".to_string()],
            allowed_service_types: vec![ComputeServiceType::Worker, ComputeServiceType::Sandbox],
            service_account_emails: vec![
                "management@test-project.iam.gserviceaccount.com".to_string()
            ],
        });
        registry
            .remove_cross_account_access("vendor/artifacts-prj-a", cleanup.clone())
            .await
            .unwrap();
        registry
            .remove_cross_account_access("vendor/artifacts-prj-a", cleanup)
            .await
            .unwrap();
        assert_eq!(
            state.lock().unwrap().policies["artifacts-prj-a"]["bindings"][0]["members"],
            json!([
                "user:keep@example.com",
                "deleted:serviceAccount:other@test-project.iam.gserviceaccount.com?uid=456"
            ])
        );
        let policy = state.lock().unwrap().policies["artifacts-prj-a"].clone();
        assert_eq!(
            policy["bindings"][1]["members"],
            json!(["user:conditional@example.com"])
        );
        assert_eq!(
            policy["bindings"][1]["condition"],
            json!({"title":"limited", "expression":"true"})
        );
        assert_eq!(
            policy["bindings"][2]["members"],
            json!([
                "serviceAccount:service-123456789012@serverless-robot-prod.iam.gserviceaccount.com"
            ])
        );
        let registry = GarArtifactRegistry {
            pull_service_account_email: Some("pull@vendor.iam.gserviceaccount.com".to_string()),
            push_service_account_email: Some("push@vendor.iam.gserviceaccount.com".to_string()),
            ..registry
        };
        // Provisioning repairs existing repositories as well as granting new ones.
        for project in ["prj_a", "prj_c"] {
            let repository = format!("artifacts-{}", project.replace('_', "-"));
            let previous = state.lock().unwrap().policies.get(&repository).cloned();
            registry.create_project_repository(project).await.unwrap();
            let policy = state.lock().unwrap().policies[&repository].clone();
            for (role, member) in [
                (
                    "roles/artifactregistry.reader",
                    "serviceAccount:pull@vendor.iam.gserviceaccount.com",
                ),
                (
                    "roles/artifactregistry.writer",
                    "serviceAccount:push@vendor.iam.gserviceaccount.com",
                ),
            ] {
                let bindings = policy["bindings"].as_array().unwrap();
                assert!(bindings.iter().any(|binding| binding["role"] == role
                    && binding.get("condition").is_none()
                    && binding["members"]
                        .as_array()
                        .unwrap()
                        .contains(&json!(member))));
            }
            if let Some(previous) = previous {
                for binding in previous["bindings"].as_array().unwrap() {
                    let retained = policy["bindings"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|candidate| {
                            candidate["role"] == binding["role"]
                                && candidate.get("condition") == binding.get("condition")
                        })
                        .unwrap();
                    for member in binding["members"].as_array().unwrap() {
                        assert!(retained["members"].as_array().unwrap().contains(member));
                    }
                }
            }
            let writes = state.lock().unwrap().writes.len();
            registry.create_project_repository(project).await.unwrap();
            assert_eq!(
                state.lock().unwrap().writes.len(),
                writes,
                "unchanged policy is not rewritten"
            );
        }
        // A failed IAM write must not return a usable-looking repository path.
        state.lock().unwrap().policy_write_fails = true;
        assert_eq!(
            registry
                .create_project_repository("prj_d")
                .await
                .unwrap_err()
                .http_status_code,
            Some(403)
        );
        state.lock().unwrap().policy_write_fails = false;
        registry.create_project_repository("prj_d").await.unwrap();
        assert!(state
            .lock()
            .unwrap()
            .policies
            .contains_key("artifacts-prj-d"));
        server.abort();
    }
}
