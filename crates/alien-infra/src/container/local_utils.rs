//! Utilities for Local platform container binding transformations.
//!
//! This module handles two types of binding transformations needed when running
//! containers on the Local platform:
//!
//! 1. **Filesystem path rewriting** - Host paths → container mount paths
//! 2. **Network URL rewriting** - localhost → host.docker.internal

use crate::error::{ErrorData, Result};
use alien_core::bindings::{
    binding_env_var_name, ArtifactRegistryBinding, BindingValue, ContainerBinding, FunctionBinding,
    KvBinding, StorageBinding, VaultBinding,
};
use alien_error::{AlienError, Context, IntoAlienError};
use tracing::debug;

/// Rewrites a binding's filesystem path from host path to container path.
///
/// For containers, linked resources (Storage, KV, Vault) are mounted at different
/// paths inside the container. This function updates the binding env var to use
/// the container-internal path.
///
/// **Fails fast** if non-Local binding variants are encountered - this indicates
/// a bug in the deployment system (Local platform should only have Local bindings).
pub(super) fn rewrite_binding_path_for_container(
    env_vars: &mut std::collections::HashMap<String, String>,
    resource_id: &str,
    container_path: &str,
) -> Result<()> {
    let binding_key = binding_env_var_name(resource_id);
    let Some(binding_json) = env_vars.get_mut(&binding_key) else {
        return Ok(());
    };

    // Try each binding type - they're mutually exclusive

    // Storage binding: only Local variant allowed on Local platform
    if let Ok(mut binding) = serde_json::from_str::<StorageBinding>(binding_json) {
        match binding {
            StorageBinding::Local(ref mut local) => {
                local.storage_path = BindingValue::value(format!("file://{}/", container_path));
                let new_json = serde_json::to_string(&binding).into_alien_error().context(
                    ErrorData::ResourceControllerConfigError {
                        resource_id: resource_id.to_string(),
                        message: "Failed to serialize Storage binding".to_string(),
                    },
                )?;

                *binding_json = new_json;
                return Ok(());
            }
            _ => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: resource_id.to_string(),
                    message: "Local platform containers cannot use cloud storage bindings"
                        .to_string(),
                }));
            }
        }
    }

    // KV binding: only Local variant allowed on Local platform
    if let Ok(mut binding) = serde_json::from_str::<KvBinding>(binding_json) {
        match binding {
            KvBinding::Local(ref mut local) => {
                local.data_dir = BindingValue::value(container_path.to_string());
                let new_json = serde_json::to_string(&binding).into_alien_error().context(
                    ErrorData::ResourceControllerConfigError {
                        resource_id: resource_id.to_string(),
                        message: "Failed to serialize KV binding".to_string(),
                    },
                )?;

                *binding_json = new_json;
                return Ok(());
            }
            _ => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: resource_id.to_string(),
                    message: "Local platform containers cannot use cloud KV bindings".to_string(),
                }));
            }
        }
    }

    // Vault binding: only Local variant allowed on Local platform
    if let Ok(mut binding) = serde_json::from_str::<VaultBinding>(binding_json) {
        match binding {
            VaultBinding::Local(ref mut local) => {
                local.data_dir = BindingValue::value(container_path.to_string());
                let new_json = serde_json::to_string(&binding).into_alien_error().context(
                    ErrorData::ResourceControllerConfigError {
                        resource_id: resource_id.to_string(),
                        message: "Failed to serialize Vault binding".to_string(),
                    },
                )?;

                *binding_json = new_json;
                return Ok(());
            }
            _ => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: resource_id.to_string(),
                    message: "Local platform containers cannot use cloud vault bindings"
                        .to_string(),
                }));
            }
        }
    }

    Ok(())
}

