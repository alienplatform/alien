use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, ValueEnum};
use serde_json::Value;
use url::Url;

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::dim_label;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Show gateway usage",
    after_help = "EXAMPLES:
    alien usage ai --range 24h
    alien usage ai --range 7d --json
    alien usage encryption --range 30d"
)]
pub struct UsageArgs {
    #[arg(value_enum)]
    pub service: UsageService,
    #[arg(long, value_enum, default_value_t = UsageRange::Hours24)]
    pub range: UsageRange,
    /// Project to inspect (defaults to the linked project)
    #[arg(long)]
    pub project: Option<String>,
    /// Emit the complete privacy-safe aggregate response
    #[arg(long)]
    pub json: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum UsageService {
    #[value(alias = "ai-gateway", alias = "models")]
    Ai,
    #[value(alias = "encryption-gateway", alias = "keys")]
    Encryption,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum UsageRange {
    #[value(name = "24h")]
    Hours24,
    #[value(name = "7d")]
    Days7,
    #[value(name = "30d")]
    Days30,
}

impl UsageArgs {
    pub fn wants_json_output(&self) -> bool {
        self.json
    }
}

pub async fn usage_task(args: UsageArgs, ctx: ExecutionMode) -> Result<()> {
    let auth = ctx.auth_http().await?;
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    let (project, _) = ctx
        .resolve_project(args.project.as_deref(), !args.json)
        .await?;
    let metric = match args.service {
        UsageService::Ai => "ai-metrics",
        UsageService::Encryption => "encryption-metrics",
    };
    let mut url =
        Url::parse(&auth.base_url)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Platform base URL is invalid".to_string(),
            })?;
    url.set_path(&format!(
        "{}/v1/projects/{}/{}",
        url.path().trim_end_matches('/'),
        urlencoding::encode(&project),
        metric
    ));
    url.query_pairs_mut()
        .append_pair("workspace", &workspace)
        .append_pair("range", args.range.as_str());
    let response = auth
        .client
        .get(url.clone())
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to load gateway usage".to_string(),
            url: Some(url.to_string()),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("Gateway usage request returned HTTP {status}: {body}"),
            url: Some(url.to_string()),
        }));
    }
    let usage: Value =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Gateway usage response was not valid JSON".to_string(),
                url: Some(url.to_string()),
            })?;

    if args.json {
        return print_json(&usage);
    }
    if usage.get("status").and_then(Value::as_str) != Some("available") {
        println!(
            "{} {}",
            dim_label("Usage unavailable:"),
            usage
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown reason")
        );
        return Ok(());
    }
    let totals = usage.get("totals").ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "Gateway usage response is missing totals".to_string(),
        })
    })?;
    println!("{} {}", dim_label("Range"), args.range.as_str());
    println!(
        "{} {}",
        dim_label("Requests"),
        totals.get("requests").and_then(Value::as_u64).unwrap_or(0)
    );
    println!(
        "{} {}",
        dim_label("Successful"),
        totals
            .get("successfulRequests")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    println!(
        "{} {}",
        dim_label("Errors"),
        totals
            .get("errorRequests")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    if matches!(args.service, UsageService::Ai) {
        println!(
            "{} {} / {}",
            dim_label("Tokens in / out"),
            totals
                .get("inputTokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            totals
                .get("outputTokens")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        );
        let cost = totals
            .get("estimatedCostMicrousd")
            .and_then(Value::as_u64)
            .map(|value| value as f64 / 1_000_000.0)
            .unwrap_or(0.0);
        println!("{} ${cost:.6}", dim_label("Estimated provider cost"));
    }
    println!(
        "{} {}",
        dim_label("Average latency"),
        format_latency(totals.get("averageLatencyMs")),
    );
    println!(
        "{} {}",
        dim_label("P95 latency"),
        format_latency(totals.get("p95LatencyMs")),
    );
    Ok(())
}

impl UsageRange {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hours24 => "24h",
            Self::Days7 => "7d",
            Self::Days30 => "30d",
        }
    }
}

fn format_latency(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_f64)
        .map(|value| format!("{value:.2} ms"))
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_values_match_the_metrics_api() {
        let args = UsageArgs::try_parse_from(["usage", "ai-gateway", "--range", "30d", "--json"])
            .expect("gateway aliases and range should parse");
        assert!(matches!(args.service, UsageService::Ai));
        assert!(matches!(args.range, UsageRange::Days30));
        assert!(args.json);
    }
}
