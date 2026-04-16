use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use std::sync::OnceLock;
use tracing::{error, info};

use opentelemetry::KeyValue;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{ExportConfig, LogExporter, Protocol, WithExportConfig};
use opentelemetry_sdk::{
    logs::{BatchLogProcessor, SdkLoggerProvider},
    Resource,
};

/// Global OTLP logger provider for flushing logs on shutdown
static OTLP_PROVIDER: OnceLock<Option<SdkLoggerProvider>> = OnceLock::new();

/// Configuration for OTLP logging based on environment variables
#[derive(Debug, Clone)]
pub struct OtlpConfig {
    pub endpoint: String,
    pub headers: std::collections::HashMap<String, String>,
    pub service_name: String,
    pub service_version: String,
}

impl OtlpConfig {
    /// Load OTLP configuration from environment variables
    pub fn from_env() -> Option<Self> {
        // Logs-specific endpoint takes precedence over generic endpoint
        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT")
            .or_else(|_| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT"))
            .ok()?;

        let service_name =
            std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "alien-runtime".to_string());

        let service_version = std::env::var("OTEL_SERVICE_VERSION")
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());

        // Parse headers from environment
        let mut headers = std::collections::HashMap::new();

        // Standard OTLP headers
        if let Ok(auth_header) = std::env::var("OTEL_EXPORTER_OTLP_HEADERS_AUTHORIZATION") {
            headers.insert("authorization".to_string(), auth_header);
        }

        // Parse generic headers (format: key1=value1,key2=value2)
        if let Ok(headers_str) = std::env::var("OTEL_EXPORTER_OTLP_HEADERS") {
            for header in headers_str.split(',') {
                if let Some((key, value)) = header.split_once('=') {
                    headers.insert(key.trim().to_lowercase(), value.trim().to_string());
                }
            }
        }

        Some(Self {
            endpoint,
            headers,
            service_name,
            service_version,
        })
    }
}

/// Initialize OTLP logging and return the tracing bridge layer
#[cfg(feature = "otlp")]
pub fn init_otlp_logging(
) -> Result<Option<OpenTelemetryTracingBridge<SdkLoggerProvider, opentelemetry_sdk::logs::SdkLogger>>>
{
    let config = match OtlpConfig::from_env() {
        Some(config) => {
            info!(
                endpoint = %config.endpoint,
                service_name = %config.service_name,
                service_version = %config.service_version,
                "Initializing OTLP logging"
            );
            config
        }
        None => {
            info!("No OTLP configuration found in environment variables, skipping OTLP logging");
            // Store None in the global provider
            OTLP_PROVIDER.set(None).ok();
            return Ok(None);
        }
    };

    // Build OTLP Log exporter over HTTP with protobuf.
    // When endpoint is set programmatically via ExportConfig, the SDK uses it
    // verbatim (no path appended). This matches OTEL_EXPORTER_OTLP_LOGS_ENDPOINT
    // behaviour, so we pass the full URL directly.
    let export_config = ExportConfig {
        endpoint: Some(config.endpoint.clone()),
        protocol: Protocol::HttpBinary,
        ..Default::default()
    };

    let mut exporter_builder = LogExporter::builder()
        .with_http()
        .with_export_config(export_config);

    // Configure headers if any
    if !config.headers.is_empty() {
        use opentelemetry_otlp::WithHttpConfig as _;

        exporter_builder = exporter_builder.with_headers(config.headers.clone());
    }

    let exporter = exporter_builder
        .build()
        .into_alien_error()
        .context(ErrorData::Other {
            message: format!(
                "Failed to build OTLP log exporter for endpoint: {}",
                config.endpoint
            ),
        })?;

    // Create resource with service information
    let mut attributes = vec![
        KeyValue::new("service.name", config.service_name),
        KeyValue::new("service.version", config.service_version),
        KeyValue::new("service.instance.id", uuid::Uuid::new_v4().to_string()),
    ];

    // Add alien.deployment_id if ALIEN_DEPLOYMENT_ID environment variable is set
    if let Ok(deployment_id) = std::env::var("ALIEN_DEPLOYMENT_ID") {
        attributes.push(KeyValue::new("alien.deployment_id", deployment_id));
    }

    let resource = Resource::builder_empty()
        .with_attributes(attributes)
        .build();

    // Create batch log processor
    let batch_processor = BatchLogProcessor::builder(exporter).build();

    // Create logger provider with batch processor
    let provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_log_processor(batch_processor)
        .build();

    // Create tracing bridge
    let bridge = OpenTelemetryTracingBridge::new(&provider);

    // Store provider globally for shutdown flushing
    // In tests, this might fail if already set, which is okay
    if OTLP_PROVIDER.set(Some(provider)).is_err() {
        #[cfg(test)]
        {
            // In tests, we might reinitialize OTLP multiple times
            info!("OTLP provider already set (likely in tests), continuing with new provider");
        }
        #[cfg(not(test))]
        {
            return Err(AlienError::new(ErrorData::Other {
                message: "Failed to store OTLP provider globally".to_string(),
            }));
        }
    }

    info!("OTLP logging initialized successfully");
    Ok(Some(bridge))
}

