mod normalization;

use crate::error::{ErrorData, Result};
use alien_core::ENV_ALIEN_RUNTIME_SECRETS;
use alien_error::{AlienError, Context, IntoAlienError};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};
use tracing::{error, info};

use alien_client_core::runtime_credentials::RuntimeCredentials;
use opentelemetry::KeyValue;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithHttpConfig;
use opentelemetry_otlp::{ExportConfig, LogExporter, Protocol, WithExportConfig};
use opentelemetry_sdk::{
    logs::{BatchLogProcessor, SdkLoggerProvider},
    Resource,
};

#[derive(Debug)]
struct IdentityHttpClient {
    client: reqwest::Client,
    credentials: Arc<RuntimeCredentials>,
    runtime: tokio::runtime::Handle,
}

#[async_trait::async_trait]
impl opentelemetry_http::HttpClient for IdentityHttpClient {
    async fn send_bytes(
        &self,
        mut request: http::Request<bytes::Bytes>,
    ) -> std::result::Result<http::Response<bytes::Bytes>, opentelemetry_http::HttpError> {
        let credentials = self.credentials.clone();
        let client = self.client.clone();
        // The batch processor exports from its own thread, outside Tokio.
        self.runtime
            .spawn(async move {
                let trusted = credentials.allows_endpoint(&request.uri().to_string());
                let token = if trusted {
                    Some(credentials.token().await?)
                } else {
                    None
                };
                if let Some(token) = &token {
                    request.headers_mut().insert(
                        http::header::AUTHORIZATION,
                        format!("Bearer {token}").parse()?,
                    );
                }
                let mut response = client.execute(request.clone().try_into()?).await?;
                if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                    if let Some(token) = &token {
                        let replacement = credentials.token_after_rejection(token).await?;
                        request.headers_mut().insert(
                            http::header::AUTHORIZATION,
                            format!("Bearer {replacement}").parse()?,
                        );
                        response = client.execute(request.try_into()?).await?;
                        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                            credentials.reject(&replacement).await;
                        }
                    }
                }
                let mut response = response.error_for_status()?;
                let headers = std::mem::take(response.headers_mut());
                let mut result = http::Response::builder()
                    .status(response.status())
                    .body(response.bytes().await?)?;
                *result.headers_mut() = headers;
                Ok::<_, opentelemetry_http::HttpError>(result)
            })
            .await?
    }
}

/// Global OTLP logger provider for captured application logs.
static OTLP_PROVIDER: LazyLock<Mutex<Option<SdkLoggerProvider>>> =
    LazyLock::new(|| Mutex::new(None));

const ENV_ALIEN_RUNTIME_SEND_OTLP: &str = "ALIEN_RUNTIME_SEND_OTLP";

/// Configuration for OTLP logging based on environment variables
#[derive(Debug, Clone)]
pub struct OtlpConfig {
    pub endpoint: String,
    pub headers: HashMap<String, String>,
    pub service_name: String,
    pub service_version: String,
}

impl OtlpConfig {
    /// Load OTLP configuration from environment variables
    pub fn from_env() -> Option<Self> {
        if runtime_otlp_disabled() {
            return None;
        }

        // Logs-specific endpoint takes precedence over generic endpoint
        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT")
            .or_else(|_| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT"))
            .ok()?;

        let service_name = std::env::var("OTEL_SERVICE_NAME")
            .unwrap_or_else(|_| "alien-worker-runtime".to_string());

        let service_version = std::env::var("OTEL_SERVICE_VERSION")
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());

        // Parse headers from environment
        let mut headers = HashMap::new();

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

fn runtime_otlp_disabled() -> bool {
    std::env::var(ENV_ALIEN_RUNTIME_SEND_OTLP)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "false" | "0" | "off"
            )
        })
        .unwrap_or(false)
}

/// Initialize OTLP logging and return the tracing bridge layer
#[cfg(feature = "otlp")]
pub fn init_otlp_logging(
) -> Result<Option<OpenTelemetryTracingBridge<SdkLoggerProvider, opentelemetry_sdk::logs::SdkLogger>>>
{
    // Runtime telemetry credentials are deliberately kept in the deployment's
    // secret provider rather than the process environment. `run` resolves them
    // asynchronously and installs the authenticated app-log provider later. Do
    // not attach an unauthenticated tracing bridge during process bootstrap: it
    // would keep exporting through its original provider even after the global
    // app-log provider is replaced.
    if runtime_otlp_credentials_are_vault_backed() {
        info!("Deferring OTLP initialization until runtime credentials are loaded");
        *OTLP_PROVIDER.lock().expect("OTLP provider mutex poisoned") = None;
        return Ok(None);
    }

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
            *OTLP_PROVIDER.lock().expect("OTLP provider mutex poisoned") = None;
            return Ok(None);
        }
    };

    let provider = build_otlp_provider(&config)?;
    let bridge = OpenTelemetryTracingBridge::new(&provider);
    store_otlp_provider(provider);

    info!("OTLP logging initialized successfully");
    Ok(Some(bridge))
}

