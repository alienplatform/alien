use alien_bindings::AlienContext;
use serde::Deserialize;
use serde_json::json;

use crate::db::EncryptedDb;
use crate::error::{Error, Result};
use crate::monitor;

// Convert our Error to the format alien-bindings expects
fn to_bindings_error<T>(result: Result<T>) -> alien_bindings::Result<T> {
    result.map_err(|e| {
        // Use EventProcessingFailed as a generic handler error wrapper
        alien_error::AlienError::new(alien_bindings::ErrorData::EventProcessingFailed {
            event_type: "command".to_string(),
            reason: e.to_string(),
        })
    })
}

/// Register all ARC command handlers
pub fn register(ctx: &AlienContext, db: EncryptedDb) {
    // get-events command
    {
        let db = db.clone();
        ctx.on_command("get-events", move |params: GetEventsParams| {
            let db = db.clone();
            async move { to_bindings_error(handle_get_events(db, params).await) }
        });
    }

    // get-config command
    {
        ctx.on_command("get-config", move |_params: serde_json::Value| async move {
            to_bindings_error(handle_get_config().await)
        });
    }

    // scan-path command
    {
        ctx.on_command("scan-path", move |params: ScanPathParams| async move {
            to_bindings_error(handle_scan_path(params).await)
        });
    }

    // simulate-clipboard command (for testing)
    {
        let db = db.clone();
        ctx.on_command(
            "simulate-clipboard",
            move |params: SimulateClipboardParams| {
                let db = db.clone();
                async move { to_bindings_error(handle_simulate_clipboard(db, params).await) }
            },
        );
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetEventsParams {
    since: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    100
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanPathParams {
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimulateClipboardParams {
    content: String,
}

async fn handle_get_events(db: EncryptedDb, params: GetEventsParams) -> Result<serde_json::Value> {
    let since = parse_duration(&params.since)?;
    let events = db.get_events_since(since, params.limit).await?;

    Ok(json!({ "events": events }))
}

async fn handle_get_config() -> Result<serde_json::Value> {
    let monitored_paths = if let Ok(paths) = std::env::var("MONITORED_PATHS") {
        paths.split(':').map(|s| s.to_string()).collect::<Vec<_>>()
    } else {
        vec![std::env::temp_dir().to_string_lossy().to_string()]
    };

    Ok(json!({
        "monitoredPaths": monitored_paths,
        "clipboardMonitoring": true,
        "eventRetentionDays": 30
    }))
}

async fn handle_scan_path(params: ScanPathParams) -> Result<serde_json::Value> {
    let result = monitor::scan_directory(&params.path).await?;

    Ok(json!({
        "filesScanned": result.files_scanned,
        "sensitiveFiles": result.sensitive_files
    }))
}

async fn handle_simulate_clipboard(
    db: EncryptedDb,
    params: SimulateClipboardParams,
) -> Result<serde_json::Value> {
    monitor::simulate_clipboard_write(&db, &params.content).await?;

    Ok(json!({ "success": true }))
}

/// Parse duration string (e.g., "5m", "1h", "2d")
fn parse_duration(input: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    let input = input.trim();

    if input.is_empty() {
        return Err(Error::InvalidDuration(
            "Duration cannot be empty".to_string(),
        ));
    }

    // Extract number and unit
    let len = input.len();
    let unit = &input[len - 1..];
    let value = &input[..len - 1];

    let value: i64 = value
        .parse()
        .map_err(|_| Error::InvalidDuration(format!("Invalid number in duration: {}", input)))?;

    let duration = match unit {
        "s" => chrono::Duration::seconds(value),
        "m" => chrono::Duration::minutes(value),
        "h" => chrono::Duration::hours(value),
        "d" => chrono::Duration::days(value),
        _ => {
            return Err(Error::InvalidDuration(format!(
                "Invalid duration unit '{}' (use s, m, h, or d)",
                unit
            )))
        }
    };

    Ok(chrono::Utc::now() - duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        let result = parse_duration("5m").unwrap();
        let expected = chrono::Utc::now() - chrono::Duration::minutes(5);

        // Check within 1 second tolerance
        let diff = (result.timestamp() - expected.timestamp()).abs();
        assert!(diff <= 1);
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("invalid").is_err());
        assert!(parse_duration("5x").is_err());
        assert!(parse_duration("").is_err());
    }
}
