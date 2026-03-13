//! Dev registry — persistent per-project artifact registry for `alien dev --platform <cloud>`.
//!
//! When running `alien dev` with a cloud platform, built container images need to be
//! pushed to a real container registry (ECR, GAR, ACR). This module provisions an
//! `ArtifactRegistry` resource via `StackExecutor` and uses bindings to generate
//! temporary Docker credentials.
//!
//! State is persisted in `.alien/dev-registry/{platform}-state.json`.

use alien_bindings::{
    providers::artifact_registry::{
        acr::AcrArtifactRegistry, ecr::EcrArtifactRegistry, gar::GarArtifactRegistry,
    },
    ArtifactRegistry as ArtifactRegistryTrait, ArtifactRegistryPermissions,
};
use alien_build::settings::PushSettings;
use alien_core::{
    ArtifactRegistry, ArtifactRegistryBinding, ArtifactRegistryOutputs, ClientConfig, Platform,
    ResourceLifecycle, ResourceStatus, Stack, StackState,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::{ClientConfigExt, StackExecutor};
use dockdash::{ClientProtocol, RegistryAuth};
use std::path::{Path, PathBuf};
use tracing::info;

use crate::error::ErrorData;

/// Compute a stable hash prefix from the project directory for isolation.
fn project_hash(project_dir: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    project_dir.to_string_lossy().hash(&mut hasher);
    format!("{:08x}", hasher.finish() & 0xFFFFFFFF)
}

fn registry_id(project_dir: &Path) -> String {
    format!("alien-dev-{}", project_hash(project_dir))
}

fn state_path(state_dir: &Path, platform: &str) -> PathBuf {
    state_dir
        .join("dev-registry")
        .join(format!("{}-state.json", platform))
}

fn load_stack_state(state_dir: &Path, platform: &str) -> Option<StackState> {
    let path = state_path(state_dir, platform);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn save_stack_state(
    state_dir: &Path,
    platform: &str,
    state: &StackState,
) -> crate::error::Result<()> {
    let path = state_path(state_dir, platform);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "create".to_string(),
                file_path: parent.display().to_string(),
                reason: "Failed to create dev-registry directory".to_string(),
            },
        )?;
    }
    let json = serde_json::to_string_pretty(state)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "Failed to serialize dev registry stack state".to_string(),
        })?;
    std::fs::write(&path, json)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to write dev registry stack state".to_string(),
        })
}

fn build_dev_registry_stack(project_dir: &Path) -> Stack {
    let id = registry_id(project_dir);
    let registry = ArtifactRegistry::new(id).build();

    Stack::new("dev-registry".to_string())
        .add_with_remote_access(registry, ResourceLifecycle::Frozen)
        .build()
}

/// Ensure the dev registry infrastructure is provisioned, returning the final StackState.
async fn ensure_provisioned(
    platform: &Platform,
    state_dir: &Path,
    project_dir: &Path,
) -> crate::error::Result<StackState> {
    let platform_str = platform.as_str();
    let reg_id = registry_id(project_dir);

    // Load cached state if available (allows resuming partial provisioning)
    let cached_state = load_stack_state(state_dir, platform_str);

    // If the resource is already Running, return immediately
    if let Some(ref existing) = cached_state {
        if let Some(resource) = existing.resources.get(&reg_id) {
            if resource.status == ResourceStatus::Running {
                info!("Dev registry already provisioned (cached state)");
                return Ok(existing.clone());
            }
        }
    }

    info!("Provisioning dev registry for platform: {}", platform_str);

    let stack = build_dev_registry_stack(project_dir);
    let client_config = ClientConfig::from_std_env(platform.clone()).await.context(
        ErrorData::ConfigurationError {
            message: format!(
                "Failed to build client config for platform {:?}. \
                 Ensure cloud credentials are configured.",
                platform
            ),
        },
    )?;

    let executor = StackExecutor::new(&stack, client_config, Some(vec![ResourceLifecycle::Frozen]))
        .context(ErrorData::ConfigurationError {
            message: "Failed to create stack executor for dev registry".to_string(),
        })?;

    // Resume from cached state if available, otherwise start fresh
    let state = cached_state.unwrap_or_else(|| StackState::new(platform.clone()));
    let result = executor.run_until_synced(state).await;

    // Persist state regardless of success (so partial progress is saved)
    save_stack_state(state_dir, platform_str, &result.final_state)?;

    let final_state = result
        .into_result()
        .context(ErrorData::ConfigurationError {
            message: "Failed to provision dev registry".to_string(),
        })?;

    info!("Dev registry provisioned successfully");
    Ok(final_state)
}