fn runtime_otlp_credentials_are_vault_backed() -> bool {
    std::env::var_os(ENV_ALIEN_RUNTIME_SECRETS).is_some()
        || std::env::var_os("ALIEN_IDENTITY_PROVIDER").is_some()
}

/// Initialize the app-log OTLP provider from already-resolved runtime config.
#[cfg(feature = "otlp")]
pub fn init_otlp_logging_from_config(config: OtlpConfig) -> Result<()> {
    info!(
        endpoint = %config.endpoint,
        service_name = %config.service_name,
        service_version = %config.service_version,
        "Initializing app log OTLP exporter"
    );
    let provider = build_otlp_provider(&config)?;
    store_otlp_provider(provider);
    Ok(())
}

/// Install a log exporter that refreshes its bounded authorization per export.
pub fn init_otlp_logging_with_identity(
    config: OtlpConfig,
    credentials: Arc<RuntimeCredentials>,
) -> Result<()> {
    let provider = build_otlp_provider_with_identity(&config, Some(credentials))?;
    store_otlp_provider(provider);
    Ok(())
}

/// An OTLP app-log exporter owned by a single caller.
///
/// Unlike the process-global provider that `init_otlp_logging*` installs, an
/// owned logger keeps its own endpoint/service identity and flush lifecycle.
/// This lets several concurrent callers (e.g. multiple local daemons) each
/// export under their own identity without clobbering one another, and lets one
/// caller's shutdown flush only its own logs.
#[cfg(feature = "otlp")]
#[derive(Debug)]
pub struct OwnedOtlpLogger {
    provider: SdkLoggerProvider,
}

#[cfg(feature = "otlp")]
impl OwnedOtlpLogger {
    /// Builds an owned OTLP logger from already-resolved config, without ever
    /// touching the process-global `OTLP_PROVIDER`.
    pub fn from_config(config: OtlpConfig) -> Result<Self> {
        info!(
            endpoint = %config.endpoint,
            service_name = %config.service_name,
            service_version = %config.service_version,
            "Initializing owned app log OTLP exporter"
        );
        let provider = build_otlp_provider(&config)?;
        Ok(Self { provider })
    }

    /// Emits a captured stdout/stderr line through this logger's own provider.
    pub fn emit_log(&self, stream: &str, body: &str, timestamp_nanos: i64) {
        emit_to_provider(&self.provider, stream, body, timestamp_nanos, false);
    }

    /// Force-flushes this logger's own pending logs (used before shutdown).
    pub async fn flush(&self) -> Result<()> {
        force_flush_provider(self.provider.clone()).await
    }
}

/// Owned OTLP logger when the feature is disabled: a no-op that keeps callers
/// (e.g. the local daemon supervisor) uniform across build configurations.
#[cfg(not(feature = "otlp"))]
#[derive(Debug)]
pub struct OwnedOtlpLogger;

#[cfg(not(feature = "otlp"))]
impl OwnedOtlpLogger {
    pub fn from_config(_config: OtlpConfig) -> Result<Self> {
        tracing::warn!("OTLP configuration provided but alien-worker-runtime was compiled without OTLP support. Rebuild with --features otlp to enable OTLP logging.");
        Ok(Self)
    }

    pub fn emit_log(&self, _stream: &str, _body: &str, _timestamp_nanos: i64) {}

    pub async fn flush(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(feature = "otlp")]
fn build_otlp_provider(config: &OtlpConfig) -> Result<SdkLoggerProvider> {
    build_otlp_provider_with_identity(config, None)
}

fn build_otlp_provider_with_identity(
    config: &OtlpConfig,
    credentials: Option<Arc<RuntimeCredentials>>,
) -> Result<SdkLoggerProvider> {
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

    if let Some(credentials) = credentials {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Cannot initialize telemetry transport".into(),
            })?;
        exporter_builder = exporter_builder.with_http_client(IdentityHttpClient {
            client,
            credentials,
            runtime: tokio::runtime::Handle::try_current()
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Runtime identity telemetry requires an async runtime".into(),
                })?,
        });
    }

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

    let resource = build_otlp_resource(&config);

    // Create batch log processor
    let batch_processor = BatchLogProcessor::builder(exporter).build();

    // Create logger provider with batch processor
    let provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_log_processor(batch_processor)
        .build();

    Ok(provider)
}

