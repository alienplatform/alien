//! Shared assertions for generated setup and controller-provisioned notification stacks.

use std::{collections::BTreeSet, time::Duration};

use alien_core::{StackState, StorageOutputs};
use alien_test::TestDeployment;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::time::{sleep, Instant};
use tracing::info;

async fn read_batches(client: &Client, url: &str) -> Vec<Value> {
    let response = client.get(format!("{url}/batches")).send().await.unwrap();
    assert!(
        response.status().is_success(),
        "{}",
        response.text().await.unwrap()
    );
    response.json().await.unwrap()
}

/// Probes written while waiting for a rule change to take effect. They are not part of
/// any cohort and are ignored by its assertions.
fn is_settle_probe(key: &str) -> bool {
    key.rsplit('/')
        .next()
        .is_some_and(|name| name.starts_with("settle-"))
}

async fn write_objects(client: &Client, url: &str, writes: &[Value]) {
    let response = client
        .post(format!("{url}/objects"))
        .json(writes)
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "{}",
        response.text().await.unwrap()
    );
    let result: Value = response.json().await.unwrap();
    assert_eq!(result["written"], writes.len());
}

fn delivered(batches: &[Value], key: &str) -> bool {
    batches.iter().any(|batch| {
        batch["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["event"]["objectKey"] == key)
    })
}

/// Providers apply notification changes asynchronously; S3 kept routing a replaced filter
/// for several minutes in qualification. Wait until a probe under the new prefix arrives
/// while one under the old prefix, written in the same round, does not, so the cohort
/// that follows judges the changed rule rather than propagation.
async fn wait_until_rule_change_applies(client: &Client, url: &str, new: &str, old: &str) {
    let started = Instant::now();
    for round in 0.. {
        assert!(
            started.elapsed() < Duration::from_secs(600),
            "the changed notification rule did not take effect within 10 minutes"
        );
        let (fresh, stale) = (
            format!("{new}settle-{round}.json"),
            format!("{old}settle-{round}.json"),
        );
        write_objects(
            client,
            url,
            &[
                json!({"storage":"objects","key":fresh,"content":"payload"}),
                json!({"storage":"objects","key":stale,"content":"payload"}),
            ],
        )
        .await;
        let probe_started = Instant::now();
        let mut fresh_at = None;
        while probe_started.elapsed() < Duration::from_secs(90) {
            let batches = read_batches(client, url).await;
            if delivered(&batches, &stale) {
                break;
            }
            if delivered(&batches, &fresh) {
                // Give the old rule's delivery the same chance before trusting the round.
                let at = *fresh_at.get_or_insert_with(Instant::now);
                if at.elapsed() >= Duration::from_secs(30) {
                    info!(
                        elapsed_seconds = started.elapsed().as_secs_f64(),
                        round, "Changed notification rule took effect"
                    );
                    return;
                }
            }
            sleep(Duration::from_secs(2)).await;
        }
    }
}

/// Verify canonical payloads, both filters, cross-bucket coexistence, and actual batching.
/// Every invocation is recorded separately, so a wrong route or payload cannot hide behind
/// a key-indexed projection. Cloud delivery is at least once: duplicates remain visible in
/// the records and are permitted, but unexpected keys and missing keys always fail.
pub async fn check_delivery(deployment: &TestDeployment) {
    check_cohort(deployment, "", "in/").await;
}

/// A new cohort proves the changed rule and both untouched rules after setup reruns.
pub async fn check_updated_delivery(deployment: &TestDeployment) {
    check_cohort(deployment, "updated-", "changed/").await;
}

fn matching_objects(phase: &str, prefix: &str) -> Vec<(&'static str, String)> {
    let mut objects = vec![
        ("objects", format!("{prefix}{phase}quote\"é*%.json")),
        ("objects", format!("other/{phase}b.data")),
        ("objects", format!("other/{phase}d.csv")),
        ("second", format!("shared/{phase}c.json")),
    ];
    objects.extend((0..24).map(|index| ("objects", format!("{prefix}{phase}bulk-{index}.json"))));
    objects
}

async fn check_cohort(deployment: &TestDeployment, phase: &str, prefix: &str) {
    let started = Instant::now();
    let installed = deployment
        .manager()
        .client()
        .get_deployment()
        .id(&deployment.id)
        .send()
        .await
        .unwrap();
    let state: StackState = serde_json::from_value(
        serde_json::to_value(installed.stack_state.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
    let bucket_name = |id: &str| {
        state.resources[id]
            .outputs
            .as_ref()
            .unwrap()
            .downcast_ref::<StorageOutputs>()
            .unwrap()
            .bucket_name
            .as_str()
    };
    let expected_first_bucket = bucket_name("objects");
    let expected_second_bucket = bucket_name("second");
    assert_ne!(expected_first_bucket, expected_second_bucket);
    let url = super::bindings::deployment_url(deployment).unwrap();
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    if !phase.is_empty() {
        wait_until_rule_change_applies(&client, url, prefix, "in/").await;
    }
    let previous = read_batches(&client, url).await;
    assert_eq!(
        previous.is_empty(),
        phase.is_empty(),
        "cohort history must be preserved"
    );
    let mut expected = BTreeSet::new();
    for batch in &previous {
        for record in batch["events"].as_array().unwrap() {
            let key = record["event"]["objectKey"].as_str().unwrap();
            if !is_settle_probe(key) {
                expected.insert(key.to_string());
            }
        }
    }
    if !phase.is_empty() {
        let original: BTreeSet<_> = matching_objects("", "in/")
            .into_iter()
            .map(|(_, key)| key)
            .collect();
        assert_eq!(
            expected, original,
            "late excluded deliveries must not become accepted history"
        );
    }
    let previous_keys = expected.clone();
    let mut writes = vec![];
    for (source, key) in matching_objects(phase, prefix) {
        expected.insert(key.to_string());
        writes.push(json!({"storage":source,"key":key,"content":"payload"}));
    }
    for key in [
        format!("{prefix}{phase}excluded.txt"),
        format!("other/{phase}excluded.json"),
        format!("IN/{phase}excluded.json"),
    ] {
        writes.push(json!({"storage":"objects","key":key,"content":"excluded"}));
    }
    if !phase.is_empty() {
        writes.push(json!({"storage":"objects","key":"in/removed-rule.json","content":"excluded"}));
    }
    write_objects(&client, url, &writes).await;
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut complete_at = None;
    loop {
        let batches = read_batches(&client, url).await;
        let mut seen = BTreeSet::new();
        let mut batched = false;
        let mut first_bucket = None;
        let mut second_bucket = None;
        for batch in &batches {
            assert!(!batch["source"].as_str().unwrap().is_empty());
            let events = batch["events"].as_array().unwrap();
            assert!(!events.is_empty() && events.len() <= 10);
            let cohort: Vec<&str> = events
                .iter()
                .map(|record| record["event"]["objectKey"].as_str().unwrap())
                .filter(|key| !is_settle_probe(key))
                .collect();
            batched |= cohort.len() > 1 && cohort.iter().any(|key| !previous_keys.contains(*key));
            for record in events {
                if is_settle_probe(record["event"]["objectKey"].as_str().unwrap()) {
                    continue;
                }
                assert!(!record["messageId"].as_str().unwrap().is_empty());
                assert!(!record["deliveryId"].as_str().unwrap().is_empty());
                let event = &record["event"];
                let key = event["objectKey"].as_str().unwrap();
                assert!(
                    expected.contains(key),
                    "excluded key reached the Worker: {event}"
                );
                assert_eq!(event["eventType"], "created");
                assert_eq!(event["size"], 7);
                assert!(!event["etag"].as_str().unwrap().is_empty());
                chrono::DateTime::parse_from_rfc3339(event["timestamp"].as_str().unwrap()).unwrap();
                let bucket = event["bucketName"].as_str().unwrap();
                assert!(!bucket.is_empty());
                assert_eq!(
                    bucket,
                    if key.starts_with("shared/") {
                        expected_second_bucket
                    } else {
                        expected_first_bucket
                    }
                );
                let observed = if key.starts_with("shared/") {
                    &mut second_bucket
                } else {
                    &mut first_bucket
                };
                assert_eq!(*observed.get_or_insert(bucket), bucket);
                seen.insert(key.to_string());
            }
        }
        if seen == expected && batched {
            assert_ne!(
                first_bucket, second_bucket,
                "events must preserve source bucket identity"
            );
            // Keep observing after success so delayed excluded messages still fail.
            let completed = *complete_at.get_or_insert_with(Instant::now);
            if completed.elapsed() >= Duration::from_secs(30) {
                let batch_sizes: Vec<_> = batches
                    .iter()
                    .map(|batch| batch["events"].as_array().unwrap().len())
                    .collect();
                info!(
                    phase,
                    elapsed_seconds = started.elapsed().as_secs_f64(),
                    unique_events = seen.len(),
                    ?batch_sizes,
                    "Deployed Worker notification cohort passed"
                );
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "missing keys or no multi-message batch: expected {expected:?}, observed {batches:?}"
        );
        sleep(Duration::from_secs(1)).await;
    }
}
