//! Postgres binding definitions across platforms.
//!
//! The binding carries only connection details. Cloud variants keep the password
//! out of state by referencing the cloud secret store (ARN / name / URI), resolved
//! at load time; Local and External carry the password inline as a `BindingValue`.

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Connection details for a Postgres database, one variant per backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
// `rename_all = "lowercase"` would drop the hyphen (CloudSql -> cloudsql); the explicit
// renames keep the wire tags `cloud-sql`/`flexible-server`/`local-postgres`. Every tag is
// globally unique across all binding enums (serde dispatches on `service` alone).
#[serde(tag = "service", rename_all = "lowercase")]
pub enum PostgresBinding {
    /// AWS Aurora Serverless v2 (cluster endpoint + secret ARN).
    Aurora(AuroraPostgresBinding),
    /// GCP Cloud SQL (host + secret name).
    #[serde(rename = "cloud-sql")]
    CloudSql(CloudSqlPostgresBinding),
    /// Azure Database for PostgreSQL — Flexible Server (host + secret URI).
    #[serde(rename = "flexible-server")]
    FlexibleServer(FlexibleServerPostgresBinding),
    /// Operator-provided / BYO database (Kubernetes, on-prem, or cloud override).
    External(ExternalPostgresBinding),
    /// Local embedded Postgres process.
    #[serde(rename = "local-postgres")]
    Local(LocalPostgresBinding),
}

/// AWS Aurora Serverless v2 binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AuroraPostgresBinding {
    pub cluster_endpoint: BindingValue<String>,
    pub port: BindingValue<u16>,
    pub database: BindingValue<String>,
    pub username: BindingValue<String>,
    /// Secrets Manager ARN of the connection password; resolved at load time.
    pub password_secret_arn: BindingValue<String>,
}

/// GCP Cloud SQL binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudSqlPostgresBinding {
    pub host: BindingValue<String>,
    pub port: BindingValue<u16>,
    pub database: BindingValue<String>,
    pub username: BindingValue<String>,
    /// Secret Manager secret name of the connection password; resolved at load time.
    pub password_secret_name: BindingValue<String>,
}

/// Azure Flexible Server binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlexibleServerPostgresBinding {
    pub host: BindingValue<String>,
    pub port: BindingValue<u16>,
    pub database: BindingValue<String>,
    pub username: BindingValue<String>,
    /// Key Vault secret URI of the connection password; resolved at load time.
    pub password_secret_uri: BindingValue<String>,
}

/// Operator-provided / BYO database binding.
// No derived `Debug` — inline `password` would print cleartext; see the redacting impl below.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ExternalPostgresBinding {
    pub host: BindingValue<String>,
    pub port: BindingValue<u16>,
    pub database: BindingValue<String>,
    pub username: BindingValue<String>,
    /// Connection password as a concrete value, never an unresolved `SecretRef`: the platform
    /// materializes the Kubernetes secret into the pod env. The cloud variants carry a secret
    /// locator instead.
    pub password: String,
}

/// Local embedded Postgres binding.
// No derived `Debug` — inline `password` would print cleartext; see the redacting impl below.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalPostgresBinding {
    pub host: BindingValue<String>,
    pub port: BindingValue<u16>,
    pub database: BindingValue<String>,
    pub username: BindingValue<String>,
    pub password: String,
}

// These impls redact the inline password and keep every other field, mirroring
// `PostgresConnectionParams`. Cloud variants carry only a secret identifier, so they keep the derive.
impl std::fmt::Debug for ExternalPostgresBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalPostgresBinding")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

impl std::fmt::Debug for LocalPostgresBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPostgresBinding")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

impl PostgresBinding {
    /// Creates a Local Postgres binding.
    pub fn local(
        host: impl Into<BindingValue<String>>,
        port: u16,
        database: impl Into<BindingValue<String>>,
        username: impl Into<BindingValue<String>>,
        password: impl Into<String>,
    ) -> Self {
        Self::Local(LocalPostgresBinding {
            host: host.into(),
            port: BindingValue::value(port),
            database: database.into(),
            username: username.into(),
            password: password.into(),
        })
    }

    /// Creates an External (BYO / Kubernetes) Postgres binding.
    pub fn external(
        host: impl Into<BindingValue<String>>,
        port: u16,
        database: impl Into<BindingValue<String>>,
        username: impl Into<BindingValue<String>>,
        password: impl Into<String>,
    ) -> Self {
        Self::External(ExternalPostgresBinding {
            host: host.into(),
            port: BindingValue::value(port),
            database: database.into(),
            username: username.into(),
            password: password.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_binding_uses_local_postgres_tag() {
        let binding = PostgresBinding::local("127.0.0.1", 5432, "db", "alien", "secret");
        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains(r#""service":"local-postgres""#));
        let deserialized: PostgresBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn external_binding_uses_external_tag() {
        let binding = PostgresBinding::external("db.internal", 5432, "app", "alien", "secret");
        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains(r#""service":"external""#));
        let deserialized: PostgresBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn cloud_variants_keep_hyphenated_tags() {
        let aurora = PostgresBinding::Aurora(AuroraPostgresBinding {
            cluster_endpoint: "cluster.rds.amazonaws.com".into(),
            port: BindingValue::value(5432),
            database: "db".into(),
            username: "alien".into(),
            password_secret_arn: "arn:aws:secretsmanager:...".into(),
        });
        assert!(serde_json::to_string(&aurora)
            .unwrap()
            .contains(r#""service":"aurora""#));

        let cloud_sql = PostgresBinding::CloudSql(CloudSqlPostgresBinding {
            host: "10.0.0.5".into(),
            port: BindingValue::value(5432),
            database: "db".into(),
            username: "alien".into(),
            password_secret_name: "pg-credentials".into(),
        });
        assert!(serde_json::to_string(&cloud_sql)
            .unwrap()
            .contains(r#""service":"cloud-sql""#));

        let flexible = PostgresBinding::FlexibleServer(FlexibleServerPostgresBinding {
            host: "10.0.0.6".into(),
            port: BindingValue::value(5432),
            database: "db".into(),
            username: "alien".into(),
            password_secret_uri: "https://vault.vault.azure.net/secrets/pg".into(),
        });
        assert!(serde_json::to_string(&flexible)
            .unwrap()
            .contains(r#""service":"flexible-server""#));
    }
}
