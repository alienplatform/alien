//! GCP Cloud SQL Postgres provider.
//!
//! The binding carries the Secret Manager secret **name** of the connection password,
//! not the password. This provider reads the secret's latest version with the workload's
//! own identity (granted by the `postgres/data-access` permission set) and builds the
//! connection parameters.

use crate::error::{ErrorData, Result};
use crate::providers::postgres::{
    cloud::resolve_secret_locator, resolve_params, resolve_verify_ca_policy,
    PostgresConnectionInput,
};
use crate::traits::PostgresConnectionParams;
use alien_core::bindings::CloudSqlPostgresBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::secret_manager::SecretManagerApi;
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use std::sync::Arc;

/// Reads the password from Secret Manager and resolves the connection parameters.
///
/// The workload dials the binding's `host` (the Private Service Connect consumer
/// endpoint) and verifies its per-instance CA (`sslmode=verify-ca`). Hostname
/// verification is not possible because the PSC consumer endpoint is an IP address
/// that is not present in the server certificate.
///
/// Performs exactly one `accessSecretVersion`; a failure is returned to the caller,
/// which owns any retry policy.
pub(crate) async fn resolve(
    binding_name: &str,
    binding: &CloudSqlPostgresBinding,
    secrets: Arc<dyn SecretManagerApi>,
) -> Result<PostgresConnectionParams> {
    let tls = resolve_verify_ca_policy(binding_name, &binding.server_ca_certificates)?;
    let secret_name = resolve_secret_locator(
        binding_name,
        "passwordSecretName",
        &binding.password_secret_name,
    )?;
    let password = read_password(binding_name, &secret_name, secrets.as_ref()).await?;

    resolve_params(
        binding_name,
        PostgresConnectionInput {
            host: &binding.host,
            port: &binding.port,
            database: &binding.database,
            username: &binding.username,
            password: &password,
            tls,
        },
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
    let read_failed = |reason: &str| ErrorData::PostgresSecretResolutionFailed {
        binding_name: binding_name.to_string(),
        secret: secret_name.to_string(),
        reason: reason.to_string(),
    };
    let invalid_value = |reason: &str| ErrorData::PostgresSecretValueInvalid {
        binding_name: binding_name.to_string(),
        secret: secret_name.to_string(),
        reason: reason.to_string(),
    };

    let response = secrets
        .access_secret_version(format!("{secret_name}/versions/latest"))
        .await
        .context(read_failed("Secret Manager accessSecretVersion failed"))?;

    // Secret Manager returns the payload base64-encoded. Retrying the same version cannot
    // turn a missing, empty, or malformed payload into a valid password.
    let encoded = response
        .payload
        .and_then(|payload| payload.data)
        .ok_or_else(|| AlienError::new(invalid_value("secret version has no payload")))?;

    let decoded = base64_standard
        .decode(&encoded)
        .into_alien_error()
        .context(invalid_value("payload is not valid base64"))?;

    if decoded.is_empty() {
        return Err(AlienError::new(invalid_value(
            "secret version payload is empty",
        )));
    }

    String::from_utf8(decoded)
        .into_alien_error()
        .context(invalid_value("payload is not valid UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::SslMode;
    use alien_core::bindings::BindingValue;
    use alien_gcp_clients::secret_manager::{
        AccessSecretVersionResponse, MockSecretManagerApi, SecretPayload,
    };

    const SECRET_NAME: &str = "pg-credentials";
    const SERVER_CA: &str =
        "-----BEGIN CERTIFICATE-----\ncloud-sql-instance-root\n-----END CERTIFICATE-----";

    fn binding() -> CloudSqlPostgresBinding {
        CloudSqlPostgresBinding {
            host: "10.0.0.5".into(),
            port: BindingValue::value(5432),
            database: "app".into(),
            username: "alien".into(),
            server_ca_certificates: BindingValue::value(vec![SERVER_CA.to_string()]),
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
    async fn resolves_host_with_ca_verified_tls_from_latest_version() {
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
        assert_eq!(params.sslmode(), SslMode::VerifyCa);
        assert_eq!(params.ca_certificates(), &[SERVER_CA]);
        assert_eq!(
            params.connection_string(),
            "postgres://alien:a%21b%2Ac%27d%28e%29f%40%2F@10.0.0.5:5432/app?sslmode=verify-ca"
        );
    }

    #[tokio::test]
    async fn missing_server_ca_fails_before_reading_the_password() {
        for certificates in [
            BindingValue::value(Vec::new()),
            BindingValue::Expression(serde_json::json!({
                "Fn::GetAtt": ["CloudSql", "ServerCaCertificates"]
            })),
        ] {
            let mut secrets = MockSecretManagerApi::new();
            secrets.expect_access_secret_version().never();

            let mut malformed = binding();
            malformed.server_ca_certificates = certificates;

            let error = resolve("db", &malformed, Arc::new(secrets))
                .await
                .expect_err("Cloud SQL without concrete CA roots must fail closed");

            assert_eq!(error.code, "BINDING_CONFIG_INVALID");
            assert!(!error.retryable);
        }
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

            assert_eq!(error.code, "POSTGRES_SECRET_VALUE_INVALID");
            assert!(!error.retryable);
            assert!(!error.internal);
        }
    }

    /// Decode failures are permanent but retain their third-party source error.
    #[tokio::test]
    async fn malformed_payload_is_non_retryable_and_preserves_source() {
        let responses = [
            (
                "not valid base64".to_string(),
                "payload is not valid base64",
            ),
            (base64_standard.encode([0xff]), "payload is not valid UTF-8"),
        ];

        for (encoded, expected_reason) in responses {
            let mut secrets = MockSecretManagerApi::new();
            secrets
                .expect_access_secret_version()
                .times(1)
                .returning(move |_| {
                    Ok(AccessSecretVersionResponse {
                        name: Some(format!("projects/p/secrets/{SECRET_NAME}/versions/1")),
                        payload: Some(SecretPayload {
                            data: Some(encoded.clone()),
                        }),
                    })
                });

            let error = resolve("db", &binding(), Arc::new(secrets))
                .await
                .expect_err("a malformed payload must not resolve a connection");

            assert_eq!(error.code, "POSTGRES_SECRET_VALUE_INVALID");
            assert!(!error.retryable);
            assert!(!error.internal);
            assert!(
                error.to_string().contains(expected_reason),
                "error must identify the malformed payload: {error}"
            );
            assert!(
                std::error::Error::source(&error).is_some(),
                "the decode error must be retained as the source"
            );
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
