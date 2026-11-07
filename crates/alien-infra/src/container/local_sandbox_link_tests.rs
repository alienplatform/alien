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

#[tokio::test]
#[ignore = "requires Docker and registry access"]
async fn a_container_reaches_the_sandbox_it_links_with_its_binding() {
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
    let sandbox = alien_core::Sandbox::new(sandbox_id.clone())
        .egress(alien_core::SandboxEgress::None)
        .lifecycle(alien_core::SandboxLifecyclePolicy {
            max_lifetime_seconds: Some(300),
            idle_pause_seconds: None,
        })
        .code(alien_core::SandboxCode::Image {
            image: "busybox:1.37.0".to_string(),
            bundled_from: None,
        })
        .build();

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
