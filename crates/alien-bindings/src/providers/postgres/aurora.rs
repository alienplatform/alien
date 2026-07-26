//! AWS Aurora Serverless v2 Postgres provider.
//!
//! The binding carries the Secrets Manager **ARN** of the connection password, not the
//! password. This provider reads it with the workload's own identity (granted by the
//! `postgres/data-access` permission set) and builds the connection parameters.
//!
//! The password lives in Secrets Manager — not Parameter Store — so this is a different
//! client from the `vault` binding's `aws_parameter_store` provider.

use crate::error::{ErrorData, Result};
use crate::providers::postgres::{resolve_params, resolve_secret_locator};
use crate::traits::{PostgresConnectionParams, SslMode};
use alien_aws_clients::secrets_manager::{GetSecretValueRequest, SecretsManagerApi};
use alien_core::bindings::AuroraPostgresBinding;
use alien_error::{AlienError, Context};
use std::sync::Arc;

/// Reads the password from Secrets Manager and resolves the connection parameters.
///
/// Aurora is dialed at the cluster **writer endpoint**, so `clusterEndpoint` — not a
/// `host` field — becomes the connection host. TLS is required (`sslmode=require`).
///
/// Performs exactly one `GetSecretValue`; a failure is returned to the caller, which
/// owns any retry policy.
pub(crate) async fn resolve(
    binding_name: &str,
    binding: &AuroraPostgresBinding,
    secrets: Arc<dyn SecretsManagerApi>,
) -> Result<PostgresConnectionParams> {
    let secret_arn = resolve_secret_locator(
        binding_name,
        "passwordSecretArn",
        &binding.password_secret_arn,
    )?;
    let password = read_password(binding_name, &secret_arn, secrets.as_ref()).await?;

    resolve_params(
        binding_name,
        &binding.cluster_endpoint,
        &binding.port,
        &binding.database,
        &binding.username,
        &password,
        SslMode::Require,
    )
}