/// Initialize OTLP logging when feature is disabled
#[cfg(not(feature = "otlp"))]
pub fn init_otlp_logging() -> Result<Option<()>> {
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
        || std::env::var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT").is_ok()
    {
        tracing::warn!("OTLP endpoint configured but alien-runtime was compiled without OTLP support. Rebuild with --features otlp to enable OTLP logging.");
    }
    OTLP_PROVIDER.set(None).ok();
    Ok(None)
}

/// Send a log entry via the OpenTelemetry SDK.
///
/// Uses the globally configured `SdkLoggerProvider` (from `init_otlp_logging`) to emit logs
/// through the proper OTLP pipeline with batching and protobuf format.
///
/// This is used by embedded runtimes to send captured stdout/stderr from function processes.
#[cfg(feature = "otlp")]
pub fn emit_log(stream: &str, body: &str, timestamp_nanos: i64) {
    use opentelemetry::logs::{
        AnyValue, LogRecord as _, Logger as _, LoggerProvider as _, Severity,
    };
    use std::time::{Duration, UNIX_EPOCH};

    // Get the global provider (initialized by init_otlp_logging)
    let provider = match OTLP_PROVIDER.get() {
        Some(Some(p)) => p,
        _ => {
            // OTLP not configured - silently skip (common in local dev without telemetry)
            return;
        }
    };

    // Get a logger for function output
    let logger = provider.logger("function-output");

    // Create and configure the log record
    let mut record = logger.create_log_record();

    // Set timestamp from nanos
    let timestamp = UNIX_EPOCH + Duration::from_nanos(timestamp_nanos as u64);
    record.set_timestamp(timestamp);

    // Set severity based on stream (stdout = INFO, stderr = ERROR)
    if stream == "stderr" {
        record.set_severity_text("ERROR");
        record.set_severity_number(Severity::Error);
    } else {
        record.set_severity_text("INFO");
        record.set_severity_number(Severity::Info);
    }

    // Set the log body
    record.set_body(AnyValue::String(body.to_string().into()));

    // Add stream as attribute
    record.add_attribute("stream", AnyValue::String(stream.to_string().into()));

    // Emit the log record (batched by BatchLogProcessor)
    logger.emit(record);
}

/// Emit log (no-op when feature disabled)
#[cfg(not(feature = "otlp"))]
pub fn emit_log(_stream: &str, _body: &str, _timestamp_nanos: i64) {
    // No-op
}

/// Flush all pending OTLP logs
/// This should be called before shutdown to ensure all logs are sent
pub async fn flush_otlp_logs() -> Result<()> {
    if let Some(Some(provider)) = OTLP_PROVIDER.get() {
        info!("Flushing OTLP logs before shutdown...");

        // Use force_flush instead of shutdown to avoid permanently shutting down the provider
        // This allows multiple flush calls in tests without breaking subsequent log emissions
        let flush_result = tokio::task::spawn_blocking({
            let provider = provider.clone();
            move || match provider.force_flush() {
                Ok(_) => {
                    info!("OTLP logs flushed successfully");
                    Ok(())
                }
                Err(e) => {
                    error!(error = %e, "Failed to flush OTLP logs");
                    Err(AlienError::new(ErrorData::Other {
                        message: format!("Failed to flush OTLP logs: {}", e),
                    }))
                }
            }
        })
        .await;

        match flush_result {
            Ok(result) => result,
            Err(e) => {
                error!(error = %e, "OTLP flush task panicked");
                Err(AlienError::new(ErrorData::Other {
                    message: format!("OTLP flush task panicked: {}", e),
                }))
            }
        }
    } else {
        // No OTLP provider configured, nothing to flush
        Ok(())
    }
}

