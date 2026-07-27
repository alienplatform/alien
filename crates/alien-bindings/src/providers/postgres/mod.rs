//! Postgres binding providers.
//!
//! Postgres is connection-only: the provider resolves connection details and the
//! application connects with its own driver. There is no gRPC service (by design).
//!
//! `Local` and `External` carry the password inline. The three cloud variants carry only
//! a *pointer* to the password in that cloud's secret store (Secrets Manager ARN /
//! Secret Manager name / Key Vault secret URI) — the password never flows through the
//! control plane and never sits in a plaintext environment variable. Each cloud provider
//! reads that pointer with the workload's own identity, which is exactly what the
//! `postgres/data-access` permission set grants.
//!
//! Resolution happens up front, when the binding is loaded, so
//! [`crate::traits::Postgres`] stays synchronous and one handle can be read repeatedly
//! without another secret read. A cloud handle therefore holds the password that was
//! current when it was created; `BindingsProvider::load_postgres` deliberately does not
//! cache it, so loading the binding again re-reads the secret and picks up a rotation.
//! (The secret-store *client* is cached, so re-reading does not rebuild a connection
//! pool — see `BindingsProvider::postgres_secret_client`.)

#[cfg(feature = "aws")]
pub(crate) mod aurora;
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
pub(crate) mod cloud;
#[cfg(feature = "gcp")]
pub(crate) mod cloud_sql;
#[cfg(feature = "azure")]
pub(crate) mod flexible_server;
pub mod local;

use crate::error::{ErrorData, Result};
use crate::traits::{PostgresConnectionParams, SslMode};
use alien_core::bindings::BindingValue;
use alien_error::{AlienError, Context};

/// Official Amazon RDS roots for every commercial region.
///
/// Source: <https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem>.
/// AWS uses region-specific roots for each supported CA algorithm, so the global
/// set is intentionally larger than a conventional public-root bundle.
#[cfg(feature = "aws")]
pub(crate) const AWS_RDS_CA_CERTIFICATES: &[&str] = &[include_str!("ca/aws-rds-global-roots.pem")];

/// Roots currently recommended by Azure Database for PostgreSQL.
///
/// Root rotation is handled by updating this embedded set and releasing the SDK.
/// Intermediate and server certificates must never be added.
#[cfg(feature = "azure")]
pub(crate) const AZURE_POSTGRES_CA_CERTIFICATES: &[&str] =
    &[include_str!("ca/azure-postgres-roots.pem")];

/// Combines a binding's concrete connection fields with an already-resolved `password`.
///
/// Every field arrives as a [`BindingValue`], so an unresolved template expression or
/// `SecretRef` is a user-fixable configuration problem (`BINDING_CONFIG_INVALID`, not
/// retryable) rather than a runtime failure.
///
/// `host` is whichever field the backend dials: the cluster endpoint for Aurora, the
/// host for every other backend.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_params(
    binding_name: &str,
    host: &BindingValue<String>,
    port: &BindingValue<u16>,
    database: &BindingValue<String>,
    username: &BindingValue<String>,
    password: &str,
    sslmode: SslMode,
    ca_certificates: Vec<String>,
) -> Result<PostgresConnectionParams> {
    let invalid = |field: &str| ErrorData::BindingConfigInvalid {
        env_var: crate::error::binding_env_var(binding_name),
        binding_name: binding_name.to_string(),
        reason: format!("Failed to extract '{}' from Postgres binding", field),
    };
    match sslmode {
        SslMode::VerifyCa => validate_ca_certificates(binding_name, &ca_certificates)?,
        SslMode::VerifyFull if !ca_certificates.is_empty() => {
            validate_ca_certificates(binding_name, &ca_certificates)?;
        }
        SslMode::Disable if !ca_certificates.is_empty() => {
            return Err(AlienError::new(ErrorData::config_invalid(
                binding_name,
                "Postgres CA certificates require sslmode 'verify-ca' or 'verify-full'",
            )));
        }
        SslMode::Disable | SslMode::VerifyFull => {}
    }

    Ok(PostgresConnectionParams {
        host: host
            .clone()
            .into_value(binding_name, "host")
            .context(invalid("host"))?,
        port: port
            .clone()
            .into_value(binding_name, "port")
            .context(invalid("port"))?,
        database: database
            .clone()
            .into_value(binding_name, "database")
            .context(invalid("database"))?,
        username: username
            .clone()
            .into_value(binding_name, "username")
            .context(invalid("username"))?,
        password: password.to_string(),
        sslmode,
        ca_certificates,
    })
}

