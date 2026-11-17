//! Shared assertions for the dead-letter stack: a Worker fails every message marked `poison`,
//! so each one must end in `tasks-failed` with its payload after the configured attempts, while
//! every good message is processed.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use alien_test::TestDeployment;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::time::{sleep, Instant};
use tracing::info;

const GOOD: usize = 20;
const POISON: usize = 3;
const MAX_ATTEMPTS: u64 = 5;

/// Waits until the Worker answers, so the first real request does not pay for a cold start.
async fn warm_up(client: &Client, url: &str) {
    for _ in 0..30 {
        if let Ok(response) = client.get(format!("{url}/health")).send().await {
            if response.status().is_success() {
                return;
            }
        }
        sleep(Duration::from_secs(5)).await;
    }
    panic!("the Worker never answered /health");
}

async fn json_request(client: &Client, request: reqwest::RequestBuilder) -> Value {
    let _ = client;
    let response = request.send().await.unwrap();
    assert!(
        response.status().is_success(),
        "{}",
        response.text().await.unwrap()
    );
    response.json().await.unwrap()
}

/// Without a dead-letter setting (`DLQ_E2E_NO_DEAD_LETTER=1`) a message that always fails keeps
/// being redelivered. Sends the same messages, waits until every poison message was delivered
/// more than `MAX_ATTEMPTS` times, and checks that nothing reached `tasks-failed`. Returns the
/// delivery counts.
pub async fn check_without_dead_letter(
    deployment: &TestDeployment,
    prefix: &str,
) -> BTreeMap<String, usize> {
    let url = super::bindings::deployment_url(deployment).unwrap();
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();
    warm_up(&client, url).await;
    let sent = json_request(
        &client,
        client
            .post(format!("{url}/send"))
            .json(&json!({"good": GOOD, "poison": POISON, "prefix": prefix})),
    )
    .await;
    assert_eq!(sent["sent"], GOOD + POISON);
    let started = Instant::now();
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(45 * 60),
            "poison messages were not redelivered past {MAX_ATTEMPTS} attempts"
        );
        let deliveries = json_request(&client, client.get(format!("{url}/deliveries"))).await;
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for record in deliveries.as_array().unwrap() {
            let id = record["id"].as_str().unwrap();
            if id.starts_with(prefix) {
                *counts.entry(id.to_string()).or_default() += 1;
            }
        }
        let past = (0..POISON).all(|n| {
            counts
                .get(&format!("{prefix}-poison-{n}"))
                .copied()
                .unwrap_or(0) as u64
                > MAX_ATTEMPTS
        });
        if past {
            let drained = json_request(&client, client.post(format!("{url}/failed/drain"))).await;
            assert!(
                drained.as_array().unwrap().is_empty(),
                "nothing may reach tasks-failed without a dead-letter setting: {drained}"
            );
            info!(
                ?counts,
                "redelivered past {MAX_ATTEMPTS} attempts, no dead-letter move"
            );
            return counts;
        }
        sleep(Duration::from_secs(15)).await;
    }
}

/// Sends the messages, then waits for every good message to be processed and every poison
/// message to arrive in `tasks-failed`, and returns the delivery counts the Worker recorded.
pub async fn check_dead_letter(
    deployment: &TestDeployment,
    prefix: &str,
) -> BTreeMap<String, usize> {
    let url = super::bindings::deployment_url(deployment).unwrap();
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();
    warm_up(&client, url).await;
    let sent = json_request(
        &client,
        client
            .post(format!("{url}/send"))
            .json(&json!({"good": GOOD, "poison": POISON, "prefix": prefix})),
    )
    .await;
    assert_eq!(sent["sent"], GOOD + POISON);

    let started = Instant::now();
    let mut failed: BTreeMap<String, Value> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut good_seen = BTreeSet::new();
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(45 * 60),
            "dead-lettering did not finish: failed={failed:?} good_seen={}",
            good_seen.len()
        );
        // The pull path: whatever has arrived in `tasks-failed` so far.
        let drained = json_request(&client, client.post(format!("{url}/failed/drain"))).await;
        for entry in drained.as_array().unwrap() {
            let id = entry["task"]["id"].as_str().unwrap().to_string();
            failed.insert(id, entry.clone());
        }
        let deliveries = json_request(&client, client.get(format!("{url}/deliveries"))).await;
        counts.clear();
        good_seen.clear();
        for record in deliveries.as_array().unwrap() {
            let id = record["id"].as_str().unwrap();
            if !id.starts_with(prefix) {
                continue;
            }
            *counts.entry(id.to_string()).or_default() += 1;
            if !record["poison"].as_bool().unwrap() {
                good_seen.insert(id.to_string());
            }
        }
        if failed.len() == POISON && good_seen.len() == GOOD {
            break;
        }
        sleep(Duration::from_secs(15)).await;
    }

    for n in 0..POISON {
        let id = format!("{prefix}-poison-{n}");
        let entry = failed
            .get(&id)
            .unwrap_or_else(|| panic!("{id} not dead-lettered"));
        assert_eq!(
            entry["task"],
            json!({"id": id, "poison": true}),
            "payload intact"
        );
        let attempts = counts[&id] as u64;
        assert!(
            (MAX_ATTEMPTS..=MAX_ATTEMPTS + 2).contains(&attempts),
            "{id} was delivered {attempts} times; about {MAX_ATTEMPTS} expected"
        );
    }
    for n in 0..GOOD {
        assert!(counts[&format!("{prefix}-good-{n}")] >= 1);
    }
    info!(?counts, "dead-letter delivery counts");
    counts
}

/// The dead-letter check, or the same run without the setting when `DLQ_E2E_NO_DEAD_LETTER=1`.
pub async fn check(deployment: &TestDeployment, prefix: &str) -> BTreeMap<String, usize> {
    if std::env::var("DLQ_E2E_NO_DEAD_LETTER").as_deref() == Ok("1") {
        check_without_dead_letter(deployment, prefix).await
    } else {
        check_dead_letter(deployment, prefix).await
    }
}