#[cfg(feature = "otlp")]
pub(crate) fn store_otlp_provider(provider: SdkLoggerProvider) {
    *OTLP_PROVIDER.lock().expect("OTLP provider mutex poisoned") = Some(provider);
}

#[cfg(feature = "otlp")]
fn build_otlp_resource(config: &OtlpConfig) -> Resource {
    // Resource::builder() includes OTEL_RESOURCE_ATTRIBUTES and SDK-provided
    // telemetry attributes. Explicit runtime service attributes override env
    // values for the keys owned by alien-worker-runtime.
    let mut attributes = vec![
        KeyValue::new("service.name", config.service_name.clone()),
        KeyValue::new("service.version", config.service_version.clone()),
        KeyValue::new("service.instance.id", uuid::Uuid::new_v4().to_string()),
    ];

    if let Ok(deployment_id) = std::env::var("ALIEN_DEPLOYMENT_ID") {
        attributes.push(KeyValue::new("alien.deployment_id", deployment_id));
    }

    Resource::builder().with_attributes(attributes).build()
}

/// Initialize OTLP logging when feature is disabled
#[cfg(not(feature = "otlp"))]
pub fn init_otlp_logging() -> Result<Option<()>> {
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
        || std::env::var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT").is_ok()
    {
        tracing::warn!("OTLP endpoint configured but alien-worker-runtime was compiled without OTLP support. Rebuild with --features otlp to enable OTLP logging.");
    }
    *OTLP_PROVIDER.lock().expect("OTLP provider mutex poisoned") = None;
    Ok(None)
}

/// Initialize OTLP logging when feature is disabled.
#[cfg(not(feature = "otlp"))]
pub fn init_otlp_logging_from_config(_config: OtlpConfig) -> Result<()> {
    tracing::warn!("OTLP configuration provided but alien-worker-runtime was compiled without OTLP support. Rebuild with --features otlp to enable OTLP logging.");
    Ok(())
}

/// Send a log entry via the OpenTelemetry SDK.
///
/// Uses the globally configured `SdkLoggerProvider` (from `init_otlp_logging`) to emit logs
/// through the proper OTLP pipeline with batching and protobuf format.
///
/// This is used by embedded runtimes to send captured stdout/stderr from function processes.
#[cfg(feature = "otlp")]
pub fn emit_log(stream: &str, body: &str, timestamp_nanos: i64) {
    emit_captured_log(stream, body, timestamp_nanos, false);
}

#[cfg(feature = "otlp")]
pub(crate) fn emit_captured_log(stream: &str, body: &str, timestamp_nanos: i64, is_system: bool) {
    // Get the global provider (initialized by init_otlp_logging)
    let configured = {
        let guard = OTLP_PROVIDER.lock().expect("OTLP provider mutex poisoned");
        match guard.as_ref() {
            Some(configured) => configured.clone(),
            None => {
                // OTLP not configured - silently skip (common in local dev without telemetry)
                return;
            }
        }
    };

    emit_to_provider(&configured, stream, body, timestamp_nanos, is_system);
}

