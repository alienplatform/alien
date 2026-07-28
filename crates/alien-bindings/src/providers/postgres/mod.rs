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
//! pool; [`runtime::PostgresRuntime`] owns both policies.)

#[cfg(feature = "aws")]
pub(crate) mod aurora;
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
pub(crate) mod cloud;
#[cfg(feature = "gcp")]
pub(crate) mod cloud_sql;
#[cfg(feature = "azure")]
pub(crate) mod flexible_server;
pub mod local;
pub(crate) mod runtime;

use crate::error::{ErrorData, Result};
#[cfg(any(feature = "gcp", test))]
use crate::traits::SslMode;
use crate::traits::{Binding, Postgres, PostgresConnectionParams, PostgresTlsPolicy};
use alien_core::bindings::BindingValue;
#[cfg(feature = "gcp")]
use alien_error::AlienError;
use alien_error::Context;
#[cfg(any(feature = "aws", feature = "azure"))]
use std::sync::OnceLock;

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

/// A Postgres handle whose cloud-specific work has already been completed.
///
/// Local, external, and cloud bindings differ only while resolving their connection
/// details. They all expose the same immutable handle afterwards.
#[derive(Debug)]
pub struct ResolvedPostgres {
    params: PostgresConnectionParams,
}

impl ResolvedPostgres {
    pub fn new(params: PostgresConnectionParams) -> Self {
        Self { params }
    }
}

impl Binding for ResolvedPostgres {}

impl Postgres for ResolvedPostgres {
    fn connection_params(&self) -> &PostgresConnectionParams {
        &self.params
    }
}

/// Concrete inputs shared by every Postgres backend after password and TLS resolution.
pub(crate) struct PostgresConnectionInput<'a> {
    pub(crate) host: &'a BindingValue<String>,
    pub(crate) port: &'a BindingValue<u16>,
    pub(crate) database: &'a BindingValue<String>,
    pub(crate) username: &'a BindingValue<String>,
    pub(crate) password: &'a str,
    pub(crate) tls: PostgresTlsPolicy,
}

/// Combines a binding's concrete connection fields with an already-resolved `password`.
///
/// Every field arrives as a [`BindingValue`], so an unresolved template expression or
/// `SecretRef` is a user-fixable configuration problem (`BINDING_CONFIG_INVALID`, not
/// retryable) rather than a runtime failure.
///
/// `host` is whichever field the backend dials: the cluster endpoint for Aurora, the
/// host for every other backend.
pub(crate) fn resolve_params(
    binding_name: &str,
    input: PostgresConnectionInput<'_>,
) -> Result<PostgresConnectionParams> {
    let invalid = |field: &str| ErrorData::BindingConfigInvalid {
        env_var: crate::error::binding_env_var(binding_name),
        binding_name: binding_name.to_string(),
        reason: format!("Failed to extract '{}' from Postgres binding", field),
    };

    Ok(PostgresConnectionParams::new(
        input
            .host
            .clone()
            .into_value(binding_name, "host")
            .context(invalid("host"))?,
        input
            .port
            .clone()
            .into_value(binding_name, "port")
            .context(invalid("port"))?,
        input
            .database
            .clone()
            .into_value(binding_name, "database")
            .context(invalid("database"))?,
        input
            .username
            .clone()
            .into_value(binding_name, "username")
            .context(invalid("username"))?,
        input.password.to_string(),
        input.tls,
    ))
}

/// Resolves and validates the per-instance CA roots carried by a Cloud SQL binding.
#[cfg(feature = "gcp")]
pub(crate) fn resolve_verify_ca_policy(
    binding_name: &str,
    certificates: &BindingValue<Vec<String>>,
) -> Result<PostgresTlsPolicy> {
    let certificates = certificates
        .clone()
        .into_value(binding_name, "serverCaCertificates")
        .context(ErrorData::config_invalid(
            binding_name,
            "Failed to extract 'serverCaCertificates' from Postgres binding",
        ))?;
    verified_tls_policy(
        binding_name,
        SslMode::VerifyCa,
        PostgresTlsPolicy::verify_ca(certificates),
    )
}

