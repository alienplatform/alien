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

#[cfg(feature = "aws")]
pub(crate) mod aurora;
#[cfg(feature = "gcp")]
pub(crate) mod cloud_sql;
#[cfg(feature = "azure")]
pub(crate) mod flexible_server;
pub mod local;

use crate::error::{ErrorData, Result};
use crate::traits::{PostgresConnectionParams, SslMode};
use alien_core::bindings::BindingValue;
use alien_error::Context;

/// A resolved cloud Postgres binding (Aurora, Cloud SQL, or Flexible Server).
///
/// The three backends differ only in *how* they reach their password — which secret
/// store holds it and how its payload is decoded — never in what they hand back. Each
/// module therefore exposes a `resolve` function returning the connection parameters,
/// and they all share this one handle.
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
#[derive(Debug)]
pub(crate) struct CloudPostgres {
    params: PostgresConnectionParams,
}

#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
impl CloudPostgres {
    pub(crate) fn new(params: PostgresConnectionParams) -> Self {
        Self { params }
    }
}

#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
impl crate::traits::Binding for CloudPostgres {}

#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
impl crate::traits::Postgres for CloudPostgres {
    fn connection_params(&self) -> PostgresConnectionParams {
        self.params.clone()
    }
}

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
) -> Result<PostgresConnectionParams> {
    let invalid = |field: &str| ErrorData::BindingConfigInvalid {
        env_var: crate::error::binding_env_var(binding_name),
        binding_name: binding_name.to_string(),
        reason: format!("Failed to extract '{}' from Postgres binding", field),
    };
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
    })
}

/// Extracts a cloud variant's secret locator (ARN / name / URI) as a concrete string.
///
/// `field` is the camelCase binding field name, so the error names the key the user
/// would actually look for in `ALIEN_<NAME>_BINDING`.
///
/// Gated on the cloud features because only the three cloud providers call it; a
/// local-only build has no secret locator to resolve.
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
pub(crate) fn resolve_secret_locator(
    binding_name: &str,
    field: &str,
    locator: &BindingValue<String>,
) -> Result<String> {
    locator
        .clone()
        .into_value(binding_name, field)
        .context(ErrorData::config_invalid(
            binding_name,
            format!("Failed to extract '{field}' from Postgres binding"),
        ))
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
            SslMode::Require,
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
            SslMode::Require,
        )
        .expect("concrete fields resolve");

        let rendered = format!("{params:?}");
        assert!(
            !rendered.contains("super-secret-password"),
            "password leaked into Debug output: {rendered}"
        );
        assert!(rendered.contains("<redacted>"), "got: {rendered}");
    }
}
