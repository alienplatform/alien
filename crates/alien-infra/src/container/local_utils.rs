//! Utilities for Local platform container binding transformations.
//!
//! This module handles two types of binding transformations needed when running
//! containers on the Local platform:
//!
//! 1. **Filesystem path rewriting** - Host paths → container mount paths
//! 2. **Network URL rewriting** - localhost → host.docker.internal

use crate::error::{ErrorData, Result};
use alien_core::bindings::{
    binding_env_var_name, ArtifactRegistryBinding, BindingValue, ContainerBinding, KvBinding,
    PostgresBinding, StorageBinding, VaultBinding, WorkerBinding,
};
use alien_error::{AlienError, Context, IntoAlienError};
use tracing::debug;

/// Rewrites a binding's filesystem path from host path to container path.
///
/// For containers, linked resources (Storage, KV, Vault) are mounted at different
/// paths inside the container. This worker updates the binding env var to use
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

        // Worker binding: only Local variant allowed
        if let Ok(mut binding) = serde_json::from_str::<WorkerBinding>(binding_json) {
            match binding {
                WorkerBinding::Local(ref mut local) => {
                    // Extract URL to avoid borrow conflicts
                    let needs_rewrite = if let BindingValue::Value(ref url) = local.worker_url {
                        url.contains("localhost:") || url.contains("127.0.0.1:")
                    } else {
                        false
                    };

                    if needs_rewrite {
                        if let BindingValue::Value(url) = &local.worker_url {
                            let original_url = url.clone();
                            let new_url = url
                                .replace("localhost:", "host.docker.internal:")
                                .replace("127.0.0.1:", "host.docker.internal:");

                            local.worker_url = BindingValue::value(new_url.clone());
                            let new_json = serde_json::to_string(&binding)
                                .into_alien_error()
                                .context(ErrorData::ResourceControllerConfigError {
                                    resource_id: binding_key.clone(),
                                    message: "Failed to serialize Worker binding".to_string(),
                                })?;

                            debug!(
                                original_url = %original_url,
                                rewritten_url = %new_url,
                                "Rewrote Worker localhost URL for container"
                            );
                            *binding_json = new_json;
                        }
                    }
                    continue;
                }
                _ => {
                    return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: binding_key,
                        message: "Local platform containers cannot use cloud worker bindings"
                            .to_string(),
                    }));
                }
            }
        }

        // Postgres binding: the Local host is a bare `127.0.0.1` (not a URL), so match it exactly and
        // swap it for the container-reachable host. Only the Local variant is valid on Local.
        if let Ok(mut binding) = serde_json::from_str::<PostgresBinding>(binding_json) {
            match binding {
                PostgresBinding::Local(ref mut local) => {
                    let needs_rewrite = if let BindingValue::Value(ref host) = local.host {
                        host == "127.0.0.1" || host == "localhost"
                    } else {
                        false
                    };

                    if needs_rewrite {
                        local.host = BindingValue::value("host.docker.internal".to_string());
                        let new_json = serde_json::to_string(&binding)
                            .into_alien_error()
                            .context(ErrorData::ResourceControllerConfigError {
                                resource_id: binding_key.clone(),
                                message: "Failed to serialize Postgres binding".to_string(),
                            })?;

                        debug!(
                            resource = %binding_key,
                            "Rewrote Postgres localhost host for container"
                        );
                        *binding_json = new_json;
                    }
                    continue;
                }
                _ => {
                    return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: binding_key,
                        message: "Local platform containers cannot use cloud postgres bindings"
                            .to_string(),
                    }));
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bindings::{serialize_binding_as_env_var, PostgresBinding};

    // A container linked to a Local Postgres must receive the full binding (password inline) with the
    // host rewritten to a container-reachable address. This runs the exact serialize→rewrite
    // composition the container controller performs, so it fails if the env-builder ever emits a
    // password-less binding or the rewrite arm regresses.
    #[test]
    fn local_postgres_binding_rewritten_for_container_keeps_password() {
        let binding = PostgresBinding::local("127.0.0.1", 5432, "mydb", "alien", "s3cr3t-pw");
        let mut env = serialize_binding_as_env_var("mydb", &binding)
            .expect("serialize local Postgres binding");
        let key = binding_env_var_name("mydb");

        // Pre-rewrite: the full binding carries host 127.0.0.1 and the inline password.
        let before = env.get(&key).expect("binding env var present");
        assert!(before.contains("127.0.0.1"), "host should start as 127.0.0.1");
        assert!(before.contains("s3cr3t-pw"), "password must be present before the rewrite");

        rewrite_localhost_urls_for_container(&mut env).expect("rewrite should succeed");

        let rewritten: PostgresBinding =
            serde_json::from_str(env.get(&key).unwrap()).expect("rewritten binding deserializes");
        let PostgresBinding::Local(local) = rewritten else {
            panic!("expected a Local Postgres binding after rewrite");
        };
        let BindingValue::Value(host) = &local.host else {
            panic!("host is a concrete value")
        };
        assert_eq!(host, "host.docker.internal", "host rewritten to container-reachable address");
        assert_eq!(local.password, "s3cr3t-pw", "password survives the rewrite");
        let BindingValue::Value(port) = &local.port else {
            panic!("port is a concrete value")
        };
        assert_eq!(*port, 5432);
        let BindingValue::Value(database) = &local.database else {
            panic!("database is a concrete value")
        };
        assert_eq!(database, "mydb");
    }

    // The rewrite fails loud on any non-Local Postgres variant: Local containers must never carry a
    // cloud/external Postgres binding.
    #[test]
    fn non_local_postgres_binding_is_rejected() {
        let binding = PostgresBinding::external("db.example.com", 5432, "mydb", "alien", "pw");
        let mut env = serialize_binding_as_env_var("mydb", &binding)
            .expect("serialize external Postgres binding");

        let err = rewrite_localhost_urls_for_container(&mut env)
            .expect_err("non-Local Postgres must be rejected");
        assert_eq!(err.code, "RESOURCE_CONTROLLER_CONFIG_ERROR");
        assert!(
            format!("{err:?}").contains("cannot use cloud postgres bindings"),
            "error should name the postgres rejection, got: {err:?}"
        );
    }
}
