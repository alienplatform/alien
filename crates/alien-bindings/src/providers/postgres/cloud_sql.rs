//! GCP Cloud SQL Postgres provider.
//!
//! The binding carries the Secret Manager secret **name** of the connection password,
//! not the password. This provider reads the secret's latest version with the workload's
//! own identity (granted by the `postgres/data-access` permission set) and builds the
//! connection parameters.

use crate::error::{ErrorData, Result};
use crate::providers::postgres::{cloud::resolve_secret_locator, resolve_params};
use crate::traits::{PostgresConnectionParams, SslMode};
use alien_core::bindings::CloudSqlPostgresBinding;
use alien_error::{AlienError, Context};
use alien_gcp_clients::secret_manager::SecretManagerApi;
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use std::sync::Arc;

/// Reads the password from Secret Manager and resolves the connection parameters.
///
/// The workload dials the binding's `host` (the Private Service Connect consumer
/// endpoint) and TLS is required (`sslmode=require`).
///
/// Performs exactly one `accessSecretVersion`; a failure is returned to the caller,
/// which owns any retry policy.
pub(crate) async fn resolve(
    binding_name: &str,
    binding: &CloudSqlPostgresBinding,
    secrets: Arc<dyn SecretManagerApi>,
) -> Result<PostgresConnectionParams> {
    let secret_name = resolve_secret_locator(
        binding_name,
        "passwordSecretName",
        &binding.password_secret_name,
    )?;
    let password = read_password(binding_name, &secret_name, secrets.as_ref()).await?;

    resolve_params(
        binding_name,
        &binding.host,
        &binding.port,
        &binding.database,
        &binding.username,
        &password,
        SslMode::Require,
    )
}

