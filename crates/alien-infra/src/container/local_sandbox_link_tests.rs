use super::*;
use std::{panic::AssertUnwindSafe, sync::Arc};

use crate::{core::controller_test::SingleControllerExecutor, sandbox::LocalSandboxController};
use futures::FutureExt;
use tokio::process::Command;

/// Becomes healthy only once a session list came back from the linked sandbox's route, called
/// with the binding exactly as the container was given it. The token must be the one file in its
/// directory, and read-only.
const LIST_SESSIONS_THEN_SERVE: &str = r#"set -eu
url=$(printf '%s' "$__SANDBOX_BINDING__" | sed -E 's/.*"managerUrl":"([^"]*)".*/\1/')
token=$(printf '%s' "$__SANDBOX_BINDING__" | sed -E 's/.*"tokenPath":"([^"]*)".*/\1/')
bearer=$(cat "$token")
if (: >>"$token") 2>/dev/null; then echo "the token is writable" >&2; exit 1; fi
if [ "$(ls -A "$(dirname "$token")" | wc -l)" -ne 1 ]; then echo "the token's directory holds more" >&2; exit 1; fi
mkdir -p /tmp/web
wget -q -T 5 -O /tmp/web/sessions --header "Authorization: Bearer $bearer" "$url/v1/sessions"
echo ok >/tmp/web/health
exec busybox httpd -f -p 8080 -h /tmp/web"#;

/// The same call again, from inside the running container.
const AGAIN_AFTER_RESTART: &str = r#"set -eu
url=$(printf '%s' "$__SANDBOX_BINDING__" | sed -E 's/.*"managerUrl":"([^"]*)".*/\1/')
token=$(printf '%s' "$__SANDBOX_BINDING__" | sed -E 's/.*"tokenPath":"([^"]*)".*/\1/')
wget -q -T 5 -O /dev/null --header "Authorization: Bearer $(cat "$token")" "$url/v1/sessions""#;