/// Resolves the per-instance CA roots carried by a Cloud SQL binding.
pub(crate) fn resolve_ca_certificates(
    binding_name: &str,
    certificates: &BindingValue<Vec<String>>,
) -> Result<Vec<String>> {
    let certificates = certificates
        .clone()
        .into_value(binding_name, "serverCaCertificates")
        .context(ErrorData::config_invalid(
            binding_name,
            "Failed to extract 'serverCaCertificates' from Postgres binding",
        ))?;
    validate_ca_certificates(binding_name, &certificates)?;
    Ok(certificates)
}

fn validate_ca_certificates(binding_name: &str, ca_certificates: &[String]) -> Result<()> {
    if ca_certificates.is_empty() {
        return Err(AlienError::new(ErrorData::config_invalid(
            binding_name,
            "Postgres TLS verification requires at least one server CA certificate",
        )));
    }
    if ca_certificates.iter().any(|certificate| {
        let certificate = certificate.trim();
        !certificate.starts_with("-----BEGIN CERTIFICATE-----")
            || !certificate.ends_with("-----END CERTIFICATE-----")
    }) {
        return Err(AlienError::new(ErrorData::config_invalid(
            binding_name,
            "Postgres server CA certificates must be non-empty PEM certificates",
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unresolved `SecretRef` in a connection field must fail as user-fixable config,
    /// not silently produce a half-resolved connection.
    #[test]
    fn unresolved_secret_ref_field_is_binding_config_invalid() {
        let error = resolve_params(
            "db",
            &BindingValue::SecretRef {
                secret_ref: alien_core::bindings::SecretReference {
                    name: "pg-credentials".to_string(),
                    key: "host".to_string(),
                },
            },
            &BindingValue::value(5432),
            &"db".into(),
            &"alien".into(),
            "pw",
            SslMode::VerifyFull,
            vec!["-----BEGIN CERTIFICATE-----\nroot\n-----END CERTIFICATE-----".to_string()],
        )
        .expect_err("an unresolved SecretRef host must not resolve");

        assert_eq!(error.code, "BINDING_CONFIG_INVALID");
        assert!(!error.retryable, "bad binding config is user-fixable");
        assert!(
            error.to_string().contains("host"),
            "the error must name the offending field, got: {error}"
        );
    }

    /// The redacting `Debug` on `PostgresConnectionParams` is the only thing keeping a
    /// resolved cloud password out of logs and panic output; every handle derives its own
    /// `Debug` from it, so pin it here so a derive can never quietly replace it.
    #[test]
    fn debug_output_never_contains_the_password() {
        let params = resolve_params(
            "db",
            &"h".into(),
            &BindingValue::value(5432),
            &"db".into(),
            &"alien".into(),
            "super-secret-password",
            SslMode::VerifyFull,
            vec!["-----BEGIN CERTIFICATE-----\nroot\n-----END CERTIFICATE-----".to_string()],
        )
        .expect("concrete fields resolve");

        let rendered = format!("{params:?}");
        assert!(
            !rendered.contains("super-secret-password"),
            "password leaked into Debug output: {rendered}"
        );
        assert!(rendered.contains("<redacted>"), "got: {rendered}");
    }

    #[test]
    fn verify_ca_requires_valid_ca_certificates() {
        for ca_certificates in [
            Vec::new(),
            vec!["".to_string()],
            vec!["not a certificate".to_string()],
        ] {
            let error = resolve_params(
                "db",
                &"h".into(),
                &BindingValue::value(5432),
                &"db".into(),
                &"alien".into(),
                "pw",
                SslMode::VerifyCa,
                ca_certificates,
            )
            .expect_err("verified TLS without a PEM root must fail closed");

            assert_eq!(error.code, "BINDING_CONFIG_INVALID");
            assert!(!error.retryable);
        }
    }

    #[test]
    fn verify_full_can_use_the_system_trust_store() {
        let params = resolve_params(
            "db",
            &"db.example.com".into(),
            &BindingValue::value(5432),
            &"db".into(),
            &"alien".into(),
            "pw",
            SslMode::VerifyFull,
            Vec::new(),
        )
        .expect("BYO verify-full can rely on the runtime trust store");

        assert!(params.ca_certificates.is_empty());
        assert_eq!(params.sslmode, SslMode::VerifyFull);
    }
}