/// Reads the raw password the GCP controller stored as the secret version's payload.
///
/// The client scopes the name to the credential's project, so `{name}/versions/latest`
/// is the whole relative resource name this needs to pass.
async fn read_password(
    binding_name: &str,
    secret_name: &str,
    secrets: &dyn SecretManagerApi,
) -> Result<String> {
    let failed = |reason: String| ErrorData::PostgresSecretResolutionFailed {
        binding_name: binding_name.to_string(),
        secret: secret_name.to_string(),
        reason,
    };

    let response = secrets
        .access_secret_version(format!("{secret_name}/versions/latest"))
        .await
        .context(failed(
            "Secret Manager accessSecretVersion failed".to_string(),
        ))?;

    // Secret Manager returns the payload base64-encoded. An absent, empty, or
    // undecodable payload is a control-plane invariant the workload cannot fix locally,
    // so it reports the same (retryable) resolution failure as a failed read rather than
    // connecting with an empty password.
    let encoded = response
        .payload
        .and_then(|payload| payload.data)
        .ok_or_else(|| AlienError::new(failed("secret version has no payload".to_string())))?;

    let decoded = base64_standard.decode(&encoded).map_err(|error| {
        AlienError::new(failed(format!("payload is not valid base64: {error}")))
    })?;

    if decoded.is_empty() {
        return Err(AlienError::new(failed(
            "secret version payload is empty".to_string(),
        )));
    }

    String::from_utf8(decoded)
        .map_err(|error| AlienError::new(failed(format!("payload is not valid UTF-8: {error}"))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bindings::BindingValue;
    use alien_gcp_clients::secret_manager::{
        AccessSecretVersionResponse, MockSecretManagerApi, SecretPayload,
    };

    const SECRET_NAME: &str = "pg-credentials";

    fn binding() -> CloudSqlPostgresBinding {
        CloudSqlPostgresBinding {
            host: "10.0.0.5".into(),
            port: BindingValue::value(5432),
            database: "app".into(),
            username: "alien".into(),
            password_secret_name: SECRET_NAME.into(),
        }
    }

    fn response(payload: Option<&str>) -> AccessSecretVersionResponse {
        AccessSecretVersionResponse {
            name: Some(format!("projects/p/secrets/{SECRET_NAME}/versions/1")),
            payload: payload.map(|data| SecretPayload {
                data: Some(base64_standard.encode(data)),
            }),
        }
    }

    /// The happy path: the latest version of the named secret is read, the binding's host
    /// is dialed, TLS is required, and the base64 payload is decoded into the password.
    /// The password contains every RFC 3986 sub-delim that `encodeURIComponent` leaves
    /// literal, extending the encoding contract pinned in `local.rs` to this backend.
    #[tokio::test]
    async fn resolves_host_and_require_sslmode_from_latest_version() {
        let mut secrets = MockSecretManagerApi::new();
        secrets
            .expect_access_secret_version()
            .times(1)
            .withf(|name| name == "pg-credentials/versions/latest")
            .returning(|_| Ok(response(Some("a!b*c'd(e)f@/"))));

        let params = resolve("db", &binding(), Arc::new(secrets))
            .await
            .expect("cloud sql binding resolves");

        assert_eq!(params.host, "10.0.0.5");
        assert_eq!(params.port, 5432);
        assert_eq!(params.database, "app");
        assert_eq!(params.username, "alien");
        assert_eq!(params.password, "a!b*c'd(e)f@/");
        assert_eq!(params.sslmode, SslMode::Require);
        assert_eq!(
            params.connection_string(),
            "postgres://alien:a%21b%2Ac%27d%28e%29f%40%2F@10.0.0.5:5432/app?sslmode=require"
        );
    }

    /// A failed secret read is upstream/transient — it must stay retryable so an
    /// automated retry layer can recover, and it must never be reported as bad user config.
    #[tokio::test]
    async fn failed_secret_read_is_retryable() {
        let mut secrets = MockSecretManagerApi::new();
        secrets
            .expect_access_secret_version()
            .times(1)
            .returning(|_| {
                Err(AlienError::new(
                    alien_client_core::ErrorData::RemoteServiceUnavailable {
                        message: "secretmanager.googleapis.com is unavailable".to_string(),
                    },
                ))
            });

        let error = resolve("db", &binding(), Arc::new(secrets))
            .await
            .expect_err("a failed secret read must not resolve a connection");

        assert_eq!(error.code, "POSTGRES_SECRET_RESOLUTION_FAILED");
        assert!(error.retryable, "an upstream read failure is retryable");
        assert!(
            error.to_string().contains(SECRET_NAME),
            "the error must name the secret locator so operators can find it, got: {error}"
        );
    }

    /// An empty or missing payload must fail rather than silently connect with no
    /// password. An empty base64 payload decodes to zero bytes, which is easy to let
    /// through — assert both shapes.
    #[tokio::test]
    async fn empty_payload_fails_resolution() {
        for stored in [None, Some("")] {
            let mut secrets = MockSecretManagerApi::new();
            secrets
                .expect_access_secret_version()
                .times(1)
                .returning(move |_| Ok(response(stored)));

            let error = resolve("db", &binding(), Arc::new(secrets))
                .await
                .expect_err("an empty payload must not resolve a connection");

            assert_eq!(error.code, "POSTGRES_SECRET_RESOLUTION_FAILED");
            assert!(error.retryable);
        }
    }

    /// A malformed binding is user-fixable configuration: it must not be retryable, and
    /// it must fail before any secret read is attempted.
    #[tokio::test]
    async fn unresolved_secret_name_is_non_retryable_config_error() {
        let mut secrets = MockSecretManagerApi::new();
        secrets.expect_access_secret_version().never();

        let mut malformed = binding();
        malformed.password_secret_name = BindingValue::Expression(serde_json::json!({
            "Fn::GetAtt": ["PgSecret", "Name"]
        }));

        let error = resolve("db", &malformed, Arc::new(secrets))
            .await
            .expect_err("an unresolved secret name must not resolve a connection");

        assert_eq!(error.code, "BINDING_CONFIG_INVALID");
        assert!(!error.retryable, "bad binding config is user-fixable");
        assert!(
            error.to_string().contains("ALIEN_DB_BINDING"),
            "the error must name the env var the user would edit, got: {error}"
        );
    }
}
