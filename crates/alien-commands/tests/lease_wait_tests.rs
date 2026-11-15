//! A lease request may ask the manager to wait: it is answered as soon as a
//! command is queued. Covers the manager side (real router, real KV and
//! registry from the `TestCommandServer` harness) and the receiver side
//! against that server and against stand-ins for an older manager.

#![cfg(all(feature = "receiver", feature = "test-utils"))]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alien_bindings::traits::Kv;
use alien_commands::receiver::Receiver;
use alien_commands::server::CommandServer;
use alien_commands::test_utils::{test_inline_create_command, TestCommandServer};
use alien_commands::types::{CommandResponse, CommandState, LeaseRequest};
use alien_commands::LeaseClient;
use alien_core::{
    ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
    ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_COMMANDS_URL, ENV_ALIEN_DEPLOYMENT_ID,
};
use axum::{routing::post, Json, Router};
use chrono::Utc;

const DEPLOYMENT_ID: &str = "pull-agent";

async fn pull_server() -> TestCommandServer {
    TestCommandServer::builder().with_pull_mode().build().await
}

fn lease_request(server: &TestCommandServer, wait_seconds: u64) -> LeaseRequest {
    LeaseRequest {
        deployment_id: DEPLOYMENT_ID.to_string(),
        target: server.default_target.clone(),
        max_leases: 1,
        lease_seconds: 60,
        wait_seconds,
    }
}

fn client(server: &TestCommandServer) -> LeaseClient {
    let base = reqwest::Url::parse(&server.command_base_url()).unwrap();
    LeaseClient::from_base(&base, "test-token".to_string()).unwrap()
}

fn receiver_env(base_url: &str, resource_id: &str) -> HashMap<String, String> {
    HashMap::from([
        (ENV_ALIEN_COMMANDS_URL.to_string(), base_url.to_string()),
        (
            ENV_ALIEN_COMMANDS_TOKEN.to_string(),
            "test-token".to_string(),
        ),
        (
            ENV_ALIEN_DEPLOYMENT_ID.to_string(),
            DEPLOYMENT_ID.to_string(),
        ),
        (
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string(),
            resource_id.to_string(),
        ),
        (
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE.to_string(),
            "daemon".to_string(),
        ),
    ])
}

/// A second manager process on the same KV, registry and storage: a command
/// created through it never signals the first process's waiters.
fn second_replica(server: &TestCommandServer) -> CommandServer {
    CommandServer::new(
        server.kv.clone(),
        server.storage.clone(),
        server.dispatcher.clone(),
        server.registry.clone(),
        server.command_base_url(),
        b"test-signing-key-for-commands".to_vec(),
    )
}

#[tokio::test]
async fn a_queued_command_is_leased_by_a_waiting_request_at_once() {
    let server = pull_server().await;
    let lease_client = client(&server);
    let request = lease_request(&server, 20);

    let waiting = tokio::spawn(async move { lease_client.acquire(&request).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.command_server.open_lease_waits(), 1);

    let queued_at = Instant::now();
    let created = server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let leases = waiting.await.unwrap().expect("lease request succeeds");
    let waited = queued_at.elapsed();

    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].command_id, created.command_id);
    assert!(
        waited < Duration::from_millis(500),
        "leased after {waited:?}"
    );
    assert_eq!(server.command_server.open_lease_waits(), 0);
}

#[tokio::test]
async fn an_empty_wait_returns_after_wait_seconds() {
    let server = pull_server().await;
    let started = Instant::now();
    let leases = client(&server)
        .acquire(&lease_request(&server, 1))
        .await
        .unwrap();
    let waited = started.elapsed();
    assert!(leases.is_empty());
    assert!(
        waited >= Duration::from_millis(950) && waited < Duration::from_millis(2500),
        "waited {waited:?}"
    );
    assert_eq!(server.command_server.open_lease_waits(), 0);
}