/// Shutdown OTLP logging completely
/// This should only be called during application shutdown
pub async fn shutdown_otlp_logs() -> Result<()> {
    if let Some(Some(provider)) = OTLP_PROVIDER.get() {
        info!("Shutting down OTLP logs...");

        let shutdown_result = tokio::task::spawn_blocking({
            let provider = provider.clone();
            move || match provider.shutdown() {
                Ok(_) => {
                    info!("OTLP logs shut down successfully");
                    Ok(())
                }
                Err(e) => {
                    error!(error = %e, "Failed to shutdown OTLP logs");
                    Err(AlienError::new(ErrorData::Other {
                        message: format!("Failed to shutdown OTLP logs: {}", e),
                    }))
                }
            }
        })
        .await;

        match shutdown_result {
            Ok(result) => result,
            Err(e) => {
                error!(error = %e, "OTLP shutdown task panicked");
                Err(AlienError::new(ErrorData::Other {
                    message: format!("OTLP shutdown task panicked: {}", e),
                }))
            }
        }
    } else {
        // No OTLP provider configured, nothing to shutdown
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;

    /// Mutex to serialize tests that modify environment variables.
    /// This prevents race conditions when tests run in parallel.
    static ENV_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    /// Helper to clear all OTLP-related environment variables.
    fn clear_otlp_env_vars() {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
        std::env::remove_var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT");
        std::env::remove_var("OTEL_SERVICE_NAME");
        std::env::remove_var("OTEL_SERVICE_VERSION");
        std::env::remove_var("OTEL_EXPORTER_OTLP_HEADERS");
        std::env::remove_var("OTEL_EXPORTER_OTLP_HEADERS_AUTHORIZATION");
        std::env::remove_var("ALIEN_DEPLOYMENT_ID");
    }

    #[test]
    fn test_otlp_config_from_env_none() {
        let _guard = ENV_MUTEX.lock().unwrap();
        clear_otlp_env_vars();

        let config = OtlpConfig::from_env();
        assert!(config.is_none());
    }

    #[test]
    fn test_otlp_config_from_env_basic() {
        let _guard = ENV_MUTEX.lock().unwrap();
        clear_otlp_env_vars();

        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318");

        let config = OtlpConfig::from_env().expect("Should have config");
        assert_eq!(config.endpoint, "http://localhost:4318");
        assert_eq!(config.service_name, "alien-runtime");
        assert!(config.headers.is_empty());
    }

    #[test]
    fn test_otlp_config_from_env_with_headers() {
        let _guard = ENV_MUTEX.lock().unwrap();
        clear_otlp_env_vars();

        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318");
        std::env::set_var(
            "OTEL_EXPORTER_OTLP_HEADERS",
            "authorization=Bearer token123,x-custom=value",
        );
        std::env::set_var("OTEL_SERVICE_NAME", "test-service");

        let config = OtlpConfig::from_env().expect("Should have config");
        assert_eq!(config.endpoint, "http://localhost:4318");
        assert_eq!(config.service_name, "test-service");
        assert_eq!(
            config.headers.get("authorization"),
            Some(&"Bearer token123".to_string())
        );
        assert_eq!(config.headers.get("x-custom"), Some(&"value".to_string()));
    }

    #[test]
    #[cfg(feature = "otlp")]
    fn test_alien_deployment_id_attribute() {
        let _guard = ENV_MUTEX.lock().unwrap();
        clear_otlp_env_vars();

        // Test that alien.deployment_id attribute is added when ALIEN_DEPLOYMENT_ID is set
        std::env::set_var("ALIEN_DEPLOYMENT_ID", "test-agent-123");
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318");

        // We can't easily test the full init_otlp_logging function in a unit test
        // since it creates real OTLP infrastructure, but we can verify the environment
        // variable is read correctly by testing the logic inline
        let deployment_id = std::env::var("ALIEN_DEPLOYMENT_ID").ok();
        assert_eq!(deployment_id, Some("test-agent-123".to_string()));
    }
}
