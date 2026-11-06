//! Read-only collector smoke probe. Supply protected ClientConfig JSON and MetricsSource JSON
//! files, followed by an RFC3339 start/end window. Prints counts, never credentials or labels.
use alien_core::{ClientConfig, MetricsSource};
use alien_observer::metrics::{self, CollectionWindow};
use opentelemetry_proto::tonic::metrics::v1::metric::Data;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        // Print only classifications, never HTTP bodies, labels, URLs, or credential values.
        let classification = if let Some(error) = error.downcast_ref::<serde_json::Error>() {
            format!(
                "input JSON {:?} at line {} column {}",
                error.classify(),
                error.line(),
                error.column()
            )
        } else if let Some(error) = error.downcast_ref::<std::io::Error>() {
            format!("input IO {:?}", error.kind())
        } else if let Some(error) =
            error.downcast_ref::<alien_error::AlienError<alien_client_core::ErrorData>>()
        {
            format!(
                "provider code={} HTTP={:?}",
                error.code, error.http_status_code
            )
        } else if error.to_string() == "Provider returned no measurements" {
            "provider returned no measurements for this window".into()
        } else {
            "request or input failure".into()
        };
        eprintln!(
            "Collection failed ({classification}); no successful measurement proof was produced"
        );
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err(
            "Usage: collect_metrics <credentials.json> <source.json> <start RFC3339> <end RFC3339>"
                .into(),
        );
    }
    let config: ClientConfig = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let source: MetricsSource = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let window = CollectionWindow {
        start: chrono::DateTime::parse_from_rfc3339(&args[3])?.to_utc(),
        end: chrono::DateTime::parse_from_rfc3339(&args[4])?.to_utc(),
    };
    let mut http = reqwest::Client::builder();
    // Keep TLS verification enabled when an operator explicitly supplies a CA
    // bundle, including test environments with an authenticated egress proxy.
    if let Some(path) = std::env::var_os("SSL_CERT_FILE") {
        let pem = std::fs::read(path)?;
        let certificates = reqwest::Certificate::from_pem_bundle(&pem)?;
        if certificates.is_empty() {
            return Err("Configured CA bundle is empty".into());
        }
        for certificate in certificates {
            http = http.add_root_certificate(certificate);
        }
    }
    let http = http.build()?;
    let batch = match source {
        MetricsSource::Aws(sources) => {
            let config = config
                .aws_config()
                .ok_or("AWS source requires AWS credentials")?;
            let credentials =
                alien_aws_clients::AwsCredentialProvider::from_config(config.clone()).await?;
            metrics::aws::collect(
                &alien_aws_clients::CloudWatchClient::new(http, credentials),
                &sources,
                &window,
            )
            .await?
        }
        MetricsSource::Gcp(source) => {
            let config = config
                .gcp_config()
                .ok_or("GCP source requires GCP credentials")?;
            if source.project_id != config.project_id {
                return Err("Source project differs from credentials".into());
            }
            metrics::gcp::collect(
                &alien_gcp_clients::MonitoringClient::new(http, config.clone()),
                &source,
                &window,
            )
            .await?
        }
        MetricsSource::Azure(source) => {
            let config = config
                .azure_config()
                .ok_or("Azure source requires Azure credentials")?;
            metrics::azure::collect(
                &alien_azure_clients::AzureMonitorClient::new(
                    http,
                    alien_azure_clients::AzureTokenCache::new(config.clone()),
                ),
                &source,
                &window,
            )
            .await?
        }
        MetricsSource::Scrape(source) => {
            metrics::scrape::collect(
                &source,
                u64::try_from(
                    chrono::Utc::now()
                        .timestamp_nanos_opt()
                        .ok_or("Timestamp overflow")?,
                )?,
            )
            .await?
        }
    };
    let points: usize = batch
        .metrics
        .iter()
        .map(|metric| match &metric.data {
            Some(Data::Gauge(gauge)) => gauge.data_points.len(),
            Some(Data::Sum(sum)) => sum.data_points.len(),
            Some(Data::Histogram(histogram)) => histogram.data_points.len(),
            Some(Data::Summary(summary)) => summary.data_points.len(),
            _ => 0,
        })
        .sum();
    if points == 0 {
        return Err("Provider returned no measurements".into());
    }
    println!(
        "Read-only collection succeeded: {} metric streams, {points} measurement points",
        batch.metrics.len()
    );
    Ok(())
}