/// Reads the raw password the AWS controller stored as the secret's `SecretString`.
async fn read_password(
    binding_name: &str,
    secret_arn: &str,
    secrets: &dyn SecretsManagerApi,
) -> Result<String> {
    let failed = |reason: &str| ErrorData::PostgresSecretResolutionFailed {
        binding_name: binding_name.to_string(),
        secret: secret_arn.to_string(),
        reason: reason.to_string(),
    };

    let response = secrets
        .get_secret_value(
            GetSecretValueRequest::builder()
                .secret_id(secret_arn.to_string())
                .build(),
        )
        .await
        .context(failed("Secrets Manager GetSecretValue failed"))?;

    // An absent or empty `SecretString` is a control-plane invariant the workload cannot
    // fix locally, so it reports the same (retryable) resolution failure as a failed read
    // rather than connecting with an empty password.
    response
        .secret_string
        .filter(|password| !password.is_empty())
        .ok_or_else(|| AlienError::new(failed("secret has no SecretString value")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::secrets_manager::{GetSecretValueResponse, MockSecretsManagerApi};
    use alien_core::bindings::BindingValue;

    const SECRET_ARN: &str = "arn:aws:secretsmanager:us-east-1:000000000000:secret:pg-AbCdEf";

    fn binding() -> AuroraPostgresBinding {
        AuroraPostgresBinding {
            cluster_endpoint: "cluster.cluster-abc.us-east-1.rds.amazonaws.com".into(),
            port: BindingValue::value(5432),
            database: "app".into(),
            username: "alien".into(),
            password_secret_arn: SECRET_ARN.into(),
        }
    }

    fn response(secret_string: Option<&str>) -> GetSecretValueResponse {
        GetSecretValueResponse {
            arn: Some(SECRET_ARN.to_string()),
            name: Some("pg".to_string()),
            version_id: None,
            secret_binary: None,
            secret_string: secret_string.map(str::to_string),
            version_stages: None,
            created_date: None,
        }
    }

    /// The happy path: the ARN is read verbatim, the *cluster endpoint* becomes the host,
    /// TLS is required, and the password is percent-encoded into the URL. The password
    /// deliberately contains every RFC 3986 sub-delim that `encodeURIComponent` leaves
    /// literal, extending the encoding contract pinned in `local.rs` to this backend.
    #[tokio::test]
    async fn resolves_cluster_endpoint_and_require_sslmode() {
        let mut secrets = MockSecretsManagerApi::new();
        secrets
            .expect_get_secret_value()
            .times(1)
            .withf(|request| {
                request.secret_id == SECRET_ARN
                    && request.version_id.is_none()
                    && request.version_stage.is_none()
            })
            .returning(|_| Ok(response(Some("a!b*c'd(e)f@/"))));

        let params = resolve("db", &binding(), Arc::new(secrets))
            .await
            .expect("aurora binding resolves");

        assert_eq!(
            params.host, "cluster.cluster-abc.us-east-1.rds.amazonaws.com",
            "Aurora dials the cluster writer endpoint, not a host field"
        );
        assert_eq!(params.port, 5432);
        assert_eq!(params.database, "app");
        assert_eq!(params.username, "alien");
        assert_eq!(params.password, "a!b*c'd(e)f@/");
        assert_eq!(params.sslmode, SslMode::Require);
        assert_eq!(
            params.connection_string(),
            "postgres://alien:a%21b%2Ac%27d%28e%29f%40%2F@\
             cluster.cluster-abc.us-east-1.rds.amazonaws.com:5432/app?sslmode=require"
        );
    }

    /// A failed secret read is upstream/transient — it must stay retryable so an
    /// automated retry layer can recover, and it must never be reported as bad user config.
    #[tokio::test]
    async fn failed_secret_read_is_retryable() {
        let mut secrets = MockSecretsManagerApi::new();
        secrets.expect_get_secret_value().times(1).returning(|_| {
            Err(AlienError::new(
                alien_client_core::ErrorData::RateLimitExceeded {
                    message: "GetSecretValue throttled".to_string(),
                },
            ))
        });

        let error = resolve("db", &binding(), Arc::new(secrets))
            .await
            .expect_err("a failed secret read must not resolve a connection");

        assert_eq!(error.code, "POSTGRES_SECRET_RESOLUTION_FAILED");
        assert!(error.retryable, "an upstream read failure is retryable");
        assert!(
            error.to_string().contains(SECRET_ARN),
            "the error must name the secret locator so operators can find it, got: {error}"
        );
    }

    /// An empty stored secret must fail rather than silently connect with no password.
    #[tokio::test]
    async fn empty_secret_string_fails_resolution() {
        for stored in [None, Some("")] {
            let mut secrets = MockSecretsManagerApi::new();
            secrets
                .expect_get_secret_value()
                .times(1)
                .returning(move |_| Ok(response(stored)));

            let error = resolve("db", &binding(), Arc::new(secrets))
                .await
                .expect_err("an empty secret must not resolve a connection");

            assert_eq!(error.code, "POSTGRES_SECRET_RESOLUTION_FAILED");
            assert!(error.retryable);
        }
    }

    /// A malformed binding is user-fixable configuration: it must not be retryable, and
    /// it must fail before any secret read is attempted.
    #[tokio::test]
    async fn unresolved_secret_arn_is_non_retryable_config_error() {
        let mut secrets = MockSecretsManagerApi::new();
        secrets.expect_get_secret_value().never();

        let mut malformed = binding();
        malformed.password_secret_arn = BindingValue::Expression(serde_json::json!({
            "Fn::GetAtt": ["PgSecret", "Id"]
        }));

        let error = resolve("db", &malformed, Arc::new(secrets))
            .await
            .expect_err("an unresolved secret ARN must not resolve a connection");

        assert_eq!(error.code, "BINDING_CONFIG_INVALID");
        assert!(!error.retryable, "bad binding config is user-fixable");
        assert!(
            error.to_string().contains("ALIEN_DB_BINDING"),
            "the error must name the env var the user would edit, got: {error}"
        );
    }
}
