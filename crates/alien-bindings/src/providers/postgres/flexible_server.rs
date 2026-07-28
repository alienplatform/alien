//! Azure Database for PostgreSQL — Flexible Server provider.
//!
//! The binding carries the Key Vault secret **URI** of the connection password, not the
//! password. This provider reads it with the workload's own identity (granted by the
//! `postgres/data-access` permission set) and builds the connection parameters.

use crate::error::{ErrorData, Result};
use crate::providers::postgres::{
    azure_postgres_tls_policy, cloud::resolve_secret_locator, resolve_params,
    PostgresConnectionInput,
};
use crate::traits::PostgresConnectionParams;
use alien_azure_clients::keyvault::KeyVaultSecretsApi;
use alien_core::bindings::FlexibleServerPostgresBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use std::sync::Arc;
use url::Url;

const PUBLIC_KEY_VAULT_DNS_SUFFIX: &str = ".vault.azure.net";

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
/// server's Private Endpoint) and verifies the Azure roots and hostname
/// (`sslmode=verify-full`).
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
        PostgresConnectionInput {
            host: &binding.host,
            port: &binding.port,
            database: &binding.database,
            username: &binding.username,
            password: &password,
            tls: azure_postgres_tls_policy(),
        },
    )
}

/// Splits a Key Vault secret URI into the vault URL, secret name, and optional version.
///
/// The URI must identify a public-cloud Azure Key Vault over HTTPS, and the path must be
/// exactly `/secrets/<name>[/<version>]`; a missing version means "latest". Rejecting
/// every other origin prevents a binding-controlled URI from receiving a Key Vault token.
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

    if url.scheme() != "https" {
        return Err(AlienError::new(malformed("must use HTTPS")));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AlienError::new(malformed(
            "must not contain user information",
        )));
    }
    if url.port().is_some() {
        return Err(AlienError::new(malformed("must not specify a port")));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AlienError::new(malformed(
            "must not contain a query or fragment",
        )));
    }

    let host = url.host_str().ok_or_else(|| {
        AlienError::new(malformed(
            "must use a public Azure Key Vault host ending in '.vault.azure.net'",
        ))
    })?;
    let vault_name = host
        .strip_suffix(PUBLIC_KEY_VAULT_DNS_SUFFIX)
        .filter(|name| valid_vault_name(name))
        .ok_or_else(|| {
            AlienError::new(malformed(
                "must use a public Azure Key Vault host ending in '.vault.azure.net'",
            ))
        })?;

    let segments: Vec<&str> = url
        .path_segments()
        .map(Iterator::collect)
        .unwrap_or_default();

    if segments.first() != Some(&"secrets")
        || !(2..=3).contains(&segments.len())
        || !segments[1..].iter().all(|segment| valid_secret_id(segment))
    {
        return Err(AlienError::new(malformed(
            "is not a '/secrets/<name>[/<version>]' Key Vault URL",
        )));
    }

    Ok(SecretUri {
        vault_base_url: format!("https://{vault_name}{PUBLIC_KEY_VAULT_DNS_SUFFIX}"),
        name: segments[1].to_string(),
        version: segments.get(2).map(|version| (*version).to_string()),
    })
}

/// Public Azure Key Vault names are a single 3–24 character DNS label.
fn valid_vault_name(name: &str) -> bool {
    (3..=24).contains(&name.len())
        && name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && name
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && !name.contains("--")
}

/// Key Vault secret names and version identifiers are path-safe ASCII identifiers.
fn valid_secret_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// Reads the raw password the Azure controller stored as a Key Vault secret.
async fn read_password(
    binding_name: &str,
    secret_uri: &str,
    parsed: &SecretUri,
    secrets: &dyn KeyVaultSecretsApi,
) -> Result<String> {
    let read_failed = |reason: &str| ErrorData::PostgresSecretResolutionFailed {
        binding_name: binding_name.to_string(),
        secret: secret_uri.to_string(),
        reason: reason.to_string(),
    };
    let invalid_value = |reason: &str| ErrorData::PostgresSecretValueInvalid {
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
        .context(read_failed("Key Vault getSecret failed"))?;

    // Retrying the same secret version cannot turn a missing value into a password.
    bundle
        .value
        .filter(|password| !password.is_empty())
        .ok_or_else(|| AlienError::new(invalid_value("secret value is missing or empty")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::SslMode;
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
    async fn resolves_host_with_verified_tls_from_secret_uri() {
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
        assert_eq!(params.sslmode(), SslMode::VerifyFull);
        assert_eq!(params.ca_certificates().len(), 1);
        assert_eq!(
            params.ca_certificates()[0]
                .matches("-----BEGIN CERTIFICATE-----")
                .count(),
            3,
            "the embedded set must carry only Azure's recommended roots"
        );
        assert_eq!(
            params.connection_string(),
            "postgres://alien:a%21b%2Ac%27d%28e%29f%40%2F@\
             pg.privatelink.postgres.database.azure.com:5432/app?sslmode=verify-full"
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

            assert_eq!(error.code, "POSTGRES_SECRET_VALUE_INVALID");
            assert!(!error.retryable);
            assert!(!error.internal);
        }
    }

    /// A URI this provider cannot interpret is user-fixable configuration: not retryable,
    /// and it must fail before any secret read is attempted. Covers every rejected shape.
    #[tokio::test]
    async fn malformed_secret_uri_is_non_retryable_config_error() {
        let cases = [
            ("not-a-url", "unparseable"),
            (
                "http://alien-vault.vault.azure.net/secrets/pg-password",
                "insecure scheme",
            ),
            ("https://example.com/secrets/pg-password", "arbitrary host"),
            (
                "https://alien-vault.vault.azure.net.example.com/secrets/pg-password",
                "suffix lookalike",
            ),
            (
                "https://nested.alien-vault.vault.azure.net/secrets/pg-password",
                "nested subdomain",
            ),
            (
                "https://user:password@alien-vault.vault.azure.net/secrets/pg-password",
                "user information",
            ),
            (
                "https://alien-vault.vault.azure.net:8443/secrets/pg-password",
                "custom port",
            ),
            (
                "https://alien-vault.vault.azure.net/secrets/pg-password?api-version=7.4",
                "query",
            ),
            (
                "https://alien-vault.vault.azure.net/secrets/pg-password#fragment",
                "fragment",
            ),
            (
                "https://alien-vault.vault.azure.net/keys/pg-password",
                "wrong collection",
            ),
            ("https://alien-vault.vault.azure.net/secrets", "no name"),
            (
                "https://alien-vault.vault.azure.net/secrets/pg-password/v1/extra",
                "trailing segments",
            ),
            (
                "https://alien-vault.vault.azure.net/secrets/pg%2Fpassword",
                "encoded path separator",
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