#[tokio::test]
#[ignore = "requires Docker and registry access"]
async fn a_container_reaches_the_sandbox_it_links_with_its_binding_across_a_manager_restart() {
    let directory = tempfile::tempdir().unwrap();
    let id = format!("sandbox-link-{}", std::process::id());
    let sandbox_id = format!("code-{}", std::process::id());
    let bindings = alien_local::LocalBindingsProvider::new(directory.path()).unwrap();
    let manager = bindings.container_manager().unwrap();
    let services = Arc::new(crate::DefaultPlatformServiceProvider::with_local_bindings(
        bindings,
    ));
    let client = alien_core::ClientConfig::Local {
        state_directory: directory.path().to_string_lossy().into_owned(),
    };
    let sandbox_with = |egress| {
        alien_core::Sandbox::new(sandbox_id.clone())
            .egress(egress)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(300),
                idle_pause_seconds: None,
            })
            .code(alien_core::SandboxCode::Image {
                image: "busybox:1.37.0".to_string(),
                bundled_from: None,
            })
            .build()
    };
    let sandbox = sandbox_with(alien_core::SandboxEgress::None);

    // The sandbox serves its route first, as it does when a deployment links one
    let mut sandbox_executor = SingleControllerExecutor::builder()
        .resource(sandbox.clone())
        .controller(LocalSandboxController::default())
        .platform(Platform::Local)
        .client_config(client.clone())
        .service_provider(services.clone())
        .build()
        .await
        .unwrap();
    sandbox_executor
        .run_until_status(ResourceStatus::Running)
        .await
        .unwrap();
    let served = serde_json::to_vec(
        sandbox_executor
            .internal_state::<LocalSandboxController>()
            .unwrap(),
    )
    .unwrap();

    let config = Container::new(id.clone())
        .permissions("default-profile".to_string())
        .cpu(alien_core::ResourceSpec {
            min: "0.25".to_string(),
            desired: "0.25".to_string(),
        })
        .memory(alien_core::ResourceSpec {
            min: "64Mi".to_string(),
            desired: "64Mi".to_string(),
        })
        .code(ContainerCode::Image {
            image: "busybox:1.37.0".to_string(),
        })
        .command(vec![
            "sh".to_string(),
            "-c".to_string(),
            LIST_SESSIONS_THEN_SERVE.replace(
                "__SANDBOX_BINDING__",
                &alien_core::bindings::binding_env_var_name(&sandbox_id),
            ),
        ])
        .health_check(alien_core::HealthCheck {
            path: "/health".to_string(),
            port: Some(8080),
            method: "GET".to_string(),
            timeout_seconds: 1,
            failure_threshold: 20,
        })
        .link(&sandbox)
        .build();
    let mut executor = SingleControllerExecutor::builder()
        .resource(config)
        .controller(LocalContainerController::default())
        .with_dependency(
            sandbox,
            serde_json::from_slice::<LocalSandboxController>(&served).unwrap(),
        )
        .platform(Platform::Local)
        .client_config(client)
        .service_provider(services)
        .real_delays()
        .build()
        .await
        .unwrap();

    let verification = AssertUnwindSafe(async {
        // If the route is unreachable, the container exits and restarts until its health check
        // gives up; the timeout bounds that
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(180),
            executor.run_until_terminal(),
        )
        .await;
        let logs = Command::new("docker")
            .args(["logs", &format!("alien-{id}")])
            .output()
            .await
            .unwrap();
        let logs = format!(
            "{}{}",
            String::from_utf8_lossy(&logs.stdout),
            String::from_utf8_lossy(&logs.stderr)
        );
        assert!(
            matches!(outcome, Ok(Ok(()))) && executor.status() == ResourceStatus::Running,
            "the container never reached its linked sandbox ({:?}): {logs}",
            executor.status(),
        );

        // A manager restart: the route and its gateway forward go, the token file stays, and the
        // container keeps running. The sandbox's update resumes in the new process, then the
        // container's next health tick runs.
        let token_path = sandbox_executor
            .internal_state::<LocalSandboxController>()
            .unwrap()
            .token_path
            .clone()
            .unwrap();
        let token = std::fs::read(&token_path).unwrap();
        alien_local::SandboxRoute::remove(&sandbox_id).await;
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&token_path)
                .unwrap()
                .write_all(&token)
                .unwrap();
        }
        sandbox_executor
            .update(sandbox_with(alien_core::SandboxEgress::Internet))
            .unwrap();
        sandbox_executor.step().await.unwrap();
        executor.step().await.unwrap();

        // The same container, with the binding and token it read before the restart
        let after_restart = Command::new("docker")
            .args([
                "exec",
                &format!("alien-{id}"),
                "sh",
                "-c",
                &AGAIN_AFTER_RESTART.replace(
                    "__SANDBOX_BINDING__",
                    &alien_core::bindings::binding_env_var_name(&sandbox_id),
                ),
            ])
            .output()
            .await
            .unwrap();
        assert!(
            after_restart.status.success(),
            "the container lost its linked sandbox across a manager restart: {}{}",
            String::from_utf8_lossy(&after_restart.stdout),
            String::from_utf8_lossy(&after_restart.stderr)
        );
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    })
    .catch_unwind()
    .await;
    // Cleanup reports rather than panics, so a failed assertion above keeps its own message
    if manager.container_exists(&id).await.unwrap_or(true) {
        if let Err(error) = manager.delete_container_and_storage(&id).await {
            eprintln!("could not remove container {id}: {error}");
        }
    }
    alien_local::SandboxRoute::remove(&sandbox_id).await;
    if let Err(panic) = verification {
        std::panic::resume_unwind(panic);
    }
}