#[cfg(feature = "gcp")]
fn verified_tls_policy(
    binding_name: &str,
    sslmode: SslMode,
    policy: std::result::Result<PostgresTlsPolicy, crate::traits::InvalidPostgresCaCertificates>,
) -> Result<PostgresTlsPolicy> {
    policy.map_err(|_| {
        AlienError::new(ErrorData::config_invalid(
            binding_name,
            format!(
                "Postgres sslmode '{}' requires one or more non-empty PEM server CA certificates",
                sslmode.as_str()
            ),
        ))
    })
}

/// Cached Aurora TLS policy. Its large embedded root bundle is parsed and allocated once,
/// then shared by all resolved handles.
#[cfg(feature = "aws")]
pub(crate) fn aws_rds_tls_policy() -> PostgresTlsPolicy {
    static POLICY: OnceLock<PostgresTlsPolicy> = OnceLock::new();
    POLICY
        .get_or_init(|| {
            PostgresTlsPolicy::verify_full(
                AWS_RDS_CA_CERTIFICATES
                    .iter()
                    .map(|certificate| (*certificate).to_string())
                    .collect(),
            )
            .expect("the embedded AWS RDS root bundle must contain valid PEM certificates")
        })
        .clone()
}

/// Cached Flexible Server TLS policy. Embedded roots are parsed and allocated once,
/// then shared by all resolved handles.
#[cfg(feature = "azure")]
pub(crate) fn azure_postgres_tls_policy() -> PostgresTlsPolicy {
    static POLICY: OnceLock<PostgresTlsPolicy> = OnceLock::new();
    POLICY
        .get_or_init(|| {
            PostgresTlsPolicy::verify_full(
                AZURE_POSTGRES_CA_CERTIFICATES
                    .iter()
                    .map(|certificate| (*certificate).to_string())
                    .collect(),
            )
            .expect("the embedded Azure Postgres roots must contain valid PEM certificates")
        })
        .clone()
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
            PostgresConnectionInput {
                host: &BindingValue::SecretRef {
                    secret_ref: alien_core::bindings::SecretReference {
                        name: "pg-credentials".to_string(),
                        key: "host".to_string(),
                    },
                },
                port: &BindingValue::value(5432),
                database: &"db".into(),
                username: &"alien".into(),
                password: "pw",
                tls: PostgresTlsPolicy::verify_full(vec![pem("root")]).unwrap(),
            },
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
            PostgresConnectionInput {
                host: &"h".into(),
                port: &BindingValue::value(5432),
                database: &"db".into(),
                username: &"alien".into(),
                password: "super-secret-password",
                tls: PostgresTlsPolicy::verify_full(vec![pem("root")]).unwrap(),
            },
        )
        .expect("concrete fields resolve");

        let rendered = format!("{params:?}");
        assert!(
            !rendered.contains("super-secret-password"),
            "password leaked into Debug output: {rendered}"
        );
        assert!(
            !rendered.contains("BEGIN CERTIFICATE"),
            "certificate bundle expanded into Debug output: {rendered}"
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
            let error = verified_tls_policy(
                "db",
                SslMode::VerifyCa,
                PostgresTlsPolicy::verify_ca(ca_certificates),
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
            PostgresConnectionInput {
                host: &"db.example.com".into(),
                port: &BindingValue::value(5432),
                database: &"db".into(),
                username: &"alien".into(),
                password: "pw",
                tls: PostgresTlsPolicy::verify_full_with_system_roots(),
            },
        )
        .expect("BYO verify-full can rely on the runtime trust store");

        assert!(params.ca_certificates().is_empty());
        assert_eq!(params.sslmode(), SslMode::VerifyFull);
    }

    #[test]
    fn tls_policy_cannot_pair_plaintext_with_roots_or_verify_ca_without_roots() {
        assert!(PostgresTlsPolicy::verify_ca(Vec::new()).is_err());
        assert!(PostgresTlsPolicy::disabled().ca_certificates().is_empty());
        assert_eq!(PostgresTlsPolicy::disabled().sslmode(), SslMode::Disable);
    }

    fn pem(body: &str) -> String {
        format!("-----BEGIN CERTIFICATE-----\n{body}\n-----END CERTIFICATE-----")
    }
}