/// Emits a single captured log line through a specific provider. Shared by the
/// process-global [`emit_log`] and the per-caller [`OwnedOtlpLogger`].
#[cfg(feature = "otlp")]
fn emit_to_provider(
    provider: &SdkLoggerProvider,
    stream: &str,
    body: &str,
    timestamp_nanos: i64,
    is_system: bool,
) {
    use opentelemetry::logs::{AnyValue, LogRecord as _, Logger as _, LoggerProvider as _};
    use std::time::{Duration, UNIX_EPOCH};

    let logger = provider.logger("function-output");
    let mut record = logger.create_log_record();
    let log = lognorm::parse(body);
    let collected = u64::try_from(timestamp_nanos)
        .ok()
        .and_then(|nanos| UNIX_EPOCH.checked_add(Duration::from_nanos(nanos)))
        .unwrap_or(UNIX_EPOCH);
    record.set_observed_timestamp(collected);
    record.set_timestamp(if is_system {
        collected
    } else {
        normalization::timestamp(&log).unwrap_or(collected)
    });

    let (severity, severity_text) = captured_log_severity(stream, &log, is_system);
    record.set_severity_text(severity_text);
    record.set_severity_number(severity);
    record.set_body(AnyValue::String(
        if is_system { body } else { log.message() }
            .to_owned()
            .into(),
    ));
    if !is_system {
        if log.outcome() == lognorm::ParseOutcome::Structured {
            record.add_attribute("app", normalization::fields(log.fields()));
        }
        if log.outcome() == lognorm::ParseOutcome::Structured || log.message() != body {
            record.add_attribute("log.record.original", body.to_owned());
        }
    }

    // Add stream as attribute
    record.add_attribute("stream", AnyValue::String(stream.to_string().into()));
    if is_system {
        record.add_attribute(
            alien_core::ALIEN_SYSTEM_RESOURCE_ATTRIBUTE,
            AnyValue::String("true".into()),
        );
    }

    // Emit the log record (batched by BatchLogProcessor)
    logger.emit(record);
}

#[cfg(feature = "otlp")]
fn captured_log_severity(
    stream: &str,
    log: &lognorm::ParsedLog<'_>,
    is_system: bool,
) -> (opentelemetry::logs::Severity, &'static str) {
    use opentelemetry::logs::Severity;
    if !is_system {
        if let Some(severity) = normalization::severity(log) {
            return (
                severity,
                log.severity().map(lognorm::Severity::as_str).unwrap_or(""),
            );
        }
    }
    if stream == "stderr" {
        (Severity::Error, "ERROR")
    } else {
        (Severity::Info, "INFO")
    }
}

/// Emit log (no-op when feature disabled)
#[cfg(not(feature = "otlp"))]
pub fn emit_log(_stream: &str, _body: &str, _timestamp_nanos: i64) {
    // No-op
}

#[cfg(not(feature = "otlp"))]
pub(crate) fn emit_captured_log(
    _stream: &str,
    _body: &str,
    _timestamp_nanos: i64,
    _is_system: bool,
) {
}

/// Exports every pending app log. Called before shutdown and, on Lambda, after each invocation,
/// since Lambda freezes the environment once the invocation completes.
pub async fn flush_otlp_logs() -> Result<()> {
    let configured = OTLP_PROVIDER
        .lock()
        .expect("OTLP provider mutex poisoned")
        .clone();
    if let Some(provider) = configured {
        force_flush_provider(provider).await
    } else {
        // No OTLP provider configured, nothing to flush
        Ok(())
    }
}