#[tokio::test]
async fn a_request_without_wait_seconds_is_answered_at_once() {
    // An older Operator sends no waitSeconds field.
    let server = pull_server().await;
    let body = serde_json::json!({
        "deploymentId": DEPLOYMENT_ID,
        "target": {"resourceId": server.default_target.resource_id, "resourceType": "daemon"},
        "maxLeases": 1,
        "leaseSeconds": 60,
    });
    let started = Instant::now();
    let response = reqwest::Client::new()
        .post(format!("{}commands/leases", server.command_base_url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let answer: serde_json::Value = response.json().await.unwrap();
    assert_eq!(answer["leases"], serde_json::json!([]));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(server.command_server.open_lease_waits(), 0);
}

const PROBE: Duration = Duration::from_millis(300);

async fn server_with_probe() -> TestCommandServer {
    TestCommandServer::builder()
        .with_pull_mode()
        .with_lease_wait_probe_interval(PROBE)
        .build()
        .await
}

/// The command is created through another manager process: the waiter's
/// in-process signal never fires, the wake key in the shared KV does.
#[tokio::test]
async fn a_command_created_on_another_replica_is_found_within_the_probe_interval() {
    let server = server_with_probe().await;
    let replica_a = second_replica(&server);
    let mut pickups = Vec::new();
    for round in 0..5 {
        let lease_client = client(&server);
        let request = lease_request(&server, 20);
        let waiting = tokio::spawn(async move { lease_client.acquire(&request).await });
        // Spread the creates over the probe interval.
        tokio::time::sleep(Duration::from_millis(200 + round * 70)).await;

        let created = replica_a
            .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
            .await
            .unwrap();
        // From the moment the create returned (the command is queued).
        let queued_at = Instant::now();
        let leases = waiting.await.unwrap().unwrap();
        pickups.push(queued_at.elapsed());
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].command_id, created.command_id);
        // Answer it, as a receiver does: a leased command that stays in the
        // pending index would fill the scan window of later requests.
        server
            .submit_command_response(&created.command_id, CommandResponse::success(b"done"))
            .await
            .unwrap();
    }
    println!("cross-replica pickup with a {PROBE:?} probe: {pickups:?}");
    for pickup in pickups {
        assert!(pickup < PROBE + Duration::from_millis(200), "{pickup:?}");
    }
}

/// A command queued through another replica after this request's scan, before
/// it starts probing: the key must have been read before the scan.
#[tokio::test]
async fn a_command_queued_between_the_scan_and_the_probing_is_not_missed() {
    let server = TestCommandServer::builder()
        .with_pull_mode()
        .with_fault_injection()
        .with_lease_wait_probe_interval(PROBE)
        .build()
        .await;
    let replica_a = Arc::new(second_replica(&server));
    server
        .fault_kv
        .as_ref()
        .unwrap()
        .arm_after_next_pending_scan(Box::new({
            let replica_a = replica_a.clone();
            move || {
                Box::pin(async move {
                    replica_a
                        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
                        .await
                        .unwrap();
                })
            }
        }));

    let started = Instant::now();
    let leases = client(&server)
        .acquire(&lease_request(&server, 20))
        .await
        .unwrap();
    assert_eq!(leases.len(), 1, "the command was missed");
    assert!(started.elapsed() < PROBE + Duration::from_millis(500));
}

#[tokio::test]
async fn a_failed_wake_key_write_does_not_fail_the_create() {
    let server = TestCommandServer::builder()
        .with_pull_mode()
        .with_fault_injection()
        .build()
        .await;
    server
        .fault_kv
        .as_ref()
        .unwrap()
        .arm_wake_key_write_failure();
    let created = server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .expect("the create succeeds without its wake key");
    // The next lease request still finds it.
    let leases = client(&server)
        .acquire(&lease_request(&server, 0))
        .await
        .unwrap();
    assert_eq!(leases[0].command_id, created.command_id);
}

#[tokio::test]
async fn every_create_changes_the_wake_key() {
    let server = pull_server().await;
    let key = format!(
        "target:{DEPLOYMENT_ID}:{}:wake",
        server.default_target.resource_id
    );
    let mut seen = Vec::new();
    for _ in 0..3 {
        server
            .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
            .await
            .unwrap();
        seen.push(server.kv.get(&key).await.unwrap().map(|entry| entry.value));
    }
    assert!(seen.iter().all(Option::is_some));
    assert_ne!(seen[0], seen[1]);
    assert_ne!(seen[1], seen[2]);
}

