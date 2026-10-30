//! Shared assertions for generated setup and controller-provisioned notification stacks.

use std::{collections::BTreeSet, time::Duration};

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

/// Verify canonical payloads, both filters, cross-bucket coexistence, and actual batching.
/// Every invocation is recorded separately, so a wrong route or payload cannot hide behind
/// a key-indexed projection. Cloud delivery is at least once: duplicates remain visible in
/// the records and are permitted, but unexpected keys and missing keys always fail.
pub async fn check_delivery(deployment: &TestDeployment) {
    let started = Instant::now();
    let url = super::bindings::deployment_url(deployment).unwrap();
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    assert!(
        read_batches(&client, url).await.is_empty(),
        "fresh fixture has no deliveries"
    );
    let mut expected = BTreeSet::new();
    let mut writes = vec![];
    for (source, key) in [
        ("objects", "in/quote\"é*%.json"),
        ("objects", "other/b.data"),
        ("second", "shared/c.json"),
    ] {
        expected.insert(key.to_string());
        writes.push(json!({"storage":source,"key":key,"content":"payload"}));
    }
    for index in 0..24 {
        let key = format!("in/bulk-{index}.json");
        expected.insert(key.clone());
        writes.push(json!({"storage":"objects","key":key,"content":"payload"}));
    }
    for key in ["in/excluded.txt", "other/excluded.json", "IN/excluded.json"] {
        writes.push(json!({"storage":"objects","key":key,"content":"excluded"}));
    }
    let response = client
        .post(format!("{url}/objects"))
        .json(&writes)
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
            batched |= events.len() > 1;
            for record in events {
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