/// Force-flushes a specific provider's pending logs on a blocking task. Uses
/// `force_flush` (not `shutdown`) so the provider stays usable afterwards.
/// Shared by [`flush_otlp_logs`] and [`OwnedOtlpLogger::flush`].
async fn force_flush_provider(provider: SdkLoggerProvider) -> Result<()> {
    let flush_result = tokio::task::spawn_blocking(move || match provider.force_flush() {
        Ok(_) => Ok(()),
        Err(e) => {
            error!(error = %e, "Failed to flush OTLP logs");
            Err(AlienError::new(ErrorData::Other {
                message: format!("Failed to flush OTLP logs: {}", e),
            }))
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
}

/// Shutdown OTLP logging completely
/// This should only be called during application shutdown
pub async fn shutdown_otlp_logs() -> Result<()> {
    let configured = OTLP_PROVIDER
        .lock()
        .expect("OTLP provider mutex poisoned")
        .clone();
    if let Some(provider) = configured {
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
        std::env::remove_var("OTEL_RESOURCE_ATTRIBUTES");
        std::env::remove_var("OTEL_EXPORTER_OTLP_HEADERS");
        std::env::remove_var("OTEL_EXPORTER_OTLP_HEADERS_AUTHORIZATION");
        std::env::remove_var("ALIEN_DEPLOYMENT_ID");
        std::env::remove_var(ENV_ALIEN_RUNTIME_SEND_OTLP);
        std::env::remove_var(ENV_ALIEN_RUNTIME_SECRETS);
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
        assert_eq!(config.service_name, "alien-worker-runtime");
        assert!(config.headers.is_empty());
    }

    #[test]
    #[cfg(feature = "otlp")]
    fn captured_log_severity_uses_application_level_for_application_output_only() {
        use opentelemetry::logs::Severity;

        let body = r#"{"level":"INFO","fields":{"message":"ready"}}"#;
        assert_eq!(
            captured_log_severity("stderr", &lognorm::parse(body), false),
            (Severity::Info, "INFO")
        );
        assert_eq!(
            captured_log_severity("stderr", &lognorm::parse(body), true),
            (Severity::Error, "ERROR")
        );
        assert_eq!(
            captured_log_severity("stderr", &lognorm::parse(r#"{"level":"LOUD"}"#), false),
            (Severity::Error, "ERROR")
        );
    }

    #[test]
    #[cfg(feature = "otlp")]
    fn captured_structured_stdout_has_readable_body_and_original_record() {
        use opentelemetry::logs::AnyValue;
        use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLoggerProvider};

        let exporter = InMemoryLogExporter::default();
        let provider = SdkLoggerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let original = r#"{"time":"2026-09-26T12:12:02Z","level":"INFO","msg":"ready","requestId":"request-1"}"#;

        emit_to_provider(
            &provider,
            "stdout",
            original,
            1_780_000_000_000_000_000,
            false,
        );
        emit_to_provider(
            &provider,
            "stdout",
            original,
            1_780_000_000_000_000_001,
            true,
        );

        let logs = exporter.get_emitted_logs().unwrap();
        assert_eq!(logs.len(), 2);
        assert!(matches!(
            logs[0].record.body(),
            Some(AnyValue::String(body)) if body.as_str() == "ready"
        ));
        assert!(logs[0].record.attributes_iter().any(|(key, value)| {
            key.as_str() == "log.record.original"
                && matches!(value, AnyValue::String(body) if body.as_str() == original)
        }));
        assert!(matches!(
            logs[1].record.body(),
            Some(AnyValue::String(body)) if body.as_str() == original
        ));
    }

    #[test]
    fn test_otlp_config_from_env_respects_disable_flag() {
        let _guard = ENV_MUTEX.lock().unwrap();
        clear_otlp_env_vars();

        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318");
        std::env::set_var(ENV_ALIEN_RUNTIME_SEND_OTLP, "false");

        let config = OtlpConfig::from_env();
        assert!(config.is_none());
    }

    #[test]
    fn test_runtime_otlp_credentials_are_vault_backed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        clear_otlp_env_vars();

        assert!(!runtime_otlp_credentials_are_vault_backed());
        std::env::set_var(
            ENV_ALIEN_RUNTIME_SECRETS,
            r#"{"otlpLogsAuthHeader":"secret"}"#,
        );
        assert!(runtime_otlp_credentials_are_vault_backed());
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

        std::env::set_var("ALIEN_DEPLOYMENT_ID", "test-agent-123");
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318");

        let config = OtlpConfig::from_env().expect("Should have config");
        let resource = build_otlp_resource(&config);

        assert_eq!(
            resource.get(&opentelemetry::Key::new("alien.deployment_id")),
            Some(opentelemetry::Value::String("test-agent-123".into()))
        );
    }

    #[test]
    #[cfg(feature = "otlp")]
    fn test_resource_includes_otel_resource_attributes() {
        let _guard = ENV_MUTEX.lock().unwrap();
        clear_otlp_env_vars();

        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318");
        std::env::set_var(
            "OTEL_RESOURCE_ATTRIBUTES",
            "alien.workspace_id=ws_test,alien.deployment_id=dep_from_resource,service.name=ignored",
        );
        std::env::set_var("OTEL_SERVICE_NAME", "runtime-service");

        let config = OtlpConfig::from_env().expect("Should have config");
        let resource = build_otlp_resource(&config);

        assert_eq!(
            resource.get(&opentelemetry::Key::new("alien.workspace_id")),
            Some(opentelemetry::Value::String("ws_test".into()))
        );
        assert_eq!(
            resource.get(&opentelemetry::Key::new("alien.deployment_id")),
            Some(opentelemetry::Value::String("dep_from_resource".into()))
        );
        assert_eq!(
            resource.get(&opentelemetry::Key::new("service.name")),
            Some(opentelemetry::Value::String("runtime-service".into()))
        );
    }
}

#[cfg(test)]
mod normalization_tests;

#[cfg(test)]
mod identity_export_tests {
    use super::*;
    use alien_client_core::runtime_credentials::{RuntimeCredentialConfig, RuntimeProofProvider};
    use async_trait::async_trait;
    use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

    struct Proof;
    #[async_trait]
    impl RuntimeProofProvider for Proof {
        async fn proof(&self, audience: &str) -> alien_client_core::Result<Value> {
            assert_eq!(audience, "test-manager");
            Ok(json!({"type":"session", "token":"test-parent"}))
        }
    }
    #[derive(Clone)]
    struct Fixture {
        clock: Arc<AtomicI64>,
        exchanges: Arc<AtomicUsize>,
        deliveries: Arc<Mutex<Vec<String>>>,
    }

    async fn exchange(State(state): State<Fixture>, Json(request): Json<Value>) -> Json<Value> {
        assert_eq!(request["purpose"], json!({"type":"telemetry"}));
        let revision = state.exchanges.fetch_add(1, Ordering::SeqCst);
        Json(
            json!({"token":format!("bounded-{revision}"), "expiresAt":chrono::DateTime::from_timestamp(state.clock.load(Ordering::SeqCst) + 100, 0).unwrap()}),
        )
    }
    async fn logs(
        State(state): State<Fixture>,
        headers: http::HeaderMap,
        body: bytes::Bytes,
    ) -> StatusCode {
        assert!(
            !body.is_empty(),
            "export must carry a real protobuf log batch"
        );
        state.deliveries.lock().unwrap().push(
            headers[http::header::AUTHORIZATION]
                .to_str()
                .unwrap()
                .into(),
        );
        StatusCode::OK
    }

    #[tokio::test]
    async fn persistent_rejection_retries_once_and_discards_each_rejected_token() {
        use opentelemetry_http::HttpClient;
        let state = Fixture {
            clock: Arc::new(AtomicI64::new(1_800_000_000)),
            exchanges: Arc::new(AtomicUsize::new(0)),
            deliveries: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/identity/exchange", post(exchange))
            .route(
                "/v1/logs",
                post(
                    |State(state): State<Fixture>, headers: http::HeaderMap| async move {
                        state.deliveries.lock().unwrap().push(
                            headers[http::header::AUTHORIZATION]
                                .to_str()
                                .unwrap()
                                .into(),
                        );
                        StatusCode::UNAUTHORIZED
                    },
                ),
            )
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let clock = state.clock.clone();
        let credentials = RuntimeCredentials::new(
            RuntimeCredentialConfig {
                manager_url: format!("http://{address}"),
                audience: "test-manager".into(),
                deployment_id: "deployment-one".into(),
                purpose: json!({"type":"telemetry"}),
            },
            Arc::new(Proof),
        )
        .unwrap()
        .with_clock(Arc::new(move || {
            chrono::DateTime::from_timestamp(clock.load(Ordering::SeqCst), 0).unwrap()
        }));
        let transport = IdentityHttpClient {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            credentials: Arc::new(credentials),
            runtime: tokio::runtime::Handle::current(),
        };
        for _ in 0..2 {
            let request = http::Request::post(format!("http://{address}/v1/logs"))
                .body(bytes::Bytes::from_static(b"fixture"))
                .unwrap();
            assert!(transport.send_bytes(request).await.is_err());
        }
        assert_eq!(state.exchanges.load(Ordering::SeqCst), 4);
        assert_eq!(
            *state.deliveries.lock().unwrap(),
            [
                "Bearer bounded-0",
                "Bearer bounded-1",
                "Bearer bounded-2",
                "Bearer bounded-3"
            ]
        );
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn external_rejection_never_exchanges_or_retries_with_manager_authority() {
        use opentelemetry_http::HttpClient;
        let state = Fixture {
            clock: Arc::new(AtomicI64::new(1_800_000_000)),
            exchanges: Arc::new(AtomicUsize::new(0)),
            deliveries: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/identity/exchange", post(exchange))
            .route(
                "/v1/logs",
                post(
                    |State(state): State<Fixture>, headers: http::HeaderMap| async move {
                        state.deliveries.lock().unwrap().push(
                            headers[http::header::AUTHORIZATION]
                                .to_str()
                                .unwrap()
                                .into(),
                        );
                        StatusCode::UNAUTHORIZED
                    },
                ),
            )
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let clock = state.clock.clone();
        let credentials = RuntimeCredentials::new(
            RuntimeCredentialConfig {
                manager_url: "https://manager.example".into(),
                audience: "test-manager".into(),
                deployment_id: "deployment-one".into(),
                purpose: json!({"type":"telemetry"}),
            },
            Arc::new(Proof),
        )
        .unwrap()
        .with_clock(Arc::new(move || {
            chrono::DateTime::from_timestamp(clock.load(Ordering::SeqCst), 0).unwrap()
        }));
        let transport = IdentityHttpClient {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            credentials: Arc::new(credentials),
            runtime: tokio::runtime::Handle::current(),
        };
        for _ in 0..2 {
            let request = http::Request::post(format!("http://{address}/v1/logs"))
                .header(http::header::AUTHORIZATION, "Bearer external-fixture")
                .body(bytes::Bytes::from_static(b"fixture"))
                .unwrap();
            assert!(transport.send_bytes(request).await.is_err());
        }
        assert_eq!(state.exchanges.load(Ordering::SeqCst), 0);
        assert_eq!(
            *state.deliveries.lock().unwrap(),
            ["Bearer external-fixture", "Bearer external-fixture"]
        );
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn concurrent_stale_rejections_share_one_replacement() {
        use opentelemetry_http::HttpClient;
        let state = Fixture {
            clock: Arc::new(AtomicI64::new(1_800_000_000)),
            exchanges: Arc::new(AtomicUsize::new(0)),
            deliveries: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let app = Router::new()
            .route("/v1/identity/exchange", post(exchange))
            .route(
                "/v1/logs",
                post(
                    move |State(state): State<Fixture>, headers: http::HeaderMap| {
                        let barrier = barrier.clone();
                        async move {
                            let token = headers[http::header::AUTHORIZATION]
                                .to_str()
                                .unwrap()
                                .to_owned();
                            state.deliveries.lock().unwrap().push(token.clone());
                            if token == "Bearer bounded-0" {
                                barrier.wait().await;
                                StatusCode::UNAUTHORIZED
                            } else {
                                StatusCode::OK
                            }
                        }
                    },
                ),
            )
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let clock = state.clock.clone();
        let credentials = RuntimeCredentials::new(
            RuntimeCredentialConfig {
                manager_url: format!("http://{address}"),
                audience: "test-manager".into(),
                deployment_id: "deployment-one".into(),
                purpose: json!({"type":"telemetry"}),
            },
            Arc::new(Proof),
        )
        .unwrap()
        .with_clock(Arc::new(move || {
            chrono::DateTime::from_timestamp(clock.load(Ordering::SeqCst), 0).unwrap()
        }));
        let transport = IdentityHttpClient {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            credentials: Arc::new(credentials),
            runtime: tokio::runtime::Handle::current(),
        };
        let request = || {
            http::Request::post(format!("http://{address}/v1/logs"))
                .body(bytes::Bytes::from_static(b"fixture"))
                .unwrap()
        };
        let (one, two) = tokio::join!(
            transport.send_bytes(request()),
            transport.send_bytes(request())
        );
        assert_eq!(one.unwrap().status(), StatusCode::OK);
        assert_eq!(two.unwrap().status(), StatusCode::OK);
        assert_eq!(state.exchanges.load(Ordering::SeqCst), 2);
        assert_eq!(
            *state.deliveries.lock().unwrap(),
            [
                "Bearer bounded-0",
                "Bearer bounded-0",
                "Bearer bounded-1",
                "Bearer bounded-1"
            ]
        );
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn external_export_retains_its_auth_without_receiving_manager_credential() {
        let state = Fixture {
            clock: Arc::new(AtomicI64::new(1_800_000_000)),
            exchanges: Arc::new(AtomicUsize::new(0)),
            deliveries: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/logs", post(logs))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let credentials = RuntimeCredentials::new(
            RuntimeCredentialConfig {
                manager_url: "https://manager.example".into(),
                audience: "test-manager".into(),
                deployment_id: "deployment-one".into(),
                purpose: json!({"type":"telemetry"}),
            },
            Arc::new(Proof),
        )
        .unwrap();
        let config = OtlpConfig {
            endpoint: format!("http://{address}/v1/logs"),
            headers: HashMap::from([("authorization".into(), "Bearer external-service".into())]),
            service_name: "test-worker".into(),
            service_version: "1".into(),
        };
        let provider =
            build_otlp_provider_with_identity(&config, Some(Arc::new(credentials))).unwrap();
        emit_to_provider(&provider, "stdout", "external log", 0, false);
        force_flush_provider(provider.clone()).await.unwrap();
        assert_eq!(
            *state.deliveries.lock().unwrap(),
            ["Bearer external-service"]
        );
        tokio::task::spawn_blocking(move || provider.shutdown().unwrap())
            .await
            .unwrap();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn actual_log_exports_refresh_authorization_without_restarting_provider() {
        let state = Fixture {
            clock: Arc::new(AtomicI64::new(1_800_000_000)),
            exchanges: Arc::new(AtomicUsize::new(0)),
            deliveries: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/identity/exchange", post(exchange))
            .route("/v1/logs", post(logs))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let clock = state.clock.clone();
        let credentials = RuntimeCredentials::new(
            RuntimeCredentialConfig {
                manager_url: format!("http://{address}"),
                audience: "test-manager".into(),
                deployment_id: "deployment-one".into(),
                purpose: json!({"type":"telemetry"}),
            },
            Arc::new(Proof),
        )
        .unwrap()
        .with_clock(Arc::new(move || {
            chrono::DateTime::from_timestamp(clock.load(Ordering::SeqCst), 0).unwrap()
        }));
        let config = OtlpConfig {
            endpoint: format!("http://{address}/v1/logs"),
            headers: HashMap::from([(
                "authorization".into(),
                "Bearer legacy-must-not-be-used".into(),
            )]),
            service_name: "test-worker".into(),
            service_version: "1".into(),
        };
        let provider =
            build_otlp_provider_with_identity(&config, Some(Arc::new(credentials))).unwrap();
        emit_to_provider(&provider, "stdout", "first log", 0, false);
        force_flush_provider(provider.clone()).await.unwrap();
        state.clock.store(1_800_000_100, Ordering::SeqCst);
        emit_to_provider(&provider, "stdout", "second log", 0, false);
        force_flush_provider(provider.clone()).await.unwrap();
        assert_eq!(state.exchanges.load(Ordering::SeqCst), 2);
        assert_eq!(
            *state.deliveries.lock().unwrap(),
            ["Bearer bounded-0", "Bearer bounded-1"]
        );
        tokio::task::spawn_blocking(move || provider.shutdown().unwrap())
            .await
            .unwrap();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
    #[tokio::test]
    async fn actual_protobuf_export_recovers_immediately_after_rejection() {
        let state = Fixture {
            clock: Arc::new(AtomicI64::new(1_800_000_000)),
            exchanges: Arc::new(AtomicUsize::new(0)),
            deliveries: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app =
            Router::new()
                .route("/v1/identity/exchange", post(exchange))
                .route(
                    "/v1/logs",
                    post(
                        |State(state): State<Fixture>,
                         headers: http::HeaderMap,
                         body: bytes::Bytes| async move {
                            assert!(!body.is_empty());
                            assert_eq!(
                                headers[http::header::CONTENT_TYPE],
                                "application/x-protobuf"
                            );
                            let token = headers[http::header::AUTHORIZATION]
                                .to_str()
                                .unwrap()
                                .to_owned();
                            state.deliveries.lock().unwrap().push(token.clone());
                            if token == "Bearer bounded-0" {
                                StatusCode::UNAUTHORIZED
                            } else {
                                StatusCode::OK
                            }
                        },
                    ),
                )
                .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let clock = state.clock.clone();
        let credentials = RuntimeCredentials::new(
            RuntimeCredentialConfig {
                manager_url: format!("http://{address}"),
                audience: "test-manager".into(),
                deployment_id: "deployment-one".into(),
                purpose: json!({"type":"telemetry"}),
            },
            Arc::new(Proof),
        )
        .unwrap()
        .with_clock(Arc::new(move || {
            chrono::DateTime::from_timestamp(clock.load(Ordering::SeqCst), 0).unwrap()
        }));
        let config = OtlpConfig {
            endpoint: format!("http://{address}/v1/logs"),
            headers: HashMap::from([(
                "authorization".into(),
                "Bearer legacy-must-not-be-used".into(),
            )]),
            service_name: "test-worker".into(),
            service_version: "1".into(),
        };
        let provider =
            build_otlp_provider_with_identity(&config, Some(Arc::new(credentials))).unwrap();
        emit_to_provider(&provider, "stdout", "first log", 0, false);
        force_flush_provider(provider.clone()).await.unwrap();
        emit_to_provider(&provider, "stdout", "second log", 0, false);
        force_flush_provider(provider.clone()).await.unwrap();
        assert_eq!(state.exchanges.load(Ordering::SeqCst), 2);
        assert_eq!(
            *state.deliveries.lock().unwrap(),
            ["Bearer bounded-0", "Bearer bounded-1", "Bearer bounded-1"]
        );
        tokio::task::spawn_blocking(move || provider.shutdown().unwrap())
            .await
            .unwrap();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}