/// Rewrites localhost URLs to host.docker.internal for network bindings.
///
/// When containers need to connect to services running on the host (like local
/// artifact registries), they cannot use "localhost" because that resolves to the
/// container itself. On Docker Desktop (Mac/Windows), `host.docker.internal` is a
/// special DNS name that resolves to the host.
///
/// **Fails fast** if non-Local binding variants are encountered - this indicates
/// a bug in the deployment system (Local platform should only have Local bindings).
pub(super) fn rewrite_localhost_urls_for_container(
    env_vars: &mut std::collections::HashMap<String, String>,
) -> Result<()> {
    // Find all binding environment variables
    let binding_keys: Vec<String> = env_vars
        .keys()
        .filter(|k| k.starts_with("ALIEN_") && k.ends_with("_BINDING"))
        .cloned()
        .collect();

    for binding_key in binding_keys {
        let Some(binding_json) = env_vars.get_mut(&binding_key) else {
            continue;
        };

        // ArtifactRegistry binding: only Local variant allowed
        if let Ok(mut binding) = serde_json::from_str::<ArtifactRegistryBinding>(binding_json) {
            match binding {
                ArtifactRegistryBinding::Local(ref mut local) => {
                    // Extract URL to avoid borrow conflicts
                    let needs_rewrite = if let BindingValue::Value(ref url) = local.registry_url {
                        url.contains("localhost:") || url.contains("127.0.0.1:")
                    } else {
                        false
                    };

                    if needs_rewrite {
                        if let BindingValue::Value(url) = &local.registry_url {
                            let original_url = url.clone();
                            let new_url = url
                                .replace("localhost:", "host.docker.internal:")
                                .replace("127.0.0.1:", "host.docker.internal:");

                            local.registry_url = BindingValue::value(new_url.clone());
                            let new_json = serde_json::to_string(&binding)
                                .into_alien_error()
                                .context(ErrorData::ResourceControllerConfigError {
                                    resource_id: binding_key.clone(),
                                    message: "Failed to serialize ArtifactRegistry binding"
                                        .to_string(),
                                })?;

                            debug!(
                                original_url = %original_url,
                                rewritten_url = %new_url,
                                "Rewrote ArtifactRegistry localhost URL for container"
                            );
                            *binding_json = new_json;
                        }
                    }
                    continue;
                }
                _ => {
                    return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: binding_key,
                        message:
                            "Local platform containers cannot use cloud artifact registry bindings"
                                .to_string(),
                    }));
                }
            }
        }

        // Container binding: only Local variant allowed
        if let Ok(mut binding) = serde_json::from_str::<ContainerBinding>(binding_json) {
            match binding {
                ContainerBinding::Local(ref mut local) => {
                    // Extract URL to avoid borrow conflicts
                    let needs_rewrite = if let Some(BindingValue::Value(ref url)) = local.public_url
                    {
                        url.contains("localhost:") || url.contains("127.0.0.1:")
                    } else {
                        false
                    };

                    if needs_rewrite {
                        if let Some(BindingValue::Value(url)) = &local.public_url {
                            let original_url = url.clone();
                            let new_url = url
                                .replace("localhost:", "host.docker.internal:")
                                .replace("127.0.0.1:", "host.docker.internal:");

                            local.public_url = Some(BindingValue::value(new_url.clone()));
                            let new_json = serde_json::to_string(&binding)
                                .into_alien_error()
                                .context(ErrorData::ResourceControllerConfigError {
                                    resource_id: binding_key.clone(),
                                    message: "Failed to serialize Container binding".to_string(),
                                })?;

                            debug!(
                                original_url = %original_url,
                                rewritten_url = %new_url,
                                "Rewrote Container localhost URL for container"
                            );
                            *binding_json = new_json;
                        }
                    }
                    continue;
                }
                _ => {
                    return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: binding_key,
                        message: "Local platform containers cannot use cloud container bindings"
                            .to_string(),
                    }));
                }
            }
        }

        // Function binding: only Local variant allowed
        if let Ok(mut binding) = serde_json::from_str::<FunctionBinding>(binding_json) {
            match binding {
                FunctionBinding::Local(ref mut local) => {
                    // Extract URL to avoid borrow conflicts
                    let needs_rewrite = if let BindingValue::Value(ref url) = local.function_url {
                        url.contains("localhost:") || url.contains("127.0.0.1:")
                    } else {
                        false
                    };

                    if needs_rewrite {
                        if let BindingValue::Value(url) = &local.function_url {
                            let original_url = url.clone();
                            let new_url = url
                                .replace("localhost:", "host.docker.internal:")
                                .replace("127.0.0.1:", "host.docker.internal:");

                            local.function_url = BindingValue::value(new_url.clone());
                            let new_json = serde_json::to_string(&binding)
                                .into_alien_error()
                                .context(ErrorData::ResourceControllerConfigError {
                                    resource_id: binding_key.clone(),
                                    message: "Failed to serialize Function binding".to_string(),
                                })?;

                            debug!(
                                original_url = %original_url,
                                rewritten_url = %new_url,
                                "Rewrote Function localhost URL for container"
                            );
                            *binding_json = new_json;
                        }
                    }
                    continue;
                }
                _ => {
                    return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: binding_key,
                        message: "Local platform containers cannot use cloud function bindings"
                            .to_string(),
                    }));
                }
            }
        }
    }

    Ok(())
}
