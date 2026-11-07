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

/// What a session list through a route answered: its status, or why nothing did.
type Answer = std::result::Result<reqwest::StatusCode, String>;

async fn list_sessions(route_url: &str, token: &str) -> Answer {
    reqwest::Client::new()
        .get(format!("{route_url}/v1/sessions"))
        .bearer_auth(token.trim())
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map(|answer| answer.status())
        .map_err(|error| format!("{error:?}"))
}

/// Serves a sandbox, drops its route as a manager restart does, optionally lets something else
/// take the old port, then runs the first step of an update. Returns the address the route was
/// first served at, the controller's binding URL after that step, and what a session list there
/// answers with the binding's token on disk and with the token a client read before the restart.
async fn update_after_a_restart(
    id: &str,
    old_port_taken: bool,
) -> (String, String, Answer, Answer) {
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

        // What a restart leaves: the persisted address and the token file, with nothing of this
        // process serving the address. A client that read the token before keeps using it.
        let token_path = executor
            .internal_state::<LocalSandboxController>()
            .unwrap()
            .token_path
            .clone()
            .unwrap();
        let earlier_token = tokio::fs::read_to_string(&token_path).await.unwrap();
        alien_local::SandboxRoute::remove(id).await;
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&token_path)
                .unwrap()
                .write_all(earlier_token.as_bytes())
                .unwrap();
        }
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
        let answer = list_sessions(&route_url, &token).await;
        let earlier_answer = list_sessions(&route_url, &earlier_token).await;
        (served, route_url, answer, earlier_answer)
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
    let (served, route_url, answer, earlier_answer) = update_after_a_restart(&id, false).await;
    assert!(
        answer.as_ref().is_ok_and(|status| status.is_success()),
        "the binding names a route that does not answer: {answer:?}"
    );
    assert_eq!(
        route_url, served,
        "workloads that outlived the restart hold the old address"
    );
    assert!(
        earlier_answer.as_ref().is_ok_and(|status| status.is_success()),
        "a workload that outlived the restart is refused with the token it holds: {earlier_answer:?}"
    );
}

/// When the old port has been taken, the route moves, and the update records where: the
/// workloads it re-creates are given this binding.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_update_after_a_restart_records_a_moved_route() {
    let id = format!("route-moved-{}", std::process::id());
    let (served, route_url, answer, _) = update_after_a_restart(&id, true).await;
    assert_ne!(route_url, served, "the old port belongs to someone else");
    assert!(
        answer.as_ref().is_ok_and(|status| status.is_success()),
        "the binding names a route that does not answer: {answer:?}"
    );
}
