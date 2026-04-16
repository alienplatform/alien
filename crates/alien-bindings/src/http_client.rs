/// Shared HTTP client configuration and utilities to prevent file descriptor exhaustion.
///
/// This module provides a centralized way to create reqwest clients with proper
/// connection pooling, timeouts, and resource limits suitable for constrained
/// environments like AWS Lambda (1024 FD limit).
use reqwest::Client;
use std::time::Duration;

/// Creates a properly configured reqwest Client with connection pooling and timeouts.
///
/// This client is designed to prevent file descriptor exhaustion by:
/// - Limiting idle connection pool size per host
/// - Setting idle connection timeouts to clean up unused connections
/// - Adding request timeouts to prevent indefinite hangs
///
/// Use this for ALL reqwest client creation to ensure consistent resource management.
pub fn create_http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(4) // Critical: Limit idle connections to prevent FD exhaustion
        .pool_idle_timeout(Some(Duration::from_secs(90))) // Close idle connections after 90s
        .build()
        .expect("Failed to build HTTP client with connection pooling")
}

/// Creates a reqwest Client with custom timeout settings.
/// Still includes connection pooling limits to prevent FD exhaustion.
pub fn create_http_client_with_timeout(timeout: Duration) -> Client {
    Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .build()
        .expect("Failed to build HTTP client with custom timeout")
}
