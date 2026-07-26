//! Azure Database for PostgreSQL — Flexible Server provider.
//!
//! The binding carries the Key Vault secret **URI** of the connection password, not the
//! password. This provider reads it with the workload's own identity (granted by the
//! `postgres/data-access` permission set) and builds the connection parameters.

use crate::error::{ErrorData, Result};
use crate::providers::postgres::{cloud::resolve_secret_locator, resolve_params};
use crate::traits::{PostgresConnectionParams, SslMode};
use alien_azure_clients::keyvault::KeyVaultSecretsApi;
use alien_core::bindings::FlexibleServerPostgresBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use std::sync::Arc;
use url::Url;

/// The parts of a Key Vault secret URI needed to read it.
#[derive(Debug, PartialEq, Eq)]
struct SecretUri {
    /// Vault base URL, e.g. `https://my-vault.vault.azure.net`.
    vault_base_url: String,
    /// Secret name.
    name: String,
    /// Explicit version, or `None` for the latest.
    version: Option<String>,
}

/// Reads the password from Key Vault and resolves the connection parameters.
///
/// The workload dials the binding's `host` (the private DNS FQDN fronting the
/// server's Private Endpoint) and TLS is required (`sslmode=require`).
///
/// Performs exactly one `getSecret`; a failure is returned to the caller, which owns
/// any retry policy.
pub(crate) async fn resolve(
    binding_name: &str,
    binding: &FlexibleServerPostgresBinding,
    secrets: Arc<dyn KeyVaultSecretsApi>,
) -> Result<PostgresConnectionParams> {
    let secret_uri = resolve_secret_locator(
        binding_name,
        "passwordSecretUri",
        &binding.password_secret_uri,
    )?;
    let parsed = parse_secret_uri(binding_name, &secret_uri)?;
    let password = read_password(binding_name, &secret_uri, &parsed, secrets.as_ref()).await?;

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

/// Splits a Key Vault secret URI into the vault URL, secret name, and optional version.
///
/// The path is exactly `/secrets/<name>[/<version>]`; a missing version means "latest".
/// Anything else is rejected rather than silently ignored — a URI this provider cannot
/// interpret is a configuration problem the user can fix, so it is *not* retryable.
fn parse_secret_uri(binding_name: &str, secret_uri: &str) -> Result<SecretUri> {
    let malformed = |reason: &str| {
        ErrorData::config_invalid(
            binding_name,
            format!("Postgres password secret URI '{secret_uri}' {reason}"),
        )
    };

    let url = Url::parse(secret_uri)
        .into_alien_error()
        .context(malformed("is not a valid URL"))?;

    let segments: Vec<&str> = url
        .path_segments()
        .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
        .unwrap_or_default();

    if segments.first() != Some(&"secrets") || segments.len() < 2 || segments.len() > 3 {
        return Err(AlienError::new(malformed(
            "is not a '/secrets/<name>[/<version>]' Key Vault URL",
        )));
    }

    Ok(SecretUri {
        // `origin().ascii_serialization()` is scheme://host[:port] — the vault base URL
        // the Key Vault client expects, with the secret path stripped.
        vault_base_url: url.origin().ascii_serialization(),
        name: segments[1].to_string(),
        version: segments.get(2).map(|version| (*version).to_string()),
    })
}

/// Reads the raw password the Azure controller stored as a Key Vault secret.
async fn read_password(
    binding_name: &str,
    secret_uri: &str,
    parsed: &SecretUri,
    secrets: &dyn KeyVaultSecretsApi,
) -> Result<String> {
    let failed = |reason: &str| ErrorData::PostgresSecretResolutionFailed {
        binding_name: binding_name.to_string(),
        secret: secret_uri.to_string(),
        reason: reason.to_string(),
    };

    let bundle = secrets
        .get_secret(
            parsed.vault_base_url.clone(),
            parsed.name.clone(),
            parsed.version.clone(),
        )
        .await
        .context(failed("Key Vault getSecret failed"))?;

    // An absent or empty value is a control-plane invariant the workload cannot fix
    // locally, so it reports the same (retryable) resolution failure as a failed read
    // rather than connecting with an empty password.
    bundle
        .value
        .filter(|password| !password.is_empty())
        .ok_or_else(|| AlienError::new(failed("secret has no value")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_azure_clients::keyvault::MockKeyVaultSecretsApi;
    use alien_azure_clients::models::secrets::SecretBundle;
    use alien_core::bindings::BindingValue;

    const VAULT: &str = "https://alien-vault.vault.azure.net";
    const SECRET_URI: &str = "https://alien-vault.vault.azure.net/secrets/pg-password";

    fn binding(secret_uri: &str) -> FlexibleServerPostgresBinding {
        FlexibleServerPostgresBinding {
            host: "pg.privatelink.postgres.database.azure.com".into(),
            port: BindingValue::value(5432),
            database: "app".into(),
            username: "alien".into(),
            password_secret_uri: secret_uri.into(),
        }
    }

    fn bundle(value: Option<&str>) -> SecretBundle {
        SecretBundle {
            value: value.map(str::to_string),
            ..SecretBundle::default()
        }
    }

    /// The happy path: the URI splits into vault + name (no version, i.e. latest), the
    /// binding's host is dialed, and TLS is required. The password contains every RFC 3986
    /// sub-delim that `encodeURIComponent` leaves literal, extending the encoding contract
    /// pinned in `local.rs` to this backend.
    #[tokio::test]
    async fn resolves_host_and_require_sslmode_from_secret_uri() {
        let mut secrets = MockKeyVaultSecretsApi::new();
        secrets
            .expect_get_secret()
            .times(1)
            .withf(|vault, name, version| {
                vault == VAULT && name == "pg-password" && version.is_none()
            })
            .returning(|_, _, _| Ok(bundle(Some("a!b*c'd(e)f@/"))));

        let params = resolve("db", &binding(SECRET_URI), Arc::new(secrets))
            .await
            .expect("flexible server binding resolves");

        assert_eq!(params.host, "pg.privatelink.postgres.database.azure.com");
        assert_eq!(params.port, 5432);
        assert_eq!(params.database, "app");
        assert_eq!(params.username, "alien");
        assert_eq!(params.password, "a!b*c'd(e)f@/");
        assert_eq!(params.sslmode, SslMode::Require);
        assert_eq!(
            params.connection_string(),
            "postgres://alien:a%21b%2Ac%27d%28e%29f%40%2F@\
             pg.privatelink.postgres.database.azure.com:5432/app?sslmode=require"
        );
    }

    /// A URI that pins a version must read exactly that version, not "latest".
    #[tokio::test]
    async fn versioned_secret_uri_reads_that_version() {
        let mut secrets = MockKeyVaultSecretsApi::new();
        secrets
            .expect_get_secret()
            .times(1)
            .withf(|vault, name, version| {
                vault == VAULT && name == "pg-password" && version.as_deref() == Some("abc123")
            })
            .returning(|_, _, _| Ok(bundle(Some("pw"))));

        let params = resolve(
            "db",
            &binding(&format!("{SECRET_URI}/abc123")),
            Arc::new(secrets),
        )
        .await
        .expect("versioned secret URI resolves");

        assert_eq!(params.password, "pw");
    }

    /// A failed secret read is upstream/transient — it must stay retryable so an
    /// automated retry layer can recover, and it must never be reported as bad user config.
    #[tokio::test]
    async fn failed_secret_read_is_retryable() {
        let mut secrets = MockKeyVaultSecretsApi::new();
        secrets.expect_get_secret().times(1).returning(|_, _, _| {
            Err(AlienError::new(alien_client_core::ErrorData::Timeout {
                message: "Key Vault request timed out".to_string(),
            }))
        });

        let error = resolve("db", &binding(SECRET_URI), Arc::new(secrets))
            .await
            .expect_err("a failed secret read must not resolve a connection");

        assert_eq!(error.code, "POSTGRES_SECRET_RESOLUTION_FAILED");
        assert!(error.retryable, "an upstream read failure is retryable");
        assert!(
            error.to_string().contains(SECRET_URI),
            "the error must name the secret locator so operators can find it, got: {error}"
        );
    }

    /// An empty stored secret must fail rather than silently connect with no password.
    #[tokio::test]
    async fn empty_secret_value_fails_resolution() {
        for stored in [None, Some("")] {
            let mut secrets = MockKeyVaultSecretsApi::new();
            secrets
                .expect_get_secret()
                .times(1)
                .returning(move |_, _, _| Ok(bundle(stored)));

            let error = resolve("db", &binding(SECRET_URI), Arc::new(secrets))
                .await
                .expect_err("an empty secret must not resolve a connection");

            assert_eq!(error.code, "POSTGRES_SECRET_RESOLUTION_FAILED");
            assert!(error.retryable);
        }
    }

    /// A URI this provider cannot interpret is user-fixable configuration: not retryable,
    /// and it must fail before any secret read is attempted. Covers every rejected shape.
    #[tokio::test]
    async fn malformed_secret_uri_is_non_retryable_config_error() {
        let cases = [
            ("not-a-url", "unparseable"),
            (
                "https://alien-vault.vault.azure.net/keys/pg-password",
                "wrong collection",
            ),
            ("https://alien-vault.vault.azure.net/secrets", "no name"),
            (
                "https://alien-vault.vault.azure.net/secrets/pg-password/v1/extra",
                "trailing segments",
            ),
        ];

        for (uri, why) in cases {
            let mut secrets = MockKeyVaultSecretsApi::new();
            secrets.expect_get_secret().never();

            let error = match resolve("db", &binding(uri), Arc::new(secrets)).await {
                Ok(_) => panic!("'{uri}' ({why}) must be rejected"),
                Err(error) => error,
            };

            assert_eq!(error.code, "BINDING_CONFIG_INVALID", "for '{uri}' ({why})");
            assert!(!error.retryable, "for '{uri}' ({why})");
        }
    }

    /// An unresolved binding value (a template expression that never got substituted) is
    /// also user-fixable configuration and must not reach the secret store.
    #[tokio::test]
    async fn unresolved_secret_uri_is_non_retryable_config_error() {
        let mut secrets = MockKeyVaultSecretsApi::new();
        secrets.expect_get_secret().never();

        let mut malformed = binding(SECRET_URI);
        malformed.password_secret_uri = BindingValue::Expression(serde_json::json!({
            "Fn::GetAtt": ["PgSecret", "Uri"]
        }));

        let error = resolve("db", &malformed, Arc::new(secrets))
            .await
            .expect_err("an unresolved secret URI must not resolve a connection");

        assert_eq!(error.code, "BINDING_CONFIG_INVALID");
        assert!(!error.retryable, "bad binding config is user-fixable");
        assert!(
            error.to_string().contains("ALIEN_DB_BINDING"),
            "the error must name the env var the user would edit, got: {error}"
        );
    }
}
