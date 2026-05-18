/// Utility module for running readiness probes across cloud providers
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::Worker;
use alien_error::AlienError;
use std::{net::SocketAddr, time::Duration};
use tokio::net::lookup_host;
use tracing::{debug, info, warn};

// Readiness probe configuration constants
pub const READINESS_PROBE_MAX_ATTEMPTS: u32 = 10;
pub const READINESS_PROBE_REQUEST_TIMEOUT_SECONDS: u64 = 30;
pub const READINESS_PROBE_MAX_BACKOFF_SECONDS: u64 = 60;

#[derive(Debug, Clone)]
pub struct ReadinessProbeDnsOverride {
    pub host: String,
    pub target_dns_name: String,
    pub port: u16,
}

/// Runs a readiness probe for a worker.
/// Returns Ok(()) if the probe succeeds, or an error if it fails.
/// This worker should be called from within retry logic in the controllers.
pub async fn run_readiness_probe(ctx: &ResourceControllerContext<'_>, url: &str) -> Result<()> {
    run_readiness_probe_with_dns_override(ctx, url, None).await
}

pub async fn run_readiness_probe_with_dns_override(
    ctx: &ResourceControllerContext<'_>,
    url: &str,
    dns_override: Option<ReadinessProbeDnsOverride>,
) -> Result<()> {
    let worker_config = ctx.desired_resource_config::<Worker>()?;
    let probe_config = match &worker_config.readiness_probe {
        Some(probe) => probe,
        None => {
            // No probe configured, just return success
            return Ok(());
        }
    };

    info!(
        name = %worker_config.id,
        "Running readiness probe"
    );

    // Construct the full URL for the probe
    let probe_url = format!("{}{}", url.trim_end_matches('/'), &probe_config.path);
    let client = build_readiness_probe_client(&worker_config, &probe_url, dns_override).await?;

    // Perform the HTTP request
    let method = match probe_config.method {
        alien_core::HttpMethod::Get => reqwest::Method::GET,
        alien_core::HttpMethod::Post => reqwest::Method::POST,
        alien_core::HttpMethod::Put => reqwest::Method::PUT,
        alien_core::HttpMethod::Delete => reqwest::Method::DELETE,
        alien_core::HttpMethod::Head => reqwest::Method::HEAD,
        alien_core::HttpMethod::Options => reqwest::Method::OPTIONS,
        alien_core::HttpMethod::Patch => reqwest::Method::PATCH,
    };

    let request_result = client
        .request(method, &probe_url)
        .timeout(Duration::from_secs(READINESS_PROBE_REQUEST_TIMEOUT_SECONDS))
        .send()
        .await;

    match request_result {
        Ok(response) if response.status().is_success() => {
            info!(
                name = %worker_config.id,
                status = %response.status(),
                "Readiness probe succeeded"
            );
            Ok(())
        }
        Ok(response) => {
            warn!(
                name = %worker_config.id,
                status = %response.status(),
                "Readiness probe failed with HTTP error"
            );

            Err(AlienError::new(ErrorData::ReadinessProbeFailure {
                resource_id: worker_config.id.clone(),
                reason: format!("HTTP status {}", response.status()),
                probe_url: probe_url.clone(),
            }))
        }
        Err(e) => {
            warn!(
                name = %worker_config.id,
                error = %e,
                "Readiness probe failed with network error"
            );

            Err(AlienError::new(ErrorData::ReadinessProbeFailure {
                resource_id: worker_config.id.clone(),
                reason: "Network error".to_string(),
                probe_url: probe_url.clone(),
            }))
        }
    }
}

async fn build_readiness_probe_client(
    worker_config: &Worker,
    probe_url: &str,
    dns_override: Option<ReadinessProbeDnsOverride>,
) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();

    if let Some(override_config) = dns_override {
        let addrs = lookup_readiness_probe_target(
            worker_config,
            probe_url,
            &override_config.target_dns_name,
            override_config.port,
        )
        .await?;

        debug!(
            name = %worker_config.id,
            host = %override_config.host,
            target = %override_config.target_dns_name,
            addrs = ?addrs,
            "Using readiness probe DNS override"
        );

        builder = builder.resolve_to_addrs(&override_config.host, &addrs);
    }

    builder.build().map_err(|err| {
        AlienError::new(ErrorData::ReadinessProbeFailure {
            resource_id: worker_config.id.clone(),
            reason: format!("Failed to build HTTP client: {err}"),
            probe_url: probe_url.to_string(),
        })
    })
}