/// Build PushSettings from the provisioned StackState by generating temporary credentials.
async fn make_push_settings(
    platform: &Platform,
    state: &StackState,
    project_dir: &Path,
) -> crate::error::Result<PushSettings> {
    let reg_id = registry_id(project_dir);

    // Extract outputs
    let outputs = state
        .get_resource_outputs::<ArtifactRegistryOutputs>(&reg_id)
        .context(ErrorData::ConfigurationError {
            message: "Dev registry resource has no outputs".to_string(),
        })?;

    // Extract binding params
    let resource_state = state.resources.get(&reg_id).ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Dev registry resource '{}' not found in state", reg_id),
        })
    })?;

    let binding_params = resource_state
        .remote_binding_params
        .as_ref()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "Dev registry resource has no remote binding params".to_string(),
            })
        })?;

    let binding: ArtifactRegistryBinding = serde_json::from_value(binding_params.clone())
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "deserialize".to_string(),
            reason: "Failed to deserialize ArtifactRegistryBinding from state".to_string(),
        })?;

    // Build platform client config for credential generation
    let client_config = ClientConfig::from_std_env(platform.clone()).await.context(
        ErrorData::ConfigurationError {
            message: format!(
                "Failed to build client config for credential generation on {:?}",
                platform
            ),
        },
    )?;

    let binding_name = reg_id.clone();
    let endpoint = &outputs.registry_endpoint;

    // Build credentials and repository URL per platform.
    // The repository URL is {registry_endpoint}/{repository_path} where
    // the path component varies by platform.
    let (credentials, repository) = match platform {
        Platform::Aws => {
            // ECR: repository_prefix from binding is the ECR repo path
            let repo_prefix = match &binding {
                ArtifactRegistryBinding::Ecr(ecr) => match &ecr.repository_prefix {
                    alien_core::BindingValue::Value(v) => v.clone(),
                    _ => reg_id.clone(),
                },
                _ => reg_id.clone(),
            };
            let aws_config = client_config.aws_config().ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "Expected AWS client config".to_string(),
                })
            })?;
            let registry = EcrArtifactRegistry::new(binding_name, binding, aws_config)
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Failed to initialize ECR binding".to_string(),
                })?;
            let creds = registry
                .generate_credentials(&reg_id, ArtifactRegistryPermissions::PushPull, None)
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Failed to generate ECR credentials".to_string(),
                })?;
            (creds, format!("{}/{}", endpoint, repo_prefix))
        }
        Platform::Gcp => {
            // GAR: endpoint is {location}-docker.pkg.dev/{project_id},
            // append the resource id as the repository name
            let gcp_config = client_config.gcp_config().ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "Expected GCP client config".to_string(),
                })
            })?;
            let registry = GarArtifactRegistry::new(binding_name, binding, gcp_config)
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Failed to initialize GAR binding".to_string(),
                })?;
            let creds = registry
                .generate_credentials(&reg_id, ArtifactRegistryPermissions::PushPull, None)
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Failed to generate GAR credentials".to_string(),
                })?;
            (creds, format!("{}/{}", endpoint, reg_id))
        }
        Platform::Azure => {
            // ACR: endpoint is {registry}.azurecr.io, use directly
            let azure_config = client_config.azure_config().ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "Expected Azure client config".to_string(),
                })
            })?;
            let registry = AcrArtifactRegistry::new(binding_name, binding, azure_config)
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Failed to initialize ACR binding".to_string(),
                })?;
            let creds = registry
                .generate_credentials(&reg_id, ArtifactRegistryPermissions::PushPull, None)
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Failed to generate ACR credentials".to_string(),
                })?;
            (creds, endpoint.clone())
        }
        _ => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "Cannot generate credentials for platform: {}",
                    platform.as_str()
                ),
            }));
        }
    };

    Ok(PushSettings {
        repository,
        options: dockdash::PushOptions {
            auth: RegistryAuth::Basic(credentials.username, credentials.password),
            protocol: ClientProtocol::Https,
            ..Default::default()
        },
    })
}

fn parse_manual_auth(auth: &str) -> crate::error::Result<RegistryAuth> {
    if auth == "anonymous" {
        return Ok(RegistryAuth::Anonymous);
    }
    let parts: Vec<&str> = auth.splitn(3, ':').collect();
    if parts.len() == 3 && parts[0] == "basic" {
        Ok(RegistryAuth::Basic(
            parts[1].to_string(),
            parts[2].to_string(),
        ))
    } else {
        Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Invalid ALIEN_DEV_REGISTRY_AUTH format: '{}'. \
                 Expected 'anonymous' or 'basic:username:password'",
                auth
            ),
        }))
    }
}

/// Resolve push settings (registry endpoint + auth) for the dev registry.
pub async fn resolve_dev_push_settings(
    platform: &Platform,
    state_dir: &Path,
    project_dir: &Path,
) -> crate::error::Result<PushSettings> {
    // Manual override via env vars
    if let (Ok(endpoint), Ok(auth)) = (
        std::env::var("ALIEN_DEV_REGISTRY_ENDPOINT"),
        std::env::var("ALIEN_DEV_REGISTRY_AUTH"),
    ) {
        info!("Using manual dev registry override: {}", endpoint);
        let registry_auth = parse_manual_auth(&auth)?;
        return Ok(PushSettings {
            repository: endpoint,
            options: dockdash::PushOptions {
                auth: registry_auth,
                protocol: ClientProtocol::Https,
                ..Default::default()
            },
        });
    }

    match platform {
        Platform::Kubernetes => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "Kubernetes dev registry requires manual setup. \
                    Set ALIEN_DEV_REGISTRY_ENDPOINT and ALIEN_DEV_REGISTRY_AUTH environment variables."
                    .to_string(),
            }));
        }
        Platform::Aws | Platform::Gcp | Platform::Azure => {}
        _ => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: format!(
                    "Dev registry not supported for platform: {}",
                    platform.as_str()
                ),
            }));
        }
    }

    let state = ensure_provisioned(platform, state_dir, project_dir).await?;
    make_push_settings(platform, &state, project_dir).await
}
