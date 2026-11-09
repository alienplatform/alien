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
    PostgresBinding, QueueBinding, SandboxBinding, StorageBinding, VaultBinding, WorkerBinding,
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

        if let Ok(mut binding) = serde_json::from_str::<QueueBinding>(binding_json) {
            match &mut binding {
                QueueBinding::LocalHttp(local) => {
                    if let BindingValue::Value(url) = &mut local.endpoint {
                        let mut endpoint = reqwest::Url::parse(url).into_alien_error().context(
                            ErrorData::ResourceControllerConfigError {
                                resource_id: binding_key.clone(),
                                message: "Invalid local queue endpoint".into(),
                            },
                        )?;
                        if endpoint.host_str() == Some("localhost")
                            || endpoint.host_str().is_some_and(|host| {
                                host.trim_matches(['[', ']'])
                                    .parse::<std::net::IpAddr>()
                                    .is_ok_and(|ip| ip.is_loopback())
                            })
                        {
                            endpoint.set_host(Some("host.docker.internal")).into_alien_error().context(
                                ErrorData::ResourceControllerConfigError {
                                    resource_id: binding_key.clone(),
                                    message: "Rewrite local queue endpoint for Docker".into(),
                                },
                            )?;
                        }
                        *url = endpoint.to_string();
                    }
                    *binding_json = serde_json::to_string(&binding).into_alien_error().context(
                        ErrorData::ResourceControllerConfigError { resource_id: binding_key.clone(), message: "Serialize local queue endpoint".into() })?;
                    continue;
                }
                _ => return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: binding_key.clone(), message: "Local containers require a manager-owned local-queue-http binding; rebuild and migrate legacy queues offline".into(),
                })),
            }
        }

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
                        let new_json = serde_json::to_string(&binding).into_alien_error().context(
                            ErrorData::ResourceControllerConfigError {
                                resource_id: binding_key.clone(),
                                message: "Failed to serialize Postgres binding".to_string(),
                            },
                        )?;

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

/// Where a linked Local sandbox's token file is mounted in a container, one file per binding.
const SANDBOX_TOKEN_DIR: &str = "/run/alien/sandbox";

/// The bindings of the Local sandboxes a container links, as the controller resolved them from
/// those links, once per sandbox. A sandbox linked more than once still gets one token mount:
/// Docker refuses two mounts at the same container path.
pub(super) fn linked_sandbox_bindings(
    links: &[alien_core::ResourceRef],
    env_vars: &std::collections::HashMap<String, String>,
) -> Vec<(String, String)> {
    let bindings: std::collections::BTreeMap<String, String> = links
        .iter()
        .filter(|link| link.resource_type == alien_core::Sandbox::RESOURCE_TYPE)
        .filter_map(|link| {
            let key = binding_env_var_name(link.id());
            env_vars.get(&key).map(|value| (key, value.clone()))
        })
        .collect();
    bindings.into_iter().collect()
}

