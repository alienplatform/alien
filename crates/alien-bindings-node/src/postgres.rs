//! Postgres binding handle. Thin argument/error translation over the `Postgres`
//! trait.
//!
//! Postgres is connection-only: there are no operations to forward, just the connection
//! details the Rust provider already resolved (including reading a cloud backend's
//! password from its secret store). The handle is therefore synchronous — every field is
//! in memory by the time `BindingsHandle::postgres` returns.

use alien_bindings::{Postgres, PostgresConnectionParams};
use napi_derive::napi;
use std::sync::Arc;

/// Everything a Postgres driver needs to connect.
#[napi(object)]
pub struct PostgresConnectionJs {
    /// `postgres://user:password@host:port/database?sslmode=<mode>`, with the username,
    /// password, and database percent-encoded.
    pub connection_string: String,
    /// Address to dial (the cluster writer endpoint for Aurora).
    pub host: String,
    /// TCP port.
    pub port: u32,
    /// Database name.
    pub database: String,
    /// Role to connect as.
    pub username: String,
    /// Connection password, already resolved from the cloud secret store where applicable.
    pub password: String,
    /// `disable` (local or explicit BYO plaintext), `verify-ca` (Cloud SQL), or
    /// `verify-full` (BYO, Aurora, and Flexible Server).
    pub sslmode: String,
    /// PEM-encoded root CAs used by the verified TLS modes.
    pub ca_certificates: Vec<String>,
}

/// Handle to a resolved Postgres binding.
#[napi]
pub struct PostgresHandle {
    inner: Arc<dyn Postgres>,
}

impl PostgresHandle {
    pub(crate) fn new(inner: Arc<dyn Postgres>) -> Self {
        Self { inner }
    }
}

/// Translate resolved connection parameters into their JS shape.
fn connection_to_js(params: &PostgresConnectionParams) -> PostgresConnectionJs {
    PostgresConnectionJs {
        connection_string: params.connection_string(),
        host: params.host.clone(),
        port: u32::from(params.port),
        database: params.database.clone(),
        username: params.username.clone(),
        password: params.password.clone(),
        sslmode: params.sslmode.as_str().to_string(),
        ca_certificates: params.ca_certificates.clone(),
    }
}

#[napi]
impl PostgresHandle {
    /// Return the resolved connection details.
    #[napi]
    pub fn connection(&self) -> PostgresConnectionJs {
        connection_to_js(&self.inner.connection_params())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_bindings::SslMode;

    /// The JS shape must carry every field verbatim, with the connection string derived
    /// from the same params (not stored separately) and `sslmode` as its wire string.
    #[test]
    fn connection_to_js_maps_every_field() {
        let params = PostgresConnectionParams {
            host: "cluster.rds.amazonaws.com".to_string(),
            port: 5432,
            database: "app".to_string(),
            username: "alien".to_string(),
            password: "p@ss/word".to_string(),
            sslmode: SslMode::VerifyFull,
            ca_certificates: vec![
                "-----BEGIN CERTIFICATE-----\nroot\n-----END CERTIFICATE-----".to_string(),
            ],
        };

        let js = connection_to_js(&params);

        assert_eq!(js.host, "cluster.rds.amazonaws.com");
        assert_eq!(js.port, 5432);
        assert_eq!(js.database, "app");
        assert_eq!(js.username, "alien");
        assert_eq!(js.password, "p@ss/word");
        assert_eq!(js.sslmode, "verify-full");
        assert_eq!(js.ca_certificates, params.ca_certificates);
        assert_eq!(
            js.connection_string,
            "postgres://alien:p%40ss%2Fword@cluster.rds.amazonaws.com:5432/app?sslmode=verify-full"
        );
    }

    /// Each `SslMode` must reach JS as the exact string the `sslmode` query parameter
    /// uses, so the TS layer can map it to a driver TLS setting without guessing.
    #[test]
    fn connection_to_js_reports_each_sslmode_as_its_wire_string() {
        for (mode, expected) in [
            (SslMode::Disable, "disable"),
            (SslMode::VerifyCa, "verify-ca"),
            (SslMode::VerifyFull, "verify-full"),
        ] {
            let params = PostgresConnectionParams {
                host: "h".to_string(),
                port: 5432,
                database: "db".to_string(),
                username: "u".to_string(),
                password: "p".to_string(),
                sslmode: mode,
                ca_certificates: match mode {
                    SslMode::VerifyCa | SslMode::VerifyFull => vec![
                        "-----BEGIN CERTIFICATE-----\nroot\n-----END CERTIFICATE-----".to_string(),
                    ],
                    SslMode::Disable => Vec::new(),
                },
            };

            assert_eq!(connection_to_js(&params).sslmode, expected);
        }
    }
}
