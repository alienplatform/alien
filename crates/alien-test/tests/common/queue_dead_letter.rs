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

/// The fewest deliveries (or reported attempts) a dead-lettered message may show. AWS and Azure
/// count exactly, so anything below the limit means the setting was applied off by one. Only
/// Pub/Sub counts push attempts the Worker never saw (a cold start answering 5xx), so a recorded
/// count can be one short there.
fn lowest_recorded(platform: &str) -> u64 {
    if platform == "gcp" {
        MAX_ATTEMPTS - 1
    } else {
        MAX_ATTEMPTS
    }
}

/// How often a check that needs an empty queue asks again when the Worker's receive gave up
/// before the cloud answered (a timeout is not an empty queue).
const EMPTY_CONFIRMATIONS: usize = 6;

/// One drain of `path`: the messages it returned, and whether the Worker's receive gave up
/// before the cloud answered (which says nothing about the queue being empty).
async fn drain_once(client: &Client, url: &str, path: &str) -> (Vec<Value>, bool) {
    let drained = json_request(client, client.post(format!("{url}{path}"))).await;
    (
        drained["messages"].as_array().unwrap().to_vec(),
        drained["timedOut"] == true,
    )
}

/// Everything `path` returns until a drain ends on a real empty answer. A give-up is asked again
/// (bounded); never counting it as empty keeps "nothing arrived" checks from passing on a slow
/// cloud.
async fn drain_confirmed(client: &Client, url: &str, path: &str) -> Vec<Value> {
    let mut all = Vec::new();
    for _ in 0..EMPTY_CONFIRMATIONS {
        let (messages, timed_out) = drain_once(client, url, path).await;
        all.extend(messages);
        if !timed_out {
            return all;
        }
        sleep(Duration::from_secs(5)).await;
    }
    panic!("{path} never answered before the Worker gave up: emptiness is not confirmed");
}

/// Waits until the Worker answers, so the first real request does not pay for a cold start.
async fn warm_up(client: &Client, url: &str) {
    let mut last = String::from("no answer");
    for _ in 0..30 {
        match client.get(format!("{url}/health")).send().await {
            Ok(response) if response.status().is_success() => return,
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                last = format!(
                    "HTTP {status}: {}",
                    body.chars().take(300).collect::<String>()
                );
            }
            Err(error) => last = format!("request error: {error}"),
        }
        sleep(Duration::from_secs(5)).await;
    }
    panic!("the Worker never answered /health at {url}; last answer: {last}");
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
            let drained = drain_confirmed(&client, url, "/failed/drain").await;
            assert!(
                drained.is_empty(),
                "nothing may reach tasks-failed without a dead-letter setting: {drained:?}"
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
    // The highest attempt number Pub/Sub put on a delivery the Worker received.
    let mut highest: BTreeMap<String, u64> = BTreeMap::new();
    let mut good_seen = BTreeSet::new();
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(45 * 60),
            "dead-lettering did not finish: failed={failed:?} good_seen={}",
            good_seen.len()
        );
        // The pull path: whatever has arrived in `tasks-failed` so far.
        let (drained, _) = drain_once(&client, url, "/failed/drain").await;
        for entry in drained {
            let id = entry["task"]["id"].as_str().unwrap().to_string();
            failed.insert(id, entry);
        }
        let deliveries = json_request(&client, client.get(format!("{url}/deliveries"))).await;
        counts.clear();
        highest.clear();
        good_seen.clear();
        for record in deliveries.as_array().unwrap() {
            let id = record["id"].as_str().unwrap();
            if !id.starts_with(prefix) {
                continue;
            }
            *counts.entry(id.to_string()).or_default() += 1;
            if let Some(attempt) = record["attempt"].as_u64() {
                let entry = highest.entry(id.to_string()).or_default();
                *entry = (*entry).max(attempt);
            }
            if !record["poison"].as_bool().unwrap() {
                good_seen.insert(id.to_string());
            }
        }
        if failed.len() == POISON && good_seen.len() == GOOD {
            break;
        }
        sleep(Duration::from_secs(15)).await;
    }

    info!(?counts, "dead-letter delivery counts");
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
        if deployment.platform == "gcp" {
            // The Worker's own count is the wrong instrument on GCP: Pub/Sub counts every push
            // attempt, including one that failed before it reached the Worker (a cold start
            // answering 5xx). The push body carries Pub/Sub's own attempt number when the
            // subscription has a dead-letter policy. Pub/Sub moves the message after
            // `maxDeliveryAttempts` attempts, so the highest number seen is about `MAX_ATTEMPTS`:
            // one less when the last attempt never reached the Worker, up to two more because the
            // limit is best effort.
            // A Worker image from before the runtime passed the number on reports none (the
            // attempt then reads as 1 on every delivery). The bound is then on the Worker's own
            // count and has an upper limit only (Pub/Sub's limit is best effort: at most
            // `MAX_ATTEMPTS + 2`; the Worker can also miss attempts that failed before it).
            // The subscription's own policy is checked against the cloud by
            // `check_gcp_subscription_policy`.
            let worker_deliveries = counts.get(&id).copied().unwrap_or_else(|| {
                panic!("{id} was dead-lettered but the Worker recorded no delivery for it")
            });
            let seen = highest.get(&id).copied().unwrap_or(1);
            if highest.values().any(|attempt| *attempt > 1) {
                assert!(
                    (MAX_ATTEMPTS - 1..=MAX_ATTEMPTS + 2).contains(&seen),
                    "{id}: highest attempt Pub/Sub reported was {seen}; {MAX_ATTEMPTS} expected \
                     (Worker saw {worker_deliveries} deliveries)"
                );
            } else {
                info!(%id, worker_deliveries, "Worker image does not report Pub/Sub's attempt number");
                assert!(
                    worker_deliveries as u64 <= MAX_ATTEMPTS + 2,
                    "{id}: the Worker saw {worker_deliveries} deliveries; at most {} expected",
                    MAX_ATTEMPTS + 2
                );
            }
        } else {
            let attempts = counts[&id] as u64;
            assert!(
                (MAX_ATTEMPTS..=MAX_ATTEMPTS + 2).contains(&attempts),
                "{id} was delivered {attempts} times; about {MAX_ATTEMPTS} expected"
            );
        }
    }
    for n in 0..GOOD {
        assert!(counts[&format!("{prefix}-good-{n}")] >= 1);
    }
    if peek_tasks {
        // `tasks` ends empty: nothing is left to receive. (On GCP the queue's default pull
        // subscription also holds a copy of every message for `receive` consumers, so the
        // trigger path is judged by its deliveries instead.)
        sleep(Duration::from_secs(20)).await;
        let mut left = json!(null);
        for _ in 0..EMPTY_CONFIRMATIONS {
            left = json_request(&client, client.post(format!("{url}/tasks/peek"))).await;
            if left["timedOut"] != true {
                break;
            }
            sleep(Duration::from_secs(5)).await;
        }
        assert!(
            left["timedOut"] != true,
            "tasks was never confirmed empty: the receive kept giving up: {left}"
        );
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
    // Azure: `abandon_message` answers 411 on the base revision, so nack cannot be used there;
    // the lock expires instead (same redelivery, a few minutes slower).
    let nack = deployment.platform != "azure";
    await_pull(
        &client,
        url,
        prefix,
        GOOD,
        nack,
        lowest_recorded(&deployment.platform),
    )
    .await
}

