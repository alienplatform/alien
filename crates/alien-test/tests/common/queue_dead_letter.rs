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
    let request = request.build().unwrap();
    let what = format!("{} {}", request.method(), request.url().path());
    let started = Instant::now();
    let response = client.execute(request).await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(
        status.is_success(),
        "{what} answered {status} after {:?}: {body}",
        started.elapsed()
    );
    serde_json::from_str(&body).unwrap_or_else(|error| panic!("{what}: {error}: {body}"))
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
    peek_tasks: bool,
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
    if peek_tasks {
        // `tasks` ends empty: nothing is left to receive. (On GCP the queue's default pull
        // subscription also holds a copy of every message for `receive` consumers, so the
        // trigger path is judged by its deliveries instead.)
        sleep(Duration::from_secs(20)).await;
        let left = json_request(&client, client.post(format!("{url}/tasks/peek"))).await;
        assert_eq!(left["visible"], 0, "tasks must end empty: {left}");
    }
    counts
}

/// The pull path: `pulled` has no trigger; the Worker consumes it with `receive`, acknowledging
/// good messages and failing (nacking) the poison ones. Each poison message must end in
/// `pulled-failed` with its payload after about five deliveries, and `pulled` ends empty.
pub async fn check_pull(deployment: &TestDeployment, prefix: &str) -> BTreeMap<String, u64> {
    let url = super::bindings::deployment_url(deployment).unwrap();
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();
    warm_up(&client, url).await;
    let sent = json_request(
        &client,
        client
            .post(format!("{url}/pull/send"))
            .json(&json!({"good": GOOD, "poison": POISON, "prefix": prefix})),
    )
    .await;
    assert_eq!(sent["sent"], GOOD + POISON);

    let started = Instant::now();
    let mut failed: BTreeMap<String, Value> = BTreeMap::new();
    let mut attempts: BTreeMap<String, u64> = BTreeMap::new();
    let mut good_seen = BTreeSet::new();
    while !(failed.len() == POISON && good_seen.len() == GOOD) {
        assert!(
            started.elapsed() < Duration::from_secs(30 * 60),
            "the pull path did not finish: failed={failed:?} good_seen={}",
            good_seen.len()
        );
        let seen = json_request(&client, client.post(format!("{url}/pull/consume"))).await;
        for entry in seen.as_array().unwrap() {
            let id = entry["id"].as_str().unwrap().to_string();
            let attempt = entry["attempt"].as_u64().unwrap();
            if entry["poison"].as_bool().unwrap() {
                let highest = attempts.entry(id).or_default();
                *highest = (*highest).max(attempt);
            } else {
                good_seen.insert(id);
            }
        }
        let drained = json_request(&client, client.post(format!("{url}/pull/failed/drain"))).await;
        for entry in drained.as_array().unwrap() {
            let id = entry["task"]["id"].as_str().unwrap().to_string();
            failed.insert(id, entry.clone());
        }
        if seen.as_array().unwrap().is_empty() {
            sleep(Duration::from_secs(5)).await;
        }
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
        let highest = attempts[&id];
        assert!(
            (MAX_ATTEMPTS - 1..=MAX_ATTEMPTS + 1).contains(&highest),
            "{id}: the last reported attempt was {highest}; about {MAX_ATTEMPTS} expected"
        );
    }
    sleep(Duration::from_secs(10)).await;
    let left = json_request(&client, client.post(format!("{url}/pull/peek"))).await;
    assert_eq!(left["visible"], 0, "pulled must end empty: {left}");
    info!(
        ?attempts,
        "pull path: highest attempt reported per poison message"
    );
    attempts
}

/// The dead-letter check on the trigger path and on the pull path, or the same run without the
/// setting when `DLQ_E2E_NO_DEAD_LETTER=1`. `peek_tasks` is false where the trigger's queue also
/// keeps a pull copy of every message (GCP).
pub async fn check(deployment: &TestDeployment, prefix: &str, peek_tasks: bool) {
    if std::env::var("DLQ_E2E_NO_DEAD_LETTER").as_deref() == Ok("1") {
        check_without_dead_letter(deployment, prefix).await;
    } else {
        check_dead_letter(deployment, prefix, peek_tasks).await;
        check_pull(deployment, prefix).await;
    }
}
