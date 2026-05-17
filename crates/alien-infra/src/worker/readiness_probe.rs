/// Utility module for running readiness probes across cloud providers
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::Worker;
use alien_error::AlienError;
use std::time::Duration;
use tracing::{info, warn};

// Readiness probe configuration constants
pub const READINESS_PROBE_MAX_ATTEMPTS: u32 = 10;
pub const READINESS_PROBE_REQUEST_TIMEOUT_SECONDS: u64 = 30;
pub const READINESS_PROBE_MAX_BACKOFF_SECONDS: u64 = 60;

/// Runs a readiness probe for a worker.
/// Returns Ok(()) if the probe succeeds, or an error if it fails.
/// This worker should be called from within retry logic in the controllers.
pub async fn run_readiness_probe(ctx: &ResourceControllerContext<'_>, url: &str) -> Result<()> {
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

    // Perform the HTTP request
    let client = reqwest::Client::new();
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

#[cfg(test)]
pub mod test_utils {
    //! Test utilities for readiness probe mocking

    use alien_core::{Worker, HttpMethod};
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
    pub fn create_failing_readiness_probe_mock(
        worker: &Worker,
        status_code: u16,
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
