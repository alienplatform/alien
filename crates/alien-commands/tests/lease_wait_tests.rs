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

/// Wait until `count` lease requests are held open by the manager.
async fn wait_for_open_waits(server: &TestCommandServer, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while server.command_server.open_lease_waits() != count {
        assert!(
            Instant::now() < deadline,
            "{} open waits, expected {count}",
            server.command_server.open_lease_waits()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Wait until the fault KV has seen `count` pending-index scans in total.
async fn wait_for_scans(server: &TestCommandServer, count: usize) {
    let kv = server.fault_kv.as_ref().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while kv.pending_scans() < count {
        assert!(Instant::now() < deadline, "no scan");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

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
    replica_on(server, server.kv.clone())
}

/// A second manager process whose KV is `kv` (for example the fault-injecting
/// decorator the first process uses).
fn replica_on(server: &TestCommandServer, kv: Arc<dyn Kv>) -> CommandServer {
    CommandServer::new(
        kv,
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
    wait_for_open_waits(&server, 1).await;
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
    assert!(waited < Duration::from_secs(1), "leased after {waited:?}");
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
        waited >= Duration::from_millis(950) && waited < Duration::from_secs(6),
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
    assert!(started.elapsed() < Duration::from_secs(2));
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
        assert!(
            pickup < PROBE * 2 + Duration::from_millis(500),
            "{pickup:?}"
        );
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
    assert!(started.elapsed() < PROBE * 2 + Duration::from_millis(500));
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

async fn server_counting_scans() -> TestCommandServer {
    TestCommandServer::builder()
        .with_pull_mode()
        .with_fault_injection()
        .with_lease_wait_probe_interval(Duration::from_millis(100))
        .build()
        .await
}

fn wake_key(server: &TestCommandServer) -> String {
    format!(
        "target:{DEPLOYMENT_ID}:{}:wake",
        server.default_target.resource_id
    )
}

#[tokio::test]
async fn a_wake_key_that_stays_absent_causes_no_scans() {
    let server = server_counting_scans().await;
    let fault_kv = server.fault_kv.clone().unwrap();
    let leases = client(&server)
        .acquire(&lease_request(&server, 2))
        .await
        .unwrap();
    assert!(leases.is_empty());
    // The first scan, and one last scan when the wait ends: nothing per probe
    // (about twenty probes).
    assert_eq!(fault_kv.pending_scans(), 2);
}

#[tokio::test]
async fn a_wake_key_that_appears_is_a_change() {
    let server = server_counting_scans().await;
    let other_replica = second_replica(&server);
    assert!(server.kv.get(&wake_key(&server)).await.unwrap().is_none());
    let lease_client = client(&server);
    let request = lease_request(&server, 20);
    let scans_before = server.fault_kv.as_ref().unwrap().pending_scans();
    let waiting = tokio::spawn(async move { lease_client.acquire(&request).await });
    wait_for_scans(&server, scans_before + 1).await;
    other_replica
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let leases = tokio::time::timeout(Duration::from_secs(2), waiting)
        .await
        .expect("woken by the new key")
        .unwrap()
        .unwrap();
    assert_eq!(leases.len(), 1);
}

#[tokio::test]
async fn a_wake_key_that_expires_costs_one_scan_not_one_per_probe() {
    let server = server_counting_scans().await;
    let fault_kv = server.fault_kv.clone().unwrap();
    // Leave a key behind, as an earlier create did.
    let created = server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let lease = server
        .acquire_single_lease(DEPLOYMENT_ID)
        .await
        .unwrap()
        .unwrap();
    server
        .submit_command_response(&lease.command_id, CommandResponse::success(b"done"))
        .await
        .unwrap();
    assert_eq!(lease.command_id, created.command_id);
    assert!(server.kv.get(&wake_key(&server)).await.unwrap().is_some());

    let scans_before = fault_kv.pending_scans();
    let lease_client = client(&server);
    let request = lease_request(&server, 2);
    let waiting = tokio::spawn(async move { lease_client.acquire(&request).await });
    wait_for_open_waits(&server, 1).await;
    // The key expires while the request waits.
    server.kv.delete(&wake_key(&server), None).await.unwrap();
    assert!(waiting.await.unwrap().unwrap().is_empty());

    // First scan, one scan for the change, one when the wait ends: not one
    // per probe (about twenty).
    assert_eq!(fault_kv.pending_scans() - scans_before, 3);
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
    wait_for_open_waits(&server, 2).await;
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
    wait_for_open_waits(&server, 1).await;

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
    wait_for_open_waits(&server, 1).await;
    assert_eq!(server.command_server.open_lease_waits(), 1);

    let started = Instant::now();
    let answer = lease_client.acquire(&request).await.unwrap();
    assert!(answer.is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
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
    wait_for_open_waits(&server, 1).await;
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

// ---- review round: consistency, per-target bound, shared probe, release wake ----

async fn lagging_server(lag: Duration) -> TestCommandServer {
    let server = server_with_probe_and_faults().await;
    server
        .fault_kv
        .as_ref()
        .unwrap()
        .arm_lagging_pending_scans(lag);
    server
}

async fn server_with_probe_and_faults() -> TestCommandServer {
    TestCommandServer::builder()
        .with_pull_mode()
        .with_fault_injection()
        .with_lease_wait_probe_interval(Duration::from_millis(100))
        .build()
        .await
}

/// A command created just before the request arrives, on a store whose plain
/// scans lag behind its writes: the waiting request's first scan must read
/// consistently, or the command is invisible for the whole wait.
#[tokio::test]
async fn a_command_created_just_before_a_waiting_request_is_found_by_its_first_scan() {
    let server = lagging_server(Duration::from_secs(3)).await;
    let created = server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();

    let started = Instant::now();
    let leases = client(&server)
        .acquire(&lease_request(&server, 6))
        .await
        .unwrap();
    assert_eq!(leases.len(), 1, "the lagging scan hid the command");
    assert_eq!(leases[0].command_id, created.command_id);
    assert!(started.elapsed() < Duration::from_secs(2));

    // A request that does not wait keeps today's (lagging) read.
    server
        .submit_command_response(&created.command_id, CommandResponse::success(b"done"))
        .await
        .unwrap();
    server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let plain = client(&server)
        .acquire(&lease_request(&server, 0))
        .await
        .unwrap();
    assert!(plain.is_empty(), "plain requests read as before");
}

#[tokio::test]
async fn waits_on_one_target_are_bounded_and_another_target_is_unaffected() {
    let server = TestCommandServer::builder()
        .with_pull_mode()
        .with_max_open_lease_waits_per_target(2)
        .build()
        .await;
    server
        .registry
        .register_target(
            "other-daemon",
            alien_commands::types::CommandTargetType::Daemon,
        )
        .await
        .unwrap();
    let lease_client = client(&server);
    let request = lease_request(&server, 3);
    let held: Vec<_> = (0..2)
        .map(|_| {
            let (c, r) = (lease_client.clone(), request.clone());
            tokio::spawn(async move { c.acquire(&r).await })
        })
        .collect();
    wait_for_open_waits(&server, 2).await;

    // The third on the same target is answered at once, empty.
    let started = Instant::now();
    assert!(lease_client.acquire(&request).await.unwrap().is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(server.command_server.open_lease_waits(), 2);

    // Another target still waits while this one is saturated.
    let mut other_request = request.clone();
    other_request.target = alien_commands::types::CommandTarget::new(
        "other-daemon",
        alien_commands::types::CommandTargetType::Daemon,
    );
    let other = {
        let c = lease_client.clone();
        tokio::spawn(async move { c.acquire(&other_request).await })
    };
    wait_for_open_waits(&server, 3).await;

    // The waiters that hold slots are still served.
    let mut create = test_inline_create_command(DEPLOYMENT_ID, "read");
    create.target_resource_id = Some(server.default_target.resource_id.clone());
    server.create_command(create).await.unwrap();
    let mut leased = 0;
    for waiter in held {
        leased += waiter.await.unwrap().unwrap().len();
    }
    assert_eq!(leased, 1);
    other.abort();
}

/// A credential that is not bound to a target may send any target id. A wait
/// for a target the deployment does not have is answered at once and holds no
/// slot, so invented ids cannot fill the process, however many are sent.
#[tokio::test]
async fn a_wait_for_a_target_the_deployment_does_not_have_is_answered_at_once() {
    let server = pull_server().await;
    let lease_client = client(&server);
    let started = Instant::now();
    let invented: Vec<_> = (0..50)
        .map(|i| {
            let mut request = lease_request(&server, 20);
            request.target = alien_commands::types::CommandTarget::new(
                format!("invented-{i}"),
                alien_commands::types::CommandTargetType::Daemon,
            );
            let c = lease_client.clone();
            tokio::spawn(async move { c.acquire(&request).await })
        })
        .collect();
    // Every one is answered empty at once; none is parked.
    for waiter in invented {
        let answer = tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("answered at once")
            .unwrap()
            .unwrap();
        assert!(answer.is_empty());
    }
    assert_eq!(server.command_server.open_lease_waits(), 0);
    assert!(started.elapsed() < Duration::from_secs(5));

    // The real target still waits.
    let request = lease_request(&server, 3);
    let waiting = {
        let c = lease_client.clone();
        tokio::spawn(async move { c.acquire(&request).await })
    };
    wait_for_open_waits(&server, 1).await;
    waiting.abort();
}

/// One deployment's waits over many targets stop at the deployment's cap, and
/// another deployment still waits.
#[tokio::test]
async fn waits_of_one_deployment_are_bounded_over_all_its_targets() {
    let server = TestCommandServer::builder()
        .with_pull_mode()
        .with_max_open_lease_waits_per_deployment(3)
        .build()
        .await;
    let mut targets = Vec::new();
    for i in 0..6 {
        let id = format!("daemon-{i}");
        server
            .registry
            .register_target(&id, alien_commands::types::CommandTargetType::Daemon)
            .await
            .unwrap();
        targets.push(alien_commands::types::CommandTarget::new(
            id,
            alien_commands::types::CommandTargetType::Daemon,
        ));
    }
    let hold = |deployment: &str, target: &alien_commands::types::CommandTarget| {
        let command_server = server.command_server.clone();
        let request = LeaseRequest {
            deployment_id: deployment.to_string(),
            target: target.clone(),
            max_leases: 1,
            lease_seconds: 60,
            wait_seconds: 5,
        };
        tokio::spawn(async move {
            command_server
                .acquire_lease_for_bound_target(&request.deployment_id, &request, None)
                .await
        })
    };
    let mut held: Vec<_> = targets[..3]
        .iter()
        .map(|t| hold(DEPLOYMENT_ID, t))
        .collect();
    wait_for_open_waits(&server, 3).await;

    // Three more targets of the same deployment: answered at once, no slot.
    let started = Instant::now();
    for target in &targets[3..] {
        let answer = hold(DEPLOYMENT_ID, target).await.unwrap().unwrap();
        assert!(answer.leases.is_empty());
    }
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(server.command_server.open_lease_waits(), 3);

    // Another deployment is unaffected.
    held.push(hold("other-deployment", &targets[0]));
    wait_for_open_waits(&server, 4).await;
    for waiter in held {
        waiter.abort();
    }
}

/// Eight waiters on one target: one stream of wake key reads, and one create
/// costs a few scans, not eight each.
#[tokio::test]
async fn eight_waiters_on_one_target_share_the_probe_and_serialize_their_scans() {
    let server = server_with_probe_and_faults().await;
    let fault_kv = server.fault_kv.clone().unwrap();
    let lease_client = client(&server);
    let request = lease_request(&server, 20);
    let waiters: Vec<_> = (0..8)
        .map(|_| {
            let (c, r) = (lease_client.clone(), request.clone());
            tokio::spawn(async move { c.acquire(&r).await })
        })
        .collect();
    wait_for_open_waits(&server, 8).await;
    wait_for_scans(&server, 8).await;

    // Idle for one second at a 100 ms interval: unshared, eight waiters would
    // read about 80 times; shared, a read is reused for half an interval, at
    // most about 20.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let reads_before = fault_kv.wake_key_reads();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let reads = fault_kv.wake_key_reads() - reads_before;
    println!("wake key reads by 8 waiters in 1 s: {reads}");
    assert!(reads <= 30, "{reads} reads in 1 s");

    // One create: one lease, and the other seven waiters do not each scan.
    let scans_before = fault_kv.pending_scans();
    server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.command_server.open_lease_waits() != 7 {
        assert!(Instant::now() < deadline, "the lease holder did not leave");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let scans = fault_kv.pending_scans() - scans_before;
    println!("scans for one create with 8 waiters: {scans}");
    assert!(scans <= 3, "{scans} scans for one create");
    for waiter in waiters {
        waiter.abort();
    }
}

/// A scan that stopped at its limit did not see the whole queue, so its empty
/// answer must not stop a larger request's scan. Two commands are already
/// leased (and still indexed); a waiter with room for one lease sees only
/// those two, a waiter with room for four reaches the third.
#[tokio::test]
async fn a_small_waiters_partial_empty_scan_does_not_hide_a_command_from_a_larger_waiter() {
    let server = pull_server().await;
    for _ in 0..2 {
        server
            .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
            .await
            .unwrap();
    }
    let mut take_two = lease_request(&server, 0);
    take_two.max_leases = 2;
    let held = client(&server);
    assert_eq!(held.acquire(&take_two).await.unwrap().len(), 2);

    // The small waiter registers first, so it scans first after the wake-up.
    let small = {
        let (c, r) = (client(&server), lease_request(&server, 20));
        tokio::spawn(async move { c.acquire(&r).await })
    };
    wait_for_open_waits(&server, 1).await;
    let large = {
        let (c, mut r) = (client(&server), lease_request(&server, 20));
        r.max_leases = 4;
        tokio::spawn(async move { c.acquire(&r).await })
    };
    wait_for_open_waits(&server, 2).await;

    let started = Instant::now();
    server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let leased = tokio::time::timeout(Duration::from_secs(3), large)
        .await
        .expect("the larger waiter was left to the end of its wait")
        .unwrap()
        .unwrap();
    assert_eq!(leased.len(), 1);
    println!("larger waiter leased after {:?}", started.elapsed());
    small.abort();
}

#[tokio::test]
async fn a_released_lease_wakes_a_parked_waiter() {
    let server = server_with_probe_and_faults().await;
    let created = server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let first = client(&server)
        .acquire(&lease_request(&server, 0))
        .await
        .unwrap();
    assert_eq!(first.len(), 1);

    // Parks: the only command is leased.
    let lease_client = client(&server);
    let request = lease_request(&server, 20);
    let waiting = tokio::spawn(async move { lease_client.acquire(&request).await });
    wait_for_open_waits(&server, 1).await;
    wait_for_scans(&server, 2).await;

    let released_at = Instant::now();
    server
        .command_server
        .release_lease(&created.command_id, &first[0].lease_id)
        .await
        .unwrap();
    let again = waiting.await.unwrap().unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].command_id, created.command_id);
    assert!(released_at.elapsed() < Duration::from_secs(1));
}

/// The release goes through another replica: the waiter finds it by the wake
/// key within the probe interval.
#[tokio::test]
async fn a_lease_released_on_another_replica_is_found_by_the_probe() {
    let server = server_with_probe_and_faults().await;
    let replica = second_replica(&server);
    let created = server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let first = client(&server)
        .acquire(&lease_request(&server, 0))
        .await
        .unwrap();
    let lease_client = client(&server);
    let request = lease_request(&server, 20);
    let waiting = tokio::spawn(async move { lease_client.acquire(&request).await });
    wait_for_open_waits(&server, 1).await;
    wait_for_scans(&server, 2).await;

    let released_at = Instant::now();
    replica
        .release_lease(&created.command_id, &first[0].lease_id)
        .await
        .unwrap();
    assert_eq!(waiting.await.unwrap().unwrap().len(), 1);
    assert!(released_at.elapsed() < Duration::from_secs(2));
}

/// The wake key's own TTL runs out while a request waits (instead of the key
/// being deleted): one change, one scan, not a scan per probe.
#[tokio::test]
async fn a_wake_key_that_expires_by_ttl_costs_one_scan() {
    let server = server_counting_scans().await;
    let fault_kv = server.fault_kv.clone().unwrap();
    server
        .kv
        .put(
            &wake_key(&server),
            vec![7; 16],
            Some(alien_bindings::traits::PutOptions {
                ttl: Some(Duration::from_millis(600)),
                condition: alien_bindings::traits::PutCondition::None,
            }),
        )
        .await
        .unwrap();
    let scans_before = fault_kv.pending_scans();
    let started = Instant::now();
    let leases = client(&server)
        .acquire(&lease_request(&server, 3))
        .await
        .unwrap();
    assert!(leases.is_empty());
    assert!(started.elapsed() >= Duration::from_millis(2900));
    // First scan, one for the expiry, one when the wait ends.
    assert_eq!(fault_kv.pending_scans() - scans_before, 3);
}

/// The create's wake key write fails and the command is queued through another
/// replica: the waiter has nothing to see, and finds the command at the end of
/// its wait.
#[tokio::test]
async fn a_failed_wake_key_write_on_another_replica_is_found_at_the_end_of_the_wait() {
    let server = server_counting_scans().await;
    let fault_kv = server.fault_kv.clone().unwrap();
    let replica = replica_on(&server, fault_kv.clone());
    fault_kv.arm_wake_key_write_failure();

    let lease_client = client(&server);
    let request = lease_request(&server, 2);
    let started = Instant::now();
    let waiting = tokio::spawn(async move { lease_client.acquire(&request).await });
    wait_for_open_waits(&server, 1).await;
    wait_for_scans(&server, 1).await;
    replica
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    let leases = waiting.await.unwrap().unwrap();
    assert_eq!(leases.len(), 1, "found by the last scan");
    assert!(started.elapsed() >= Duration::from_millis(1900));
}

/// Reading the wake key fails: waiting still works. A create in this process
/// wakes at once; one on another replica is found at the end of the wait.
#[tokio::test]
async fn a_wake_key_read_error_does_not_break_waiting() {
    let server = server_counting_scans().await;
    let fault_kv = server.fault_kv.clone().unwrap();
    fault_kv.arm_wake_key_read_failure();

    // Same process: woken at once.
    let lease_client = client(&server);
    let request = lease_request(&server, 20);
    let waiting = {
        let (c, r) = (lease_client.clone(), request.clone());
        tokio::spawn(async move { c.acquire(&r).await })
    };
    wait_for_open_waits(&server, 1).await;
    wait_for_scans(&server, 1).await;
    let queued_at = Instant::now();
    server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    assert_eq!(waiting.await.unwrap().unwrap().len(), 1);
    assert!(queued_at.elapsed() < Duration::from_secs(2));

    // Another replica: found when the wait ends.
    let replica = replica_on(&server, fault_kv.clone());
    let (c, r) = (lease_client.clone(), lease_request(&server, 2));
    let waiting = tokio::spawn(async move { c.acquire(&r).await });
    wait_for_open_waits(&server, 1).await;
    replica
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    assert_eq!(waiting.await.unwrap().unwrap().len(), 1);
}

/// Commands queued through another replica and consumed by another receiver at
/// once: the parked waiter scans for each change, and goes quiet when the
/// stream stops (its baseline follows the key).
#[tokio::test]
async fn a_stream_of_creates_consumed_elsewhere_stops_costing_scans_when_it_stops() {
    let server = server_counting_scans().await;
    let fault_kv = server.fault_kv.clone().unwrap();
    let replica = replica_on(&server, fault_kv.clone());
    let lease_client = client(&server);
    let request = lease_request(&server, 6);
    let waiting = tokio::spawn(async move { lease_client.acquire(&request).await });
    wait_for_open_waits(&server, 1).await;
    wait_for_scans(&server, 1).await;

    for _ in 0..4 {
        let created = replica
            .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
            .await
            .unwrap();
        // Another receiver takes it before the waiter's next probe.
        let lease = replica
            .acquire_lease(
                DEPLOYMENT_ID,
                &LeaseRequest {
                    wait_seconds: 0,
                    ..lease_request(&server, 0)
                },
            )
            .await
            .unwrap();
        if lease.leases.is_empty() {
            // The waiter was first; it ends the test case early.
            break;
        }
        assert_eq!(lease.leases[0].command_id, created.command_id);
        replica
            .submit_command_response(&created.command_id, CommandResponse::success(b"done"))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if waiting.is_finished() {
        return; // the waiter won a race for a command; nothing left to count
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let scans_after_stream = fault_kv.pending_scans();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let quiet = fault_kv.pending_scans() - scans_after_stream;
    assert!(quiet <= 1, "{quiet} scans in 1.5 s without creates");
    waiting.abort();
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
    assert!(handled_at - queued_at < Duration::from_millis(1500));
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
    wait_for_open_waits(&server, 1).await;
    assert_eq!(server.command_server.open_lease_waits(), 1);

    let started = Instant::now();
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
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

/// Stand-in for a manager that holds each request for 2.1 s, then answers
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
                    tokio::time::sleep(Duration::from_millis(2100)).await;
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
        .with_lease_wait(Duration::from_secs(2))
        .with_poll_jitter(0.0);
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(5000)).await;
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    // 2.1 s each, polled again at once: at least 3 polls started in 5 s (with
    // the 30 s poll interval it would be 1).
    assert!((3..=4).contains(&count.load(Ordering::SeqCst)));
}

/// Stand-in manager: every lease request is held for `hold`, then answered by
/// `answer`. Returns its URL and the arrival time of each request.
async fn slow_manager(
    hold: Duration,
    status: axum::http::StatusCode,
) -> (String, Arc<Mutex<Vec<Instant>>>) {
    let arrivals = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().route(
        "/v1/commands/leases",
        post({
            let arrivals = arrivals.clone();
            move || {
                let arrivals = arrivals.clone();
                async move {
                    arrivals.lock().unwrap().push(Instant::now());
                    tokio::time::sleep(hold).await;
                    (status, Json(serde_json::json!({"leases": []})))
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/v1/"), arrivals)
}

fn gaps(arrivals: &Arc<Mutex<Vec<Instant>>>) -> Vec<Duration> {
    let arrivals = arrivals.lock().unwrap();
    arrivals.windows(2).map(|pair| pair[1] - pair[0]).collect()
}

/// An older manager that is merely slow (1.5 s) answers empty without having
/// waited: the answer is far shorter than the requested 25 s, so the receiver
/// keeps its backoff instead of polling again at once.
#[tokio::test]
async fn a_slow_manager_that_does_not_wait_keeps_the_backoff() {
    let (url, arrivals) =
        slow_manager(Duration::from_millis(1500), axum::http::StatusCode::OK).await;
    let receiver = Receiver::from_env_vars(&receiver_env(&url, "x"))
        .unwrap()
        .with_poll_interval(Duration::from_secs(2))
        .with_poll_max_interval(Duration::from_secs(2))
        .with_poll_jitter(0.0);
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(6000)).await;
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    let gaps = gaps(&arrivals);
    assert!(!gaps.is_empty());
    // 1.5 s held, then the 2 s backoff: never polled again at once (which
    // would be about 1.75 s after the previous start).
    for gap in gaps {
        assert!(
            gap >= Duration::from_millis(3300),
            "polled again after {gap:?}"
        );
    }
}

/// An error answer after a long hold keeps today's backoff, and so does a
/// connection that is cut.
#[tokio::test]
async fn a_5xx_during_a_wait_keeps_the_backoff() {
    let (url, arrivals) = slow_manager(
        Duration::from_millis(300),
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    let receiver = Receiver::from_env_vars(&receiver_env(&url, "x"))
        .unwrap()
        .with_poll_interval(Duration::from_secs(1))
        .with_poll_max_interval(Duration::from_secs(1))
        .with_poll_jitter(0.0);
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(4000)).await;
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    let gaps = gaps(&arrivals);
    assert!(!gaps.is_empty());
    for gap in gaps {
        assert!(
            gap >= Duration::from_millis(1200),
            "polled again after {gap:?}"
        );
    }
}

#[tokio::test]
async fn a_cut_connection_during_a_wait_keeps_the_backoff() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let arrivals = Arc::new(Mutex::new(Vec::<Instant>::new()));
    tokio::spawn({
        let arrivals = arrivals.clone();
        async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                arrivals.lock().unwrap().push(Instant::now());
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                tokio::time::sleep(Duration::from_millis(300)).await;
                drop(stream); // cut without an answer
            }
        }
    });
    let receiver = Receiver::from_env_vars(&receiver_env(&format!("http://{addr}/v1/"), "x"))
        .unwrap()
        .with_poll_interval(Duration::from_secs(1))
        .with_poll_max_interval(Duration::from_secs(1))
        .with_poll_jitter(0.0);
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(4000)).await;
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    let gaps = gaps(&arrivals);
    assert!(!gaps.is_empty());
    for gap in gaps {
        assert!(
            gap >= Duration::from_millis(1200),
            "polled again after {gap:?}"
        );
    }
}

/// A poll that could still be the immediate answer is awaited at shutdown: the
/// manager scans, takes its time, writes a lease; the receiver handles it.
#[tokio::test]
async fn a_poll_that_may_be_an_immediate_answer_is_awaited_at_shutdown() {
    let server = TestCommandServer::builder()
        .with_pull_mode()
        .with_fault_injection()
        .build()
        .await;
    server
        .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
        .await
        .unwrap();
    // The scan has answered; leasing is slow (700 ms), as on a loaded store.
    server
        .fault_kv
        .as_ref()
        .unwrap()
        .arm_after_next_pending_scan(Box::new(|| {
            Box::pin(async { tokio::time::sleep(Duration::from_millis(700)).await })
        }));
    let handled = Arc::new(AtomicUsize::new(0));
    let mut receiver = Receiver::from_env_vars(&receiver_env(
        &server.command_base_url(),
        &server.default_target.resource_id,
    ))
    .unwrap();
    receiver.command("read", {
        let handled = handled.clone();
        move |_: serde_json::Value, _ctx| {
            let handled = handled.clone();
            async move {
                handled.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({}))
            }
        }
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    shutdown.shutdown(); // the poll is 300 ms old: it may be the answer
    run.await.unwrap().unwrap();
    assert_eq!(
        handled.load(Ordering::SeqCst),
        1,
        "the leased command was dropped"
    );
}

#[tokio::test]
async fn the_wait_setting_zero_turns_waiting_off() {
    let (url, count, bodies) = old_manager().await;
    let mut env = receiver_env(&url, "x");
    env.insert(
        "ALIEN_COMMANDS_LEASE_WAIT_SECONDS".to_string(),
        "0".to_string(),
    );
    let receiver = Receiver::from_env_vars(&env)
        .unwrap()
        .with_poll_interval(Duration::from_millis(100))
        .with_poll_max_interval(Duration::from_millis(100))
        .with_poll_jitter(0.0);
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    assert!(count.load(Ordering::SeqCst) >= 2);
    assert!(bodies
        .lock()
        .unwrap()
        .iter()
        .all(|body| body["waitSeconds"] == 0));
}

#[tokio::test]
async fn the_wait_setting_is_read_and_checked() {
    let mut env = receiver_env("http://127.0.0.1:1/v1/", "x");
    env.insert(
        "ALIEN_COMMANDS_LEASE_WAIT_SECONDS".to_string(),
        "10".to_string(),
    );
    assert!(Receiver::from_env_vars(&env).is_ok());
    env.insert(
        "ALIEN_COMMANDS_LEASE_WAIT_SECONDS".to_string(),
        "26".to_string(),
    );
    let error = Receiver::from_env_vars(&env).unwrap_err();
    assert!(error.message.contains("ALIEN_COMMANDS_LEASE_WAIT_SECONDS"));
    env.insert(
        "ALIEN_COMMANDS_LEASE_WAIT_SECONDS".to_string(),
        "soon".to_string(),
    );
    assert!(Receiver::from_env_vars(&env).is_err());
}

/// Request-to-lease time for 20 commands queued at uneven gaps (6 to 14 s)
/// while a receiver with production poll settings is idle between them.
/// Run by hand, once per mode:
///   LEASE_WAIT_SECONDS=0  cargo test ... measure_request_to_lease -- --ignored --nocapture   (before: no wait)
///   LEASE_WAIT_SECONDS=25 cargo test ... measure_request_to_lease -- --ignored --nocapture   (after)
#[tokio::test]
#[ignore = "takes minutes; measurement"]
async fn measure_request_to_lease() {
    let wait: u64 = std::env::var("LEASE_WAIT_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(25);
    let server = pull_server().await;
    let (seen_tx, mut seen_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut receiver = Receiver::from_env_vars(&receiver_env(
        &server.command_base_url(),
        &server.default_target.resource_id,
    ))
    .unwrap()
    .with_lease_wait(Duration::from_secs(wait));
    receiver.command("read", move |_: serde_json::Value, _ctx| {
        let seen_tx = seen_tx.clone();
        async move {
            seen_tx.send(Instant::now()).unwrap();
            Ok(serde_json::json!({}))
        }
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });

    let gaps = [
        6, 9, 12, 7, 14, 8, 11, 6, 13, 10, 7, 12, 9, 14, 6, 8, 11, 13, 7, 10,
    ];
    let mut latencies = Vec::new();
    for gap in gaps {
        tokio::time::sleep(Duration::from_secs(gap)).await;
        server
            .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
            .await
            .unwrap();
        let queued_at = Instant::now();
        let handled_at = tokio::time::timeout(Duration::from_secs(60), seen_rx.recv())
            .await
            .expect("handled")
            .unwrap();
        latencies.push(handled_at.saturating_duration_since(queued_at));
    }
    shutdown.shutdown();
    run.await.unwrap().unwrap();

    let mut secs: Vec<f64> = latencies.iter().map(Duration::as_secs_f64).collect();
    println!("waitSeconds={wait} latencies (s): {secs:.2?}");
    secs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "waitSeconds={wait} min {:.2} median {:.2} p90 {:.2} max {:.2} mean {:.2}",
        secs[0],
        secs[secs.len() / 2],
        secs[(secs.len() * 9) / 10],
        secs[secs.len() - 1],
        secs.iter().sum::<f64>() / secs.len() as f64
    );
}

// ---- repoll after a non-empty answer -------------------------------------------

#[tokio::test]
async fn commands_queued_a_second_apart_are_each_picked_up_within_a_second() {
    let server = pull_server().await;
    let (seen_tx, mut seen_rx) = tokio::sync::mpsc::unbounded_channel();
    // Default poll interval (5 s): only an immediate repoll can be fast enough.
    let mut receiver = Receiver::from_env_vars(&receiver_env(
        &server.command_base_url(),
        &server.default_target.resource_id,
    ))
    .unwrap();
    receiver.command("read", move |_: serde_json::Value, _ctx| {
        let seen_tx = seen_tx.clone();
        async move {
            seen_tx.send(Instant::now()).unwrap();
            Ok(serde_json::json!({}))
        }
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(300)).await;

    for _ in 0..3 {
        server
            .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
            .await
            .unwrap();
        let queued_at = Instant::now();
        let handled_at = tokio::time::timeout(Duration::from_secs(4), seen_rx.recv())
            .await
            .expect("handled")
            .unwrap();
        assert!(
            handled_at - queued_at < Duration::from_millis(1500),
            "picked up after {:?}",
            handled_at - queued_at
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    shutdown.shutdown();
    run.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_queue_of_commands_is_drained_at_the_floor_rate_not_faster_or_at_the_poll_interval() {
    let server = pull_server().await;
    let handled = Arc::new(AtomicUsize::new(0));
    let mut receiver = Receiver::from_env_vars(&receiver_env(
        &server.command_base_url(),
        &server.default_target.resource_id,
    ))
    .unwrap()
    .with_poll_jitter(0.0);
    receiver.command("read", {
        let handled = handled.clone();
        move |_: serde_json::Value, _ctx| {
            let handled = handled.clone();
            async move {
                handled.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({}))
            }
        }
    });
    for _ in 0..8 {
        server
            .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
            .await
            .unwrap();
    }
    let started = Instant::now();
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    while handled.load(Ordering::SeqCst) < 8 {
        assert!(started.elapsed() < Duration::from_secs(10), "too slow");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let took = started.elapsed();
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    // One lease per request, at least 250 ms apart: 7 gaps of 250 ms. With the
    // 5 s poll interval it would take 35 s.
    assert!(took >= Duration::from_millis(1700), "no floor: {took:?}");
    assert!(took < Duration::from_secs(12), "too slow: {took:?}");
}

/// An agent asks for three read operations at once; the receiver runs one at
/// a time (max_leases 1). The next starts when the previous finishes, not at
/// the next poll interval.
#[tokio::test]
async fn three_commands_queued_at_once_run_back_to_back_with_one_slot() {
    let server = pull_server().await;
    let finished = Arc::new(Mutex::new(Vec::<Instant>::new()));
    let mut receiver = Receiver::from_env_vars(&receiver_env(
        &server.command_base_url(),
        &server.default_target.resource_id,
    ))
    .unwrap();
    receiver.command("read", {
        let finished = finished.clone();
        move |_: serde_json::Value, _ctx| {
            let finished = finished.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                finished.lock().unwrap().push(Instant::now());
                Ok(serde_json::json!({}))
            }
        }
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });
    tokio::time::sleep(Duration::from_millis(300)).await;

    let started = Instant::now();
    for _ in 0..3 {
        server
            .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
            .await
            .unwrap();
    }
    while finished.lock().unwrap().len() < 3 {
        assert!(started.elapsed() < Duration::from_secs(8), "too slow");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    println!(
        "three commands, one slot: all done after {:?}",
        started.elapsed()
    );
    assert!(started.elapsed() < Duration::from_secs(6));
    shutdown.shutdown();
    run.await.unwrap().unwrap();
}

fn proc_status_kb(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix(field))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap()
}

fn open_fds() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}

/// Cost of holding the default maximum of open waits: memory and file
/// descriptors of the manager side. Linux only. Raise `ulimit -n` to 32768
/// first. Clients are bare sockets that send one request and read nothing, so
/// their cost is one descriptor each.
///   cargo test ... measure_open_waits -- --ignored --nocapture
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "measurement; needs a high descriptor limit"]
async fn measure_open_waits() {
    use tokio::io::AsyncWriteExt;
    const WAITS: usize = 5_000;
    // Spread over the per-process cap's worth of keys is not needed: the per-target
    // bound is lifted so one target can hold them all.
    let server = TestCommandServer::builder()
        .with_pull_mode()
        .with_max_open_lease_waits_per_target(WAITS + 1)
        .build()
        .await;
    let body = serde_json::json!({
        "deploymentId": DEPLOYMENT_ID,
        "target": {"resourceId": server.default_target.resource_id, "resourceType": "daemon"},
        "maxLeases": 1, "leaseSeconds": 60, "waitSeconds": 25,
    })
    .to_string();
    let request = format!(
        "POST /v1/commands/leases HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let (rss_before, fds_before) = (proc_status_kb("VmRSS:"), open_fds());
    let mut sockets = Vec::new();
    for _ in 0..WAITS {
        let mut stream = tokio::net::TcpStream::connect(server.server_addr)
            .await
            .unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        sockets.push(stream);
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    while server.command_server.open_lease_waits() < WAITS {
        assert!(Instant::now() < deadline, "waits did not open");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (rss_after, fds_after) = (proc_status_kb("VmRSS:"), open_fds());
    println!(
        "{WAITS} open waits: RSS {rss_before} kB -> {rss_after} kB (+{} kB, {} bytes per wait); fds {fds_before} -> {fds_after} (+{}; half are the test's own client sockets, so {} on the manager side)",
        rss_after - rss_before,
        (rss_after - rss_before) * 1024 / WAITS as u64,
        fds_after - fds_before,
        (fds_after - fds_before) / 2
    );
    // Over the cap: answered at once.
    let started = Instant::now();
    let answer = client(&server)
        .acquire(&lease_request(&server, 25))
        .await
        .unwrap();
    assert!(answer.is_empty() && started.elapsed() < Duration::from_secs(3));
    drop(sockets);
    let deadline = Instant::now() + Duration::from_secs(30);
    while server.command_server.open_lease_waits() > 0 {
        assert!(Instant::now() < deadline, "waits not released after close");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!("all waits released after the clients closed");
}