/// A manager restart mid-deploy: the container's create runs before the sandbox's next tick
/// serves the route again. The create waits for the sandbox (a dependency that is not ready, not
/// a failure), and once the sandbox serves the route the same container starts and reaches it.
#[tokio::test]
#[ignore = "requires Docker and registry access"]
async fn a_container_created_before_its_sandbox_route_returns_waits_and_then_starts() {
    let directory = tempfile::tempdir().unwrap();
    let id = format!("sandbox-wait-{}", std::process::id());
    let sandbox_id = format!("wait-{}", std::process::id());
    let bindings = alien_local::LocalBindingsProvider::new(directory.path()).unwrap();
    let manager = bindings.container_manager().unwrap();
    let services = Arc::new(crate::DefaultPlatformServiceProvider::with_local_bindings(
        bindings,
    ));
    let client = alien_core::ClientConfig::Local {
        state_directory: directory.path().to_string_lossy().into_owned(),
    };
    let sandbox_with = |egress| {
        alien_core::Sandbox::new(sandbox_id.clone())
            .egress(egress)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(300),
                idle_pause_seconds: None,
            })
            .code(alien_core::SandboxCode::Image {
                image: "busybox:1.37.0".to_string(),
                bundled_from: None,
            })
            .build()
    };
    let sandbox = sandbox_with(alien_core::SandboxEgress::None);
    let mut sandbox_executor = SingleControllerExecutor::builder()
        .resource(sandbox.clone())
        .controller(LocalSandboxController::default())
        .platform(Platform::Local)
        .client_config(client.clone())
        .service_provider(services.clone())
        .build()
        .await
        .unwrap();
    sandbox_executor
        .run_until_status(ResourceStatus::Running)
        .await
        .unwrap();
    let served = serde_json::to_vec(
        sandbox_executor
            .internal_state::<LocalSandboxController>()
            .unwrap(),
    )
    .unwrap();

    // The manager restarts before the container is created: the route and its forward go, the
    // token file stays on disk. (Removing the route here also deletes the file, so it is put back.)
    let token_path = sandbox_executor
        .internal_state::<LocalSandboxController>()
        .unwrap()
        .token_path
        .clone()
        .unwrap();
    let token = std::fs::read(&token_path).unwrap();
    alien_local::SandboxRoute::remove(&sandbox_id).await;
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&token_path)
            .unwrap()
            .write_all(&token)
            .unwrap();
    }

    let config = Container::new(id.clone())
        .permissions("default-profile".to_string())
        .cpu(alien_core::ResourceSpec {
            min: "0.25".to_string(),
            desired: "0.25".to_string(),
        })
        .memory(alien_core::ResourceSpec {
            min: "64Mi".to_string(),
            desired: "64Mi".to_string(),
        })
        .code(ContainerCode::Image {
            image: "busybox:1.37.0".to_string(),
        })
        .command(vec![
            "sh".to_string(),
            "-c".to_string(),
            LIST_SESSIONS_THEN_SERVE.replace(
                "__SANDBOX_BINDING__",
                &alien_core::bindings::binding_env_var_name(&sandbox_id),
            ),
        ])
        .health_check(alien_core::HealthCheck {
            path: "/health".to_string(),
            port: Some(8080),
            method: "GET".to_string(),
            timeout_seconds: 1,
            failure_threshold: 20,
        })
        .link(&sandbox)
        .build();
    let mut executor = SingleControllerExecutor::builder()
        .resource(config)
        .controller(LocalContainerController::default())
        .with_dependency(
            sandbox,
            serde_json::from_slice::<LocalSandboxController>(&served).unwrap(),
        )
        .platform(Platform::Local)
        .client_config(client)
        .service_provider(services)
        .real_delays()
        .build()
        .await
        .unwrap();

    let verification = AssertUnwindSafe(async {
        // Steps until the create reaches the sandbox's route, which is not served yet
        let mut waited = None;
        for _ in 0..10 {
            match executor.step().await {
                Ok(_) => {}
                Err(error) => {
                    waited = Some(error);
                    break;
                }
            }
        }
        let waited = waited.expect("the create reached the missing sandbox route");
        assert_eq!(waited.code, "DEPENDENCY_NOT_READY", "{waited:?}");
        assert!(waited.retryable, "{waited:?}");
        assert_ne!(executor.status(), ResourceStatus::ProvisionFailed);
        assert!(
            !manager.container_exists(&id).await.unwrap(),
            "nothing is started while the sandbox is not there"
        );

        // The sandbox's next tick serves the route again, at the address the container was given
        sandbox_executor
            .update(sandbox_with(alien_core::SandboxEgress::Internet))
            .unwrap();
        sandbox_executor.step().await.unwrap();

        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(180),
            executor.run_until_terminal(),
        )
        .await;
        assert!(
            matches!(outcome, Ok(Ok(()))) && executor.status() == ResourceStatus::Running,
            "the container did not start once its sandbox returned ({:?}): {outcome:?}",
            executor.status(),
        );
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    })
    .catch_unwind()
    .await;
    if manager.container_exists(&id).await.unwrap_or(true) {
        if let Err(error) = manager.delete_container_and_storage(&id).await {
            eprintln!("could not remove container {id}: {error}");
        }
    }
    alien_local::SandboxRoute::remove(&sandbox_id).await;
    if let Err(panic) = verification {
        std::panic::resume_unwind(panic);
    }
}