/// Makes each linked Local sandbox reachable from inside a container.
///
/// The binding points at the manager's route on loopback and at a token file on the host; neither
/// exists inside the container. Its URL moves to `host.docker.internal` (the caller forwards the
/// route there), and the one token file is mounted
/// read-only (not its directory, which holds other sandboxes' tokens), with the binding pointed
/// at the mount. The mount is shared with the host operator's identity because the file is
/// readable by it alone, as Storage, KV and Vault mounts already are.
///
/// **Fails fast** on a cloud sandbox binding, as the other rewrites do. A token file that is not
/// there yet is not mounted (Docker would create a directory in its place, which the route could
/// then never write its token to): the sandbox is reported as a dependency that is not ready,
/// because after a manager restart its next tick writes the token again.
pub(super) fn mount_local_sandboxes_for_container(
    container_id: &str,
    sandbox_bindings: Vec<(String, String)>,
    env_vars: &mut std::collections::HashMap<String, String>,
    bind_mounts: &mut Vec<alien_local::BindMount>,
) -> Result<()> {
    for (binding_key, resolved) in sandbox_bindings {
        let mut binding = serde_json::from_str::<SandboxBinding>(&resolved)
            .into_alien_error()
            .context(ErrorData::ResourceControllerConfigError {
                resource_id: binding_key.clone(),
                message: "A linked sandbox's binding is not a sandbox binding".into(),
            })?;
        let SandboxBinding::Local(local) = &mut binding else {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: binding_key,
                message: "Local platform containers cannot use cloud sandbox bindings".to_string(),
            }));
        };

        if let BindingValue::Value(url) = &mut local.manager_url {
            let mut endpoint = reqwest::Url::parse(url).into_alien_error().context(
                ErrorData::ResourceControllerConfigError {
                    resource_id: binding_key.clone(),
                    message: "Invalid local sandbox route URL".into(),
                },
            )?;
            if endpoint.host_str().is_some_and(|host| {
                host == "localhost"
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            }) {
                endpoint
                    .set_host(Some("host.docker.internal"))
                    .into_alien_error()
                    .context(ErrorData::ResourceControllerConfigError {
                        resource_id: binding_key.clone(),
                        message: "Rewrite local sandbox route URL for Docker".into(),
                    })?;
                *url = endpoint.as_str().trim_end_matches('/').to_string();
            }
        }
        if let BindingValue::Value(host_path) = &local.token_path {
            if !std::path::Path::new(host_path).is_file() {
                debug!(resource = %binding_key, %host_path, "the linked sandbox's route has not written its token yet");
                return Err(AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: container_id.to_string(),
                    dependency_id: binding_key,
                }));
            }
            let container_path = format!(
                "{SANDBOX_TOKEN_DIR}/{}.token",
                binding_key.to_ascii_lowercase()
            );
            bind_mounts.push(alien_local::BindMount {
                host_path: std::path::PathBuf::from(host_path),
                container_path: container_path.clone(),
                resource_id: binding_key.clone(),
                shared_with_host_workloads: true,
                read_only: true,
            });
            local.token_path = BindingValue::value(container_path);
        }

        let rewritten = serde_json::to_string(&binding).into_alien_error().context(
            ErrorData::ResourceControllerConfigError {
                resource_id: binding_key.clone(),
                message: "Failed to serialize Sandbox binding".to_string(),
            },
        )?;
        debug!(resource = %binding_key, "Pointed the Local sandbox binding at the container's view");
        // Written over whatever a deployment variable of the same name put there.
        env_vars.insert(binding_key, rewritten);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bindings::{serialize_binding_as_env_var, PostgresBinding, QueueBinding};

    #[test]
    fn queue_network_binding_preserves_credentials_and_rewrites_only_loopback_host() {
        for (host, expected) in [
            ("localhost", "host.docker.internal"),
            ("127.0.0.1", "host.docker.internal"),
            ("127.0.0.2", "host.docker.internal"),
            ("[::1]", "host.docker.internal"),
            ("192.0.2.10", "192.0.2.10"),
            ("[2001:db8::1]", "[2001:db8::1]"),
        ] {
            let binding = QueueBinding::LocalHttp(alien_core::bindings::LocalHttpQueueBinding {
                endpoint: BindingValue::value(format!(
                    "http://{host}:4318/v1/local/queues/dep/localhost:jobs"
                )),
                token: Some("test-credential".into()),
            });
            let mut env = serialize_binding_as_env_var("jobs", &binding).unwrap();
            rewrite_localhost_urls_for_container(&mut env).unwrap();
            let QueueBinding::LocalHttp(actual) =
                serde_json::from_str(&env[&binding_env_var_name("jobs")]).unwrap()
            else {
                panic!("queue transport changed");
            };
            assert_eq!(actual.token.as_deref(), Some("test-credential"));
            assert!(
                matches!(actual.endpoint, BindingValue::Value(url) if url == format!("http://{expected}:4318/v1/local/queues/dep/localhost:jobs"))
            );
        }
    }

    fn local_sandbox_binding(token_path: &std::path::Path) -> SandboxBinding {
        SandboxBinding::Local(alien_core::bindings::LocalSandboxBinding {
            default_policy: None,
            egress: None,
            manager_url: BindingValue::value("http://127.0.0.1:40205".to_string()),
            sandbox_key: BindingValue::value("code-key".to_string()),
            token_path: BindingValue::value(token_path.display().to_string()),
        })
    }

    #[test]
    fn local_sandbox_binding_is_reachable_from_a_container() {
        let state = tempfile::TempDir::new().unwrap();
        let token = state.path().join("sandbox-manager-code.token");
        std::fs::write(&token, "t").unwrap();
        let mut env = serialize_binding_as_env_var("code", &local_sandbox_binding(&token)).unwrap();
        let mut mounts = Vec::new();
        mount_local_sandboxes_for_container("web", resolved(&env), &mut env, &mut mounts).unwrap();

        let SandboxBinding::Local(actual) =
            serde_json::from_str(&env[&binding_env_var_name("code")]).unwrap()
        else {
            panic!("sandbox transport changed");
        };
        assert_eq!(
            actual.manager_url,
            BindingValue::value("http://host.docker.internal:40205".to_string())
        );
        assert_eq!(
            actual.token_path,
            BindingValue::value("/run/alien/sandbox/alien_code_binding.token".to_string())
        );
        assert_eq!(
            actual.sandbox_key,
            BindingValue::value("code-key".to_string())
        );
        assert_eq!(mounts.len(), 1, "only the token file, never its directory");
        assert_eq!(mounts[0].host_path, token);
        assert_eq!(
            mounts[0].container_path,
            "/run/alien/sandbox/alien_code_binding.token"
        );
        assert!(mounts[0].read_only);
        assert!(
            mounts[0].shared_with_host_workloads,
            "the file is the operator's alone"
        );
    }

    /// A token the route has not written yet (a manager restart the sandbox has not caught up
    /// with) waits for the sandbox instead of failing the container, and mounts nothing until
    /// then. Once the token is there, the next attempt mounts it.
    #[test]
    fn a_token_not_written_yet_waits_for_the_sandbox_then_mounts() {
        let state = tempfile::TempDir::new().unwrap();
        let token = state.path().join("sandbox-manager-code.token");
        let linked = serialize_binding_as_env_var("code", &local_sandbox_binding(&token)).unwrap();

        let mut env = linked.clone();
        let mut mounts = Vec::new();
        let error =
            mount_local_sandboxes_for_container("web", resolved(&env), &mut env, &mut mounts)
                .expect_err("no token file yet");
        assert_eq!(error.code, "DEPENDENCY_NOT_READY");
        assert!(error.retryable);
        assert!(
            mounts.is_empty(),
            "Docker would create a directory at a missing source"
        );

        std::fs::write(&token, "t").unwrap();
        let mut env = linked;
        mount_local_sandboxes_for_container("web", resolved(&env), &mut env, &mut mounts)
            .expect("the token is there now");
        assert_eq!(mounts.len(), 1, "{mounts:?}");
        assert_eq!(mounts[0].host_path, token);
    }

    #[test]
    fn cloud_sandbox_binding_is_rejected_in_a_local_container() {
        let mut env = std::collections::HashMap::from([(
            binding_env_var_name("code"),
            serde_json::json!({
                "service": "sandbox-aws",
                "imageArn": "arn:aws:lambda:us-east-1:123456789012:microvm-image:x",
                "imageVersion": "1.0",
                "region": "us-east-1",
            })
            .to_string(),
        )]);
        let mut mounts = Vec::new();
        assert!(
            mount_local_sandboxes_for_container("web", resolved(&env), &mut env, &mut mounts)
                .is_err()
        );
        assert!(mounts.is_empty());
    }

    /// A deployment variable shaped like a sandbox binding mounts nothing: neither a new
    /// binding that names a host file, nor one written over a linked sandbox's binding.
    #[test]
    fn a_forged_sandbox_binding_mounts_no_host_file() {
        let state = tempfile::TempDir::new().unwrap();
        let token = state.path().join("sandbox-manager-code.token");
        std::fs::write(&token, "t").unwrap();
        let secret = state.path().join("id_ed25519");
        std::fs::write(&secret, "private key").unwrap();

        let linked = serialize_binding_as_env_var("code", &local_sandbox_binding(&token)).unwrap();
        let sandbox_bindings = resolved(&linked);
        let forged = serde_json::to_string(&local_sandbox_binding(&secret)).unwrap();
        let mut env = linked;
        env.insert(binding_env_var_name("fake"), forged.clone());
        env.insert(binding_env_var_name("code"), forged.clone());

        let mut mounts = Vec::new();
        mount_local_sandboxes_for_container("web", sandbox_bindings, &mut env, &mut mounts)
            .unwrap();

        assert_eq!(mounts.len(), 1, "{mounts:?}");
        assert_eq!(
            mounts[0].host_path, token,
            "only the linked sandbox's token"
        );
        assert_eq!(
            env[&binding_env_var_name("fake")],
            forged,
            "an unlinked binding is left as it was, unmounted"
        );
        let SandboxBinding::Local(code) =
            serde_json::from_str(&env[&binding_env_var_name("code")]).unwrap()
        else {
            panic!("sandbox transport changed");
        };
        assert_eq!(
            code.token_path,
            BindingValue::value("/run/alien/sandbox/alien_code_binding.token".to_string()),
            "the linked binding replaces the variable written over it"
        );
    }

    /// Linking the same sandbox twice mounts its token once; Docker refuses a container whose
    /// mounts repeat a path.
    #[test]
    fn a_sandbox_linked_twice_mounts_its_token_once() {
        let state = tempfile::TempDir::new().unwrap();
        let token = state.path().join("sandbox-manager-code.token");
        std::fs::write(&token, "t").unwrap();
        let mut env = serialize_binding_as_env_var("code", &local_sandbox_binding(&token)).unwrap();
        let link = alien_core::ResourceRef::new(alien_core::Sandbox::RESOURCE_TYPE, "code");
        let unlinked = alien_core::ResourceRef::new(alien_core::Sandbox::RESOURCE_TYPE, "absent");

        let bindings = linked_sandbox_bindings(&[link.clone(), link, unlinked], &env);
        assert_eq!(bindings.len(), 1, "{bindings:?}");
        assert_eq!(bindings[0].0, binding_env_var_name("code"));

        let mut mounts = Vec::new();
        mount_local_sandboxes_for_container("web", bindings, &mut env, &mut mounts).unwrap();
        assert_eq!(mounts.len(), 1, "{mounts:?}");
        assert_eq!(mounts[0].host_path, token);
    }

    /// The bindings as the controller resolved them from the container's links.
    fn resolved(env: &std::collections::HashMap<String, String>) -> Vec<(String, String)> {
        env.iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    #[test]
    fn local_postgres_binding_rewritten_for_container_keeps_password() {
        let binding = PostgresBinding::local("127.0.0.1", 5432, "mydb", "alien", "s3cr3t-pw");
        let mut env = serialize_binding_as_env_var("mydb", &binding)
            .expect("serialize local Postgres binding");
        let key = binding_env_var_name("mydb");

        // Pre-rewrite: the full binding carries host 127.0.0.1 and the inline password.
        let before = env.get(&key).expect("binding env var present");
        assert!(
            before.contains("127.0.0.1"),
            "host should start as 127.0.0.1"
        );
        assert!(
            before.contains("s3cr3t-pw"),
            "password must be present before the rewrite"
        );

        rewrite_localhost_urls_for_container(&mut env).expect("rewrite should succeed");

        let rewritten: PostgresBinding =
            serde_json::from_str(env.get(&key).unwrap()).expect("rewritten binding deserializes");
        let PostgresBinding::Local(local) = rewritten else {
            panic!("expected a Local Postgres binding after rewrite");
        };
        let BindingValue::Value(host) = &local.host else {
            panic!("host is a concrete value")
        };
        assert_eq!(
            host, "host.docker.internal",
            "host rewritten to container-reachable address"
        );
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