async fn lookup_readiness_probe_target(
    worker_config: &Worker,
    probe_url: &str,
    target_dns_name: &str,
    port: u16,
) -> Result<Vec<SocketAddr>> {
    let addrs = lookup_host((target_dns_name, port)).await.map_err(|err| {
        AlienError::new(ErrorData::ReadinessProbeFailure {
            resource_id: worker_config.id.clone(),
            reason: format!("Failed to resolve readiness probe target '{target_dns_name}': {err}"),
            probe_url: probe_url.to_string(),
        })
    })?;

    let addrs = addrs.collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(AlienError::new(ErrorData::ReadinessProbeFailure {
            resource_id: worker_config.id.clone(),
            reason: format!("Readiness probe target '{target_dns_name}' resolved no addresses"),
            probe_url: probe_url.to_string(),
        }));
    }

    Ok(addrs)
}

#[cfg(test)]
pub mod test_utils {
    //! Test utilities for readiness probe mocking

    use alien_core::{HttpMethod, Worker};
    use httpmock::{prelude::*, MockServer};

    /// Creates a mock HTTP server for readiness probe testing.
    ///
    /// Returns a MockServer that responds successfully to the worker's readiness probe
    /// configuration. The server's base_url() can be used as the worker URL in tests.
    ///
    /// # Arguments
    /// * `worker` - The worker configuration containing readiness probe settings
    ///
    /// # Returns
    /// * `MockServer` - A running mock server that will respond to the readiness probe
    ///
    /// # Example
    /// ```ignore
    /// let worker = function_with_readiness_probe();
    /// let mock_server = create_readiness_probe_mock(&worker);
    /// // Use mock_server.base_url() as the worker URL
    /// ```
    pub fn create_readiness_probe_mock(worker: &Worker) -> MockServer {
        let server = MockServer::start();

        if let Some(probe_config) = &worker.readiness_probe {
            // Convert alien_core::HttpMethod to httpmock::Method
            let method = match probe_config.method {
                HttpMethod::Get => httpmock::Method::GET,
                HttpMethod::Post => httpmock::Method::POST,
                HttpMethod::Put => httpmock::Method::PUT,
                HttpMethod::Delete => httpmock::Method::DELETE,
                HttpMethod::Head => httpmock::Method::HEAD,
                HttpMethod::Options => httpmock::Method::OPTIONS,
                HttpMethod::Patch => httpmock::Method::PATCH,
            };

            // Create mock endpoint that responds successfully to the readiness probe
            server.mock(|when, then| {
                when.method(method).path(&probe_config.path);
                then.status(200).body("OK");
            });
        }

        server
    }

    /// Creates a mock HTTP server that fails readiness probes.
    ///
    /// This is useful for testing failure scenarios and retry logic.
    ///
    /// # Arguments
    /// * `worker` - The worker configuration containing readiness probe settings
    /// * `status_code` - The HTTP status code to return (e.g., 500, 503)
    ///
    /// # Returns
    /// * `MockServer` - A running mock server that will fail the readiness probe
    pub fn create_failing_readiness_probe_mock(worker: &Worker, status_code: u16) -> MockServer {
        let server = MockServer::start();

        if let Some(probe_config) = &worker.readiness_probe {
            let method = match probe_config.method {
                HttpMethod::Get => httpmock::Method::GET,
                HttpMethod::Post => httpmock::Method::POST,
                HttpMethod::Put => httpmock::Method::PUT,
                HttpMethod::Delete => httpmock::Method::DELETE,
                HttpMethod::Head => httpmock::Method::HEAD,
                HttpMethod::Options => httpmock::Method::OPTIONS,
                HttpMethod::Patch => httpmock::Method::PATCH,
            };

            // Create mock endpoint that fails the readiness probe
            server.mock(|when, then| {
                when.method(method).path(&probe_config.path);
                then.status(status_code).body("Service Unavailable");
            });
        }

        server
    }

    /// Creates a mock HTTP server with custom response configuration.
    ///
    /// # Arguments
    /// * `worker` - The worker configuration containing readiness probe settings
    /// * `status_code` - The HTTP status code to return
    /// * `body` - The response body to return
    ///
    /// # Returns
    /// * `MockServer` - A running mock server with custom response
    pub fn create_custom_readiness_probe_mock(
        worker: &Worker,
        status_code: u16,
        body: &str,
    ) -> MockServer {
        let server = MockServer::start();

        if let Some(probe_config) = &worker.readiness_probe {
            let method = match probe_config.method {
                HttpMethod::Get => httpmock::Method::GET,
                HttpMethod::Post => httpmock::Method::POST,
                HttpMethod::Put => httpmock::Method::PUT,
                HttpMethod::Delete => httpmock::Method::DELETE,
                HttpMethod::Head => httpmock::Method::HEAD,
                HttpMethod::Options => httpmock::Method::OPTIONS,
                HttpMethod::Patch => httpmock::Method::PATCH,
            };

            server.mock(|when, then| {
                when.method(method).path(&probe_config.path);
                then.status(status_code).body(body);
            });
        }

        server
    }
}
