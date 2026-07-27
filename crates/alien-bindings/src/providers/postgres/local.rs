use crate::error::{ErrorData, Result};
use crate::providers::postgres::resolve_params;
use crate::traits::{Binding, Postgres, PostgresConnectionParams, SslMode};
use alien_core::bindings::{ExternalPostgresSslMode, PostgresBinding};
use alien_error::AlienError;

/// A resolved Postgres binding. Holds connection details only — it never opens or
/// owns a server process.
#[derive(Debug)]
pub struct LocalPostgres {
    params: PostgresConnectionParams,
}

impl LocalPostgres {
    pub fn new(params: PostgresConnectionParams) -> Self {
        Self { params }
    }

    /// Resolves connection parameters from a binding that carries its password inline:
    /// the Local (developer) and External (BYO / Kubernetes) variants.
    ///
    /// The cloud variants carry a secret *locator* instead and need an async read against
    /// that cloud's secret store, so they have their own providers (`aurora`, `cloud_sql`,
    /// `flexible_server`) that `BindingsProvider::load_postgres` dispatches to.
    pub fn from_binding(binding_name: &str, binding: &PostgresBinding) -> Result<Self> {
        let params = match binding {
            PostgresBinding::Local(b) => resolve_params(
                binding_name,
                &b.host,
                &b.port,
                &b.database,
                &b.username,
                // Inline password is already a concrete `String` (the type forbids an
                // unresolved ref).
                &b.password,
                SslMode::Disable,
                Vec::new(),
            )?,
            PostgresBinding::External(b) => {
                let sslmode = match b.ssl_mode {
                    ExternalPostgresSslMode::VerifyFull => SslMode::VerifyFull,
                    ExternalPostgresSslMode::Disable => SslMode::Disable,
                };
                resolve_params(
                    binding_name,
                    &b.host,
                    &b.port,
                    &b.database,
                    &b.username,
                    &b.password,
                    sslmode,
                    Vec::new(),
                )?
            }
            // Listed explicitly rather than via a catch-all so a future `PostgresBinding`
            // variant forces a compile error to route it somewhere. Reaching this arm means
            // `load_postgres` dispatched a cloud binding to the wrong provider — a bug here,
            // not bad user configuration.
            PostgresBinding::Aurora(_)
            | PostgresBinding::CloudSql(_)
            | PostgresBinding::FlexibleServer(_) => {
                return Err(AlienError::new(ErrorData::config_invalid(
                    binding_name,
                    "Cloud Postgres bindings carry a password secret locator and must be \
                     resolved by their own cloud provider, not the inline-password provider",
                )));
            }
        };
        Ok(Self::new(params))
    }
}

impl Binding for LocalPostgres {}

impl Postgres for LocalPostgres {
    fn connection_params(&self) -> PostgresConnectionParams {
        self.params.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bindings::BindingValue;

    #[test]
    fn local_binding_resolves_to_disable_sslmode_connection_string() {
        let binding = PostgresBinding::local("127.0.0.1", 6543, "db", "alien", "p@ss/word");
        let pg = LocalPostgres::from_binding("db", &binding).expect("local binding resolves");
        let params = pg.connection_params();
        assert_eq!(params.host, "127.0.0.1");
        assert_eq!(params.port, 6543);
        assert_eq!(params.sslmode, SslMode::Disable);
        // password is percent-encoded; sslmode=disable for Local (plain TCP).
        assert_eq!(
            pg.connection_string(),
            "postgres://alien:p%40ss%2Fword@127.0.0.1:6543/db?sslmode=disable"
        );
    }

    #[test]
    fn external_binding_defaults_to_verify_full_sslmode() {
        let binding = PostgresBinding::external("db.internal", 5432, "app", "alien", "p@ss/word");
        let pg = LocalPostgres::from_binding("db", &binding).expect("external binding resolves");

        assert_eq!(pg.connection_params().sslmode, SslMode::VerifyFull);
        assert_eq!(
            pg.connection_string(),
            "postgres://alien:p%40ss%2Fword@db.internal:5432/app?sslmode=verify-full"
        );
    }

    #[test]
    fn external_binding_allows_explicit_plaintext_opt_out() {
        let binding: PostgresBinding = serde_json::from_value(serde_json::json!({
            "service": "external",
            "host": "db.internal",
            "port": 5432,
            "database": "app",
            "username": "alien",
            "password": "secret",
            "sslMode": "disable",
        }))
        .expect("external plaintext binding deserializes");
        let pg = LocalPostgres::from_binding("db", &binding).expect("external binding resolves");

        assert_eq!(pg.connection_params().sslmode, SslMode::Disable);
        assert_eq!(
            pg.connection_string(),
            "postgres://alien:secret@db.internal:5432/app?sslmode=disable"
        );
    }

    // The connection string must percent-encode the RFC 3986 sub-delims ! * ' ( ) that JS's
    // encodeURIComponent leaves literal, so any resolver that reimplements this (in any
    // language) produces byte-identical URLs for any generated password. This pins the
    // encoding contract; `crates/alien-bindings/src/traits.rs::encode_userinfo` is the
    // single implementation every backend shares.
    #[test]
    fn connection_string_percent_encodes_rfc3986_sub_delims() {
        let binding = PostgresBinding::local("h", 5432, "db", "alien", "a!b*c'd(e)f");
        let pg = LocalPostgres::from_binding("db", &binding).expect("local binding resolves");
        assert_eq!(
            pg.connection_string(),
            "postgres://alien:a%21b%2Ac%27d%28e%29f@h:5432/db?sslmode=disable"
        );
    }

    /// A cloud binding routed here is a dispatch bug, not user error, but it must still
    /// fail loudly rather than resolve without a password.
    #[test]
    fn cloud_binding_is_rejected_by_the_inline_password_provider() {
        let binding = PostgresBinding::Aurora(alien_core::bindings::AuroraPostgresBinding {
            cluster_endpoint: "cluster.rds.amazonaws.com".into(),
            port: BindingValue::value(5432),
            database: "db".into(),
            username: "alien".into(),
            password_secret_arn: "arn:aws:secretsmanager:...".into(),
        });

        let error = LocalPostgres::from_binding("db", &binding)
            .expect_err("cloud bindings must not resolve without their secret");

        assert_eq!(error.code, "BINDING_CONFIG_INVALID");
    }
}