/// Consumes `pulled` until `good` good messages were seen and every poison message of `prefix`
/// ended in `pulled-failed`, then checks payloads, attempts and that `pulled` is empty.
async fn await_pull(
    client: &Client,
    url: &str,
    prefix: &str,
    good: usize,
    nack: bool,
    lowest: u64,
) -> BTreeMap<String, u64> {
    let started = Instant::now();
    let mut failed: BTreeMap<String, Value> = BTreeMap::new();
    let mut attempts: BTreeMap<String, u64> = BTreeMap::new();
    let mut good_seen = BTreeSet::new();
    while !(failed.len() == POISON && good_seen.len() == good) {
        assert!(
            started.elapsed() < Duration::from_secs(30 * 60),
            "the pull path did not finish: failed={failed:?} good_seen={}",
            good_seen.len()
        );
        let seen = json_request(
            &client,
            client.post(format!("{url}/pull/consume?nack={}", u8::from(nack))),
        )
        .await;
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
        let (drained, _) = drain_once(client, url, "/pull/failed/drain").await;
        for entry in drained {
            let id = entry["task"]["id"].as_str().unwrap().to_string();
            failed.insert(id, entry);
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
        // Azure's `receive` reports a constant attempt of 1 (unchanged); the move itself and the
        // queue's delivery count in the cloud view prove the five deliveries there.
        assert!(
            !nack || (lowest..=MAX_ATTEMPTS + 1).contains(&highest),
            "{id}: the last reported attempt was {highest}; about {MAX_ATTEMPTS} expected"
        );
    }
    sleep(Duration::from_secs(10)).await;
    let left = json_request(
        &client,
        client.post(format!("{url}/pull/peek?nack={}", u8::from(nack))),
    )
    .await;
    assert_eq!(left["visible"], 0, "pulled must end empty: {left}");
    info!(
        ?attempts,
        "pull path: highest attempt reported per poison message"
    );
    attempts
}

/// The upgrade path, for a deployment installed from a stack without the setting (the
/// distribution tests set `DLQ_E2E_NO_DEAD_LETTER=1 DLQ_E2E_UPGRADE=1` themselves): poison
/// messages wait in `pulled`, then a release adds the setting (refused until setup is rerun).
/// The messages sent before the rerun must still be there and end in `pulled-failed`. Then the
/// attempts change and the setting is removed.
pub async fn check_upgrade(ctx: &mut alien_test::TestContext) {
    let url = super::bindings::deployment_url(&ctx.deployment)
        .unwrap()
        .to_string();
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();
    warm_up(&client, &url).await;
    let sent = json_request(
        &client,
        client
            .post(format!("{url}/pull/send"))
            .json(&json!({"good": 2, "poison": POISON, "prefix": "up"})),
    )
    .await;
    assert_eq!(sent["sent"], 2 + POISON);

    alien_test::distribution::verify_dead_letter_update(
        ctx,
        alien_test::distribution::DeadLetterChange::Set(5),
    )
    .await
    .expect("adding a dead-letter setting must require and recover through setup");
    // The queues were updated in place: the messages sent before the rerun are still there.
    await_pull(
        &client,
        &url,
        "up",
        2,
        ctx.deployment.platform != "azure",
        lowest_recorded(&ctx.deployment.platform),
    )
    .await;
    check_dead_letter(&ctx.deployment, "after", false).await;
    check_gcp_subscription_policy(ctx, 5).await;

    alien_test::distribution::verify_dead_letter_update(
        ctx,
        alien_test::distribution::DeadLetterChange::Set(7),
    )
    .await
    .expect("changing the attempts must require and recover through setup");
    alien_test::distribution::verify_dead_letter_update(
        ctx,
        alien_test::distribution::DeadLetterChange::Remove,
    )
    .await
    .expect("removing the setting must require and recover through setup");
}

/// The dead-letter check on the trigger path and on the pull path, or the same run without the
/// setting when `DLQ_E2E_NO_DEAD_LETTER=1`. `peek_tasks` is false where the trigger's queue also
/// keeps a pull copy of every message (GCP).
pub async fn check(ctx: &alien_test::TestContext, prefix: &str, peek_tasks: bool) {
    let deployment = &ctx.deployment;
    if std::env::var("DLQ_E2E_NO_DEAD_LETTER").as_deref() == Ok("1") {
        check_without_dead_letter(deployment, prefix).await;
    } else {
        check_dead_letter(deployment, prefix, peek_tasks).await;
        check_pull(deployment, prefix).await;
        check_gcp_subscription_policy(ctx, MAX_ATTEMPTS as i32).await;
    }
}

/// GCP only: the cloud's own view of the subscriptions of the `tasks` topic. Every one of them
/// (the queue's pull subscription and the Worker's push subscription) must carry the dead-letter
/// policy with the declared attempts and the `tasks-failed` topic. This does not depend on the
/// Worker image reporting an attempt number.
async fn check_gcp_subscription_policy(ctx: &alien_test::TestContext, max_attempts: i32) {
    use alien_core::{ClientConfig, StackState};
    use alien_gcp_clients::pubsub::{PubSubApi, PubSubClient};
    if ctx.deployment.platform != "gcp" {
        return;
    }
    let deployment = ctx
        .manager
        .client()
        .get_deployment()
        .id(&ctx.deployment.id)
        .send()
        .await
        .expect("read the deployment");
    let state: StackState = serde_json::from_value(
        serde_json::to_value(deployment.stack_state.as_ref().expect("stack state")).unwrap(),
    )
    .expect("stack state");
    let prefix = &state.resource_prefix;
    let ClientConfig::Gcp(config) = alien_test::setup::build_direct_target_config(
        &alien_test::TestConfig::from_env(),
        ctx.platform,
    )
    .expect("GCP target config") else {
        panic!("GCP config expected");
    };
    let pubsub = PubSubClient::new(Client::new(), *config);
    let source_topic = format!("/topics/{prefix}-tasks");
    let failed_topic = format!("/topics/{prefix}-tasks-failed");
    let mut page = None;
    let mut checked = Vec::new();
    loop {
        let response = pubsub
            .list_subscriptions(Some(1000), page)
            .await
            .expect("list subscriptions");
        for subscription in response.subscriptions {
            if !subscription
                .topic
                .as_deref()
                .is_some_and(|topic| topic.ends_with(&source_topic))
            {
                continue;
            }
            let name = subscription.name.clone().unwrap_or_default();
            let policy = subscription
                .dead_letter_policy
                .unwrap_or_else(|| panic!("{name} has no dead-letter policy"));
            assert_eq!(policy.max_delivery_attempts, Some(max_attempts), "{name}");
            assert!(
                policy
                    .dead_letter_topic
                    .as_deref()
                    .is_some_and(|topic| topic.ends_with(&failed_topic)),
                "{name}: dead-letter topic {:?}",
                policy.dead_letter_topic
            );
            checked.push(name);
        }
        page = response.next_page_token.filter(|value| !value.is_empty());
        if page.is_none() {
            break;
        }
    }
    assert!(
        checked.len() >= 2,
        "expected the pull and the push subscription of tasks to carry the policy: {checked:?}"
    );
    info!(?checked, "GCP subscriptions carry the dead-letter policy");
}
