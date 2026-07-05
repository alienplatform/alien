use crate::error::{binding_env_var, ErrorData, Result};
use crate::traits::{Binding, Postgres, PostgresConnectionParams, SslMode};
use alien_core::bindings::{BindingValue, PostgresBinding};
use alien_error::{AlienError, Context};

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

    /// Resolves connection parameters from a binding. Handles the Local and External (BYO)
    /// variants. Cloud variants (Aurora / Cloud SQL / Flexible Server) carry only a *reference* to
    /// the connection password in a cloud secret store; the workload SDK
    /// (`packages/sdk/src/bindings/postgres.ts`) resolves it in-process with the workload's own
    /// identity. This Rust provider intentionally does not read cloud secrets, so it rejects cloud
    /// bindings by design rather than half-resolving them.
    pub fn from_binding(binding_name: &str, binding: &PostgresBinding) -> Result<Self> {
        let params = match binding {
            PostgresBinding::Local(b) => resolve_params(
                binding_name,
                &b.host,
                &b.port,
                &b.database,
                &b.username,
                &b.password,
                SslMode::Disable,
            )?,
            PostgresBinding::External(b) => resolve_params(
                binding_name,
                &b.host,
                &b.port,
                &b.database,
                &b.username,
                &b.password,
                SslMode::Prefer,
            )?,
            // Cloud variants are resolved by the workload SDK (see the method doc), not here. Listed
            // explicitly rather than via a catch-all so a future `PostgresBinding` variant forces a
            // compile error to handle it. Name the backend so a reader of a later cloud plan can tell
            // which variant was rejected.
            PostgresBinding::Aurora(_)
            | PostgresBinding::CloudSql(_)
            | PostgresBinding::FlexibleServer(_) => {
                let backend = match binding {
                    PostgresBinding::Aurora(_) => "Aurora (AWS)",
                    PostgresBinding::CloudSql(_) => "Cloud SQL (GCP)",
                    PostgresBinding::FlexibleServer(_) => "Azure Flexible Server",
                    _ => "cloud",
                };
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    env_var: binding_env_var(binding_name),
                    binding_name: binding_name.to_string(),
                    reason: format!(
                        "{backend} Postgres bindings are resolved in-process by the workload SDK, \
                         not this Rust provider"
                    ),
                }));
            }
        };
        Ok(Self::new(params))
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_params(
    binding_name: &str,
    host: &BindingValue<String>,
    port: &BindingValue<u16>,
    database: &BindingValue<String>,
    username: &BindingValue<String>,
    password: &str,
    sslmode: SslMode,
) -> Result<PostgresConnectionParams> {
    let invalid = |field: &str| ErrorData::BindingConfigInvalid {
        env_var: binding_env_var(binding_name),
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
        // Inline password is already a concrete `String` (the type forbids an unresolved ref).
        password: password.to_string(),
        sslmode,
    })
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

    #[test]
    fn local_binding_resolves_to_disable_sslmode_connection_string() {
        let binding = PostgresBinding::local("127.0.0.1", 6543, "db", "alien", "p@ss/word");
        let pg = LocalPostgres::from_binding("db", &binding).expect("local binding resolves");
        let params = pg.connection_params();
        assert_eq!(params.host, "127.0.0.1");
        assert_eq!(params.port, 6543);
        // password is percent-encoded; sslmode=disable for Local (plain TCP).
        assert_eq!(
            pg.connection_string(),
            "postgres://alien:p%40ss%2Fword@127.0.0.1:6543/db?sslmode=disable"
        );
    }

    // The connection string must percent-encode the RFC 3986 sub-delims ! * ' ( ) that JS's
    // encodeURIComponent leaves literal, so the Rust resolver and the TS SDK resolver
    // (packages/sdk/.../postgres.ts `encodeUserinfo`) produce byte-identical URLs for any
    // generated password. This pins the shared encoding contract on the Rust side.
    #[test]
    fn connection_string_percent_encodes_rfc3986_sub_delims() {
        let binding = PostgresBinding::local("h", 5432, "db", "alien", "a!b*c'd(e)f");
        let pg = LocalPostgres::from_binding("db", &binding).expect("local binding resolves");
        assert_eq!(
            pg.connection_string(),
            "postgres://alien:a%21b%2Ac%27d%28e%29f@h:5432/db?sslmode=disable"
        );
    }

    #[test]
    fn cloud_binding_resolution_is_rejected_in_this_build() {
        let binding = PostgresBinding::Aurora(alien_core::bindings::AuroraPostgresBinding {
            cluster_endpoint: "cluster.rds.amazonaws.com".into(),
            port: BindingValue::value(5432),
            database: "db".into(),
            username: "alien".into(),
            password_secret_arn: "arn:aws:secretsmanager:...".into(),
        });
        assert!(LocalPostgres::from_binding("db", &binding).is_err());
    }
}
