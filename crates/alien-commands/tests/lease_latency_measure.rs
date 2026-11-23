//! Measures the time from creating a command to its handler starting, for a
//! receiver that does not wait on the manager (the behavior before waiting
//! leases) and for one that does. Run by hand; it takes minutes:
//!
//! `cargo test -p alien-commands --features receiver,test-utils --test lease_latency_measure -- --ignored --nocapture`

#![cfg(all(feature = "receiver", feature = "test-utils"))]

use std::collections::HashMap;
use std::time::{Duration, Instant};

use alien_commands::receiver::Receiver;
use alien_commands::test_utils::{test_inline_create_command, TestCommandServer};
use alien_core::{
    ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
    ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_COMMANDS_URL, ENV_ALIEN_DEPLOYMENT_ID,
};

const DEPLOYMENT_ID: &str = "pull-agent";
const RUNS: usize = 20;

async fn measure(wait: Duration) -> Vec<Duration> {
    let server = TestCommandServer::builder().with_pull_mode().build().await;
    let env = HashMap::from([
        (
            ENV_ALIEN_COMMANDS_URL.to_string(),
            server.command_base_url(),
        ),
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
            server.default_target.resource_id.clone(),
        ),
        (
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE.to_string(),
            "daemon".to_string(),
        ),
    ]);
    let (seen_tx, mut seen_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut receiver = Receiver::from_env_vars(&env).unwrap().with_lease_wait(wait);
    receiver.command("read", move |_: serde_json::Value, _ctx| {
        let seen_tx = seen_tx.clone();
        async move {
            let _ = seen_tx.send(Instant::now());
            Ok(serde_json::json!({}))
        }
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });

    let mut samples = Vec::new();
    for i in 0..RUNS {
        // Spread the requests over the receiver's poll cycle: 1.0 s to 8.7 s
        // of idleness before each, in steps that do not divide the interval.
        tokio::time::sleep(Duration::from_millis(1_000 + (i as u64 * 1_300) % 7_700)).await;
        let queued_at = Instant::now();
        server
            .create_command(test_inline_create_command(DEPLOYMENT_ID, "read"))
            .await
            .unwrap();
        let handled_at = tokio::time::timeout(Duration::from_secs(120), seen_rx.recv())
            .await
            .expect("handled")
            .unwrap();
        samples.push(handled_at - queued_at);
    }
    shutdown.shutdown();
    run.await.unwrap().unwrap();
    samples
}

fn report(label: &str, mut samples: Vec<Duration>) {
    samples.sort();
    let median = (samples[RUNS / 2 - 1] + samples[RUNS / 2]) / 2;
    let p95 = samples[(RUNS * 95).div_ceil(100) - 1];
    println!(
        "{label}: runs={RUNS} median={:.0} ms p95={:.0} ms min={:.0} ms max={:.0} ms",
        median.as_secs_f64() * 1e3,
        p95.as_secs_f64() * 1e3,
        samples[0].as_secs_f64() * 1e3,
        samples[RUNS - 1].as_secs_f64() * 1e3,
    );
}

#[tokio::test]
#[ignore = "measurement; takes minutes"]
async fn time_from_request_to_start() {
    report(
        "before (no waiting lease: receiver polls)",
        measure(Duration::ZERO).await,
    );
    report(
        "after  (waiting lease, 25 s)",
        measure(Duration::from_secs(25)).await,
    );
}
