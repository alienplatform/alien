//! Event-delivery checks (queue / storage / cron triggers).
//!
//! These checks prove the user's event handler actually RAN, not that an
//! endpoint has the right shape. Each trigger path ends in the app's handler
//! recording the event in KV (`storage_event:*` / `cron_event:*` /
//! `queue_message:*`); the checks trigger the event and then poll the app's
//! read-back endpoints until the specific record appears.

use alien_test::TestDeployment;
use anyhow::{bail, Context};
use tracing::info;

use super::bindings::{deployment_url, STORAGE_BINDING};

/// Queue binding dedicated to trigger delivery. `alien-queue` is consumed by
/// the app's own send/receive/ack endpoint, so it can't be used to prove
/// trigger delivery without racing that consumer.
const EVENTS_QUEUE_BINDING: &str = "alien-events-queue";

/// How long to wait for an event to travel trigger → runtime → handler → KV.
const EVENT_DELIVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
/// Cron fires at most once per minute, so its window needs one full period
/// plus delivery slack.
const CRON_DELIVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);
const EVENT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);

/// Fetch and parse GET /events/list.
async fn fetch_events_list(url: &str) -> anyhow::Result<serde_json::Value> {
    let resp = reqwest::Client::new()
        .get(format!("{}/events/list", url))
        .send()
        .await
        .context("Events list request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Events list returned {}: {}", status, body);
    }
    resp.json()
        .await
        .context("Failed to parse events list response")
}

/// Check queue trigger delivery: enqueue a send-only message with a unique
/// marker, then verify the app's `on_queue_message` handler processed exactly
/// that message (it records the payload in KV, read back via /events/list).
///
/// Fails if the platform's queue trigger stops delivering messages to the
/// registered handler.
pub async fn check_queue_event_delivery(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking queue trigger delivery");

    let marker = format!("queue-event-{}", uuid::Uuid::new_v4());
    let resp = reqwest::Client::new()
        .post(format!("{}/queue-send/{}", url, EVENTS_QUEUE_BINDING))
        .json(&serde_json::json!({ "marker": marker }))
        .send()
        .await
        .context("Queue send-only request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Queue send-only returned {}: {}", status, body);
    }

    let delivered = super::poll_until(EVENT_DELIVERY_TIMEOUT, EVENT_POLL_INTERVAL, || async {
        let data = fetch_events_list(url).await?;
        let messages = data
            .get("queueMessages")
            .and_then(|v| v.as_array())
            .context("Events list response missing queueMessages array")?;

        let Some(record) = messages.iter().find(|record| {
            record
                .get("payload")
                .and_then(|p| p.as_str())
                .map(|p| p.contains(&marker))
                .unwrap_or(false)
        }) else {
            return Ok(None);
        };

        let message_id =
            super::require_nonempty_str(record, "messageId", "Queue message record")?.to_string();
        super::require_nonempty_str(record, "processedAt", "Queue message record")?;
        Ok(Some(message_id))
    })
    .await?;

    let Some(message_id) = delivered else {
        bail!(
            "Queue trigger did not deliver message with marker '{}' to the handler within {:?}. Recorded messages: {:?}",
            marker,
            EVENT_DELIVERY_TIMEOUT,
            fetch_events_list(url).await.ok()
        );
    };
    info!(%message_id, "Queue trigger delivery check passed");
    Ok(())
}

/// Check storage trigger delivery: write an object with a unique key (write
/// only — no delete), then verify the app's `on_storage_event` handler
/// processed the `created` event for exactly that key.
///
/// Fails if the platform's storage trigger stops delivering events to the
/// registered handler.
pub async fn check_storage_event_delivery(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking storage trigger delivery");

    let key = format!("storage-event-test-{}.txt", uuid::Uuid::new_v4());
    let resp = reqwest::Client::new()
        .post(format!("{}/storage-write/{}", url, STORAGE_BINDING))
        .json(&serde_json::json!({
            "key": key,
            "content": "storage trigger delivery test",
        }))
        .send()
        .await
        .context("Storage write-only request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Storage write-only returned {}: {}", status, body);
    }

    let delivered = super::poll_until(EVENT_DELIVERY_TIMEOUT, EVENT_POLL_INTERVAL, || async {
        let resp = reqwest::Client::new()
            .get(format!("{}/events/storage/{}", url, key))
            .send()
            .await
            .context("Storage event lookup request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("Storage event lookup returned {}: {}", status, body);
        }
        let data: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse storage event lookup response")?;

        if data.get("found").and_then(|v| v.as_bool()) != Some(true) {
            return Ok(None);
        }
        let event = data
            .get("event")
            .context("Storage event record missing event body")?;
        // Key equality is the handler-ran proof: only the handler writes
        // this KV record, keyed by the object it was invoked for.
        let event_key = event.get("key").and_then(|v| v.as_str()).unwrap_or("");
        if event_key != key {
            bail!(
                "Storage event key mismatch: expected {}, got {} ({:?})",
                key,
                event_key,
                event
            );
        }
        // Event type naming varies per platform (local: "created"; clouds
        // use their native names), so assert presence rather than an exact
        // value.
        super::require_nonempty_str(event, "eventType", "Storage event record")?;
        Ok(Some(()))
    })
    .await?;

    if delivered.is_none() {
        bail!(
            "Storage trigger did not deliver 'created' event for key '{}' to the handler within {:?}",
            key,
            EVENT_DELIVERY_TIMEOUT
        );
    }
    info!(%key, "Storage trigger delivery check passed");
    Ok(())
}

/// Check cron trigger delivery: the deployment declares a `* * * * *`
/// schedule, so within one period the app's `on_cron_event` handler must run
/// and record the event in KV. The deployment is created fresh per test, so
/// any recorded cron event proves this deployment's handler executed.
///
/// Fails if the platform's schedule trigger stops firing the handler.
pub async fn check_cron_event_delivery(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking cron trigger delivery (may wait up to one schedule period)");

    let delivered = super::poll_until(CRON_DELIVERY_TIMEOUT, EVENT_POLL_INTERVAL, || async {
        let data = fetch_events_list(url).await?;
        let cron_events = data
            .get("cronEvents")
            .and_then(|v| v.as_array())
            .context("Events list response missing cronEvents array")?;

        let Some(event) = cron_events.first() else {
            return Ok(None);
        };
        // Schedule naming varies per platform (local: the cron expression;
        // clouds use their native rule identifiers), so assert presence.
        super::require_nonempty_str(event, "scheduleName", "Cron event record")?;
        super::require_nonempty_str(event, "processedAt", "Cron event record")?;
        Ok(Some(()))
    })
    .await?;

    if delivered.is_none() {
        bail!(
            "Cron trigger did not fire the handler within {:?} despite a '* * * * *' schedule",
            CRON_DELIVERY_TIMEOUT
        );
    }
    info!("Cron trigger delivery check passed");
    Ok(())
}
