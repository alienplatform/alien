use super::*;
use crate::core::controller_test::SingleControllerExecutor;
use alien_core::Platform;
use futures::FutureExt;

fn sandbox(id: &str, egress: alien_core::SandboxEgress) -> Sandbox {
    Sandbox::new(id.to_string())
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
}

/// Serves a sandbox, drops its route as a manager restart does, optionally lets something else
/// take the old port, then runs the first step of an update. Returns the address the route was
/// first served at, and the controller's binding after that step: its URL and whether a session
/// list through it, with its token, answers.
async fn update_after_a_restart(
    id: &str,
    old_port_taken: bool,
) -> (
    String,
    String,
    std::result::Result<reqwest::StatusCode, String>,
) {
    let directory = tempfile::tempdir().unwrap();
    let bindings = alien_local::LocalBindingsProvider::new(directory.path()).unwrap();
    let services = Arc::new(crate::DefaultPlatformServiceProvider::with_local_bindings(
        bindings,
    ));
    let mut executor = SingleControllerExecutor::builder()
        .resource(sandbox(id, alien_core::SandboxEgress::None))
        .controller(LocalSandboxController::default())
        .platform(Platform::Local)
        .client_config(alien_core::ClientConfig::Local {
            state_directory: directory.path().to_string_lossy().into_owned(),
        })
        .service_provider(services)
        .build()
        .await
        .unwrap();

    let outcome = std::panic::AssertUnwindSafe(async {
        executor
            .run_until_status(ResourceStatus::Running)
            .await
            .unwrap();
        let served = executor
            .internal_state::<LocalSandboxController>()
            .unwrap()
            .route_url
            .clone()
            .unwrap();

        // What a restart leaves: the persisted address, with nothing of this process serving it
        alien_local::SandboxRoute::remove(id).await;
        let _squatter = if old_port_taken {
            Some(
                tokio::net::TcpListener::bind(served.trim_start_matches("http://"))
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        executor
            .update(sandbox(id, alien_core::SandboxEgress::Internet))
            .unwrap();
        executor.step().await.unwrap();

        let controller = executor.internal_state::<LocalSandboxController>().unwrap();
        let route_url = controller.route_url.clone().unwrap();
        let token = tokio::fs::read_to_string(controller.token_path.as_ref().unwrap())
            .await
            .unwrap();
        let answer = reqwest::Client::new()
            .get(format!("{route_url}/v1/sessions"))
            .bearer_auth(token.trim())
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map(|answer| answer.status())
            .map_err(|error| format!("{error:?}"));
        (served, route_url, answer)
    })
    .catch_unwind()
    .await;
    alien_local::SandboxRoute::remove(id).await;
    outcome.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// A manager restart drops the route; workloads still hold its URL. An update resumed in the new
/// process serves the route at that same address when it is free.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_update_after_a_manager_restart_leaves_the_binding_answering() {
    let id = format!("route-restart-{}", std::process::id());
    let (served, route_url, answer) = update_after_a_restart(&id, false).await;
    assert!(
        answer.as_ref().is_ok_and(|status| status.is_success()),
        "the binding names a route that does not answer: {answer:?}"
    );
    assert_eq!(
        route_url, served,
        "workloads that outlived the restart hold the old address"
    );
}

/// When the old port has been taken, the route moves, and the update records where: the
/// workloads it re-creates are given this binding.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_update_after_a_restart_records_a_moved_route() {
    let id = format!("route-moved-{}", std::process::id());
    let (served, route_url, answer) = update_after_a_restart(&id, true).await;
    assert_ne!(route_url, served, "the old port belongs to someone else");
    assert!(
        answer.as_ref().is_ok_and(|status| status.is_success()),
        "the binding names a route that does not answer: {answer:?}"
    );
}
