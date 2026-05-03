use crate::traits::telemetry_backend::{TelemetryBackend, TelemetryCaller, TelemetrySignal};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::HashMap;

pub struct OtlpForwardingBackend {
    client: reqwest::Client,
    endpoint: String,
    /// Extra headers sent with every OTLP request (e.g. auth tokens).
    /// `content-type` is always set by the backend and cannot be overridden.
    extra_headers: HeaderMap,
}

impl OtlpForwardingBackend {
    /// Create a new OTLP forwarding backend.
    ///
    /// `headers` are validated at construction time: invalid header names or
    /// values cause a panic so misconfiguration surfaces immediately on startup.
    /// The `content-type` header is reserved and will be ignored if present in
    /// the provided map.
    pub fn new(endpoint: String, headers: HashMap<String, String>) -> Self {
        let mut header_map = HeaderMap::with_capacity(headers.len());
        for (name, value) in &headers {
            // Skip content-type — we always set it ourselves.
            if name.eq_ignore_ascii_case("content-type") {
                continue;
            }
            let header_name: HeaderName = name
                .parse()
                .unwrap_or_else(|e| panic!("Invalid OTLP header name '{}': {}", name, e));
            let header_value: HeaderValue = value
                .parse()
                .unwrap_or_else(|e| panic!("Invalid OTLP header value for '{}': {}", name, e));
            header_map.insert(header_name, header_value);
        }

        Self {
            client: reqwest::Client::new(),
            endpoint,
            extra_headers: header_map,
        }
    }
}

#[async_trait]
impl TelemetryBackend for OtlpForwardingBackend {
    async fn ingest(
        &self,
        signal: TelemetrySignal,
        _caller: &TelemetryCaller,
        data: bytes::Bytes,
    ) -> Result<(), AlienError> {
        let path = match signal {
            TelemetrySignal::Logs => "/v1/logs",
            TelemetrySignal::Traces => "/v1/traces",
            TelemetrySignal::Metrics => "/v1/metrics",
        };

        let url = format!("{}{}", self.endpoint.trim_end_matches('/'), path);

        self.client
            .post(&url)
            .headers(self.extra_headers.clone())
            .header("content-type", "application/x-protobuf")
            .body(data)
            .send()
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to forward telemetry signal to OTLP backend".to_string(),
            })?;

        Ok(())
    }
}