#[tokio::test]
async fn two_waiters_and_one_command_lease_it_exactly_once() {
    let server = pull_server().await;
    let first = {
        let c = client(&server);
        let r = lease_request(&server, 2);
        tokio::spawn(async move { c.acquire(&r).await })
    };
    let second = {
        let c = client(&server);
        let r = lease_request(&server, 2);
        tokio::spawn(async move { c.acquire(&r).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.command_server.open_lease_waits(), 2);

    server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();
    assert_eq!(first.len() + second.len(), 1, "one lease in total");
    assert_eq!(server.command_server.open_lease_waits(), 0);
}

#[tokio::test]
async fn a_waiter_is_not_woken_by_another_target() {
    let server = pull_server().await;
    server
        .registry
        .register_target(
            "other-daemon",
            alien_commands::types::CommandTargetType::Daemon,
        )
        .await
        .unwrap();
    let lease_client = client(&server);
    let request = lease_request(&server, 1);
    let waiting = tokio::spawn(async move { lease_client.acquire(&request).await });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut other = test_inline_create_command(DEPLOYMENT_ID, "read");
    other.target_resource_id = Some("other-daemon".to_string());
    server.create_command(other).await.unwrap();

    assert!(waiting.await.unwrap().unwrap().is_empty());
}

#[tokio::test]
async fn over_the_cap_a_request_is_answered_at_once_and_entries_are_released() {
    let server = TestCommandServer::builder()
        .with_pull_mode()
        .with_max_open_lease_waits(1)
        .build()
        .await;
    let lease_client = client(&server);
    let request = lease_request(&server, 20);
    let first = tokio::spawn({
        let (c, r) = (lease_client.clone(), request.clone());
        async move { c.acquire(&r).await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.command_server.open_lease_waits(), 1);

    let started = Instant::now();
    let answer = lease_client.acquire(&request).await.unwrap();
    assert!(answer.is_empty());
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(server.command_server.open_lease_waits(), 1);

    // The waiter that holds the slot is still served.
    server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    assert_eq!(first.await.unwrap().unwrap().len(), 1);
    assert_eq!(server.command_server.open_lease_waits(), 0);
}

#[tokio::test]
async fn an_abandoned_wait_releases_its_slot() {
    let server = pull_server().await;
    let lease_client = client(&server);
    let request = lease_request(&server, 20);
    let waiting = tokio::spawn(async move { lease_client.acquire(&request).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.command_server.open_lease_waits(), 1);
    waiting.abort();
    let _ = waiting.await;
    for _ in 0..50 {
        if server.command_server.open_lease_waits() == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the wait of a closed connection stayed open");
}

#[tokio::test]
async fn a_wait_ends_before_the_credential_does_and_never_leases_after_it() {
    let server = pull_server().await;
    let request = lease_request(&server, 20);
    let deadline = Utc::now() + chrono::Duration::seconds(5);

    let started = Instant::now();
    let answer = server
        .command_server
        .acquire_lease_with_deadline(DEPLOYMENT_ID, &request, Some(deadline))
        .await
        .expect("answered normally");
    let waited = started.elapsed();
    assert!(answer.leases.is_empty());
    assert!(waited < Duration::from_secs(4), "waited {waited:?}");

    // A command queued after the credential's end is not leased by it.
    server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let expired = server
        .command_server
        .acquire_lease_with_deadline(DEPLOYMENT_ID, &request, Some(deadline))
        .await;
    assert!(expired.is_err(), "an expired credential never leases");
}

// ---- receiver ----------------------------------------------------------------

#[tokio::test]
async fn a_receiver_picks_up_a_command_queued_after_a_waited_empty_answer() {
    let server = pull_server().await;
    let (seen_tx, mut seen_rx) = tokio::sync::mpsc::unbounded_channel();
    // Poll interval far beyond the test: only a re-poll at once can find it.
    let mut receiver = Receiver::from_env_vars(&receiver_env(
        &server.command_base_url(),
        &server.default_target.resource_id,
    ))
    .unwrap()
    .with_poll_interval(Duration::from_secs(30))
    .with_lease_wait(Duration::from_secs(1));
    receiver.command("read", move |_: serde_json::Value, _ctx| {
        let seen_tx = seen_tx.clone();
        async move {
            seen_tx.send(Instant::now()).unwrap();
            Ok(serde_json::json!({}))
        }
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });

    // Past the first waited (empty) answer.
    tokio::time::sleep(Duration::from_millis(1600)).await;
    let queued_at = Instant::now();
    let created = server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let handled_at = tokio::time::timeout(Duration::from_secs(5), seen_rx.recv())
        .await
        .expect("handled")
        .unwrap();
    assert!(handled_at - queued_at < Duration::from_millis(800));
    let status = server
        .wait_for_completion(&created.command_id, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(status.state, CommandState::Succeeded);

    shutdown.shutdown();
    run.await.unwrap().unwrap();
}

#[tokio::test]
async fn shutdown_during_a_wait_returns_promptly() {
    let server = pull_server().await;
    let receiver = Receiver::from_env_vars(&receiver_env(
        &server.command_base_url(),
        &server.default_target.resource_id,
    ))
    .unwrap();
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(server.command_server.open_lease_waits(), 1);

    let started = Instant::now();
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
}

/// Stand-in for a manager that predates waiting: ignores the field and
/// answers every lease request at once, empty.
async fn old_manager() -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<serde_json::Value>>>) {
    let count = Arc::new(AtomicUsize::new(0));
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().route(
        "/v1/commands/leases",
        post({
            let (count, bodies) = (count.clone(), bodies.clone());
            move |Json(body): Json<serde_json::Value>| {
                let (count, bodies) = (count.clone(), bodies.clone());
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    bodies.lock().unwrap().push(body);
                    Json(serde_json::json!({"leases": []}))
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/v1/"), count, bodies)
}

#[tokio::test]
async fn a_new_receiver_against_an_old_manager_backs_off_and_does_not_spin() {
    let (url, count, bodies) = old_manager().await;
    let receiver = Receiver::from_env_vars(&receiver_env(&url, "test-daemon"))
        .unwrap()
        .with_poll_interval(Duration::from_millis(100))
        .with_poll_max_interval(Duration::from_millis(800))
        .with_poll_jitter(0.0);
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });

    tokio::time::sleep(Duration::from_millis(3000)).await;
    shutdown.shutdown();
    run.await.unwrap().unwrap();

    // Backoff 100, 200, 400, 800, 800...: about 7 polls in 3 s. A spinning
    // receiver would make hundreds.
    let polls = count.load(Ordering::SeqCst);
    assert!((4..=9).contains(&polls), "{polls} polls in 3 s");
    // The wait was asked for on every poll.
    assert!(bodies
        .lock()
        .unwrap()
        .iter()
        .all(|body| body["waitSeconds"] == 25));
}

#[tokio::test]
async fn a_receiver_does_not_spin_on_fast_empty_answers_after_a_wait_request() {
    // Same stand-in, no backoff configured beyond the poll interval: the
    // floor and the fast-empty rule must keep the request rate at the poll
    // interval, not at network speed.
    let (url, count, _) = old_manager().await;
    let receiver = Receiver::from_env_vars(&receiver_env(&url, "test-daemon"))
        .unwrap()
        .with_poll_interval(Duration::from_millis(300))
        .with_poll_max_interval(Duration::from_millis(300))
        .with_poll_jitter(0.0);
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(2000)).await;
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    let polls = count.load(Ordering::SeqCst);
    assert!(polls <= 8, "{polls} polls in 2 s");
}

/// Stand-in for a manager that holds each request for `hold`, then answers
/// empty. Checks that a waited empty answer is followed by a poll at once,
/// not after the poll interval.
#[tokio::test]
async fn a_waited_empty_answer_is_followed_by_a_poll_at_once() {
    let count = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/v1/commands/leases",
        post({
            let count = count.clone();
            move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(1100)).await;
                    Json(serde_json::json!({"leases": []}))
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let receiver = Receiver::from_env_vars(&receiver_env(&format!("http://{addr}/v1/"), "x"))
        .unwrap()
        .with_poll_interval(Duration::from_secs(30))
        .with_poll_jitter(0.0);
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(3500)).await;
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    // 1.1 s each, polled again at once: 4 polls started in 3.5 s (with the
    // 30 s poll interval it would be 1).
    assert!((3..=4).contains(&count.load(Ordering::SeqCst)));
}
