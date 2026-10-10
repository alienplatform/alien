//! `alien logs` against a manager you run: reads the recent logs the manager
//! keeps for each deployment (`GET /v1/deployments/{id}/logs`). Long-term
//! search lives in the OpenTelemetry backend the manager forwards to.

use std::{
    collections::{hash_map::DefaultHasher, HashSet},
    hash::{Hash, Hasher},
    time::Duration as StdDuration,
};

use alien_error::{AlienError, Context};
use alien_manager_api::SdkResultExt;
use chrono::{DateTime, Utc};
use console::style;

use super::logs::{LogLevel, LogsArgs};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;

pub async fn manager_logs_task(args: LogsArgs, ctx: ExecutionMode) -> Result<()> {
    let Some(reference) = args.deployment.clone() else {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment".to_string(),
            message: "Pass --deployment <name-or-id>. Run `alien deployments ls` to list them."
                .to_string(),
        }));
    };
    if args.source.is_some() || args.system {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "source".to_string(),
            message: "--source and --system read hosted log stores this manager doesn't have; \
                      query your OpenTelemetry backend instead."
                .to_string(),
        }));
    }

    let (project_id, _) = ctx.resolve_project(None, false).await?;
    let mgr = ctx.resolve_manager(&project_id, "local").await?;
    let deployment =
        crate::deployment_resolver::resolve(&mgr.client, &reference, ctx.is_dev()).await?;

    let mut since: DateTime<Utc> = args.from.unwrap_or_else(|| Utc::now() - args.since);
    // `since` is inclusive: entries at the last timestamp seen come back on
    // the next poll, and these fingerprints skip the ones already printed.
    let mut printed_at_since: HashSet<u64> = HashSet::new();
    loop {
        let response = mgr
            .client
            .get_deployment_logs()
            .id(&deployment.id)
            .limit(args.limit as i64)
            .since(since)
            .send()
            .await
            .into_sdk_error()
            .await
            .context(ErrorData::ApiRequestFailed {
                message: format!("Failed to read logs for '{reference}'"),
                url: None,
            })?
            .into_inner();

        for entry in response.items {
            let fingerprint = fingerprint(&entry);
            if entry.timestamp > since {
                since = entry.timestamp;
                printed_at_since.clear();
            }
            if entry.timestamp == since && !printed_at_since.insert(fingerprint) {
                continue;
            }
            if !level_selected(&args.level, &entry.severity)
                || !query_matches(&args.query, &entry.message)
            {
                continue;
            }
            if let Some(to) = args.to {
                if entry.timestamp > to {
                    continue;
                }
            }
            if args.json {
                print_json(&entry)?;
            } else {
                println!(
                    "{} {:<5} {} {}",
                    style(entry.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ")).dim(),
                    severity_style(&entry.severity),
                    style(entry.resource.as_deref().unwrap_or("-")).cyan(),
                    entry.message
                );
            }
        }

        if !args.follow {
            return Ok(());
        }
        tokio::time::sleep(StdDuration::from(args.interval)).await;
    }
}

fn fingerprint(entry: &alien_manager_api::types::RecentLogEntry) -> u64 {
    let mut hasher = DefaultHasher::new();
    entry.timestamp.hash(&mut hasher);
    entry.resource.hash(&mut hasher);
    entry.message.hash(&mut hasher);
    let mut attributes: Vec<_> = entry.attributes.iter().collect();
    attributes.sort();
    attributes.hash(&mut hasher);
    hasher.finish()
}

fn level_selected(levels: &[LogLevel], severity: &str) -> bool {
    if levels.is_empty() {
        return true;
    }
    let severity = severity.to_ascii_uppercase();
    levels.iter().any(|level| {
        let name = match level {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
            LogLevel::Fatal => "FATAL",
        };
        severity.starts_with(name)
    })
}

fn query_matches(query: &str, message: &str) -> bool {
    query == "*" || message.to_lowercase().contains(&query.to_lowercase())
}

fn severity_style(severity: &str) -> console::StyledObject<String> {
    let text = severity.to_ascii_uppercase();
    match text.as_str() {
        "ERROR" | "FATAL" => style(text).red(),
        "WARN" | "WARNING" => style(text).yellow(),
        _ => style(text).dim(),
    }
}
