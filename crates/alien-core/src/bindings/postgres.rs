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
    /// Aurora-managed JSON credentials. Separate tag makes older readers reject
    /// this binding rather than treating the JSON document as a raw password.
    #[serde(rename = "aurora-managed")]
    AuroraManaged(AuroraPostgresBinding),
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
    /// Per-instance Cloud SQL server CA certificates, including every root accepted
    /// during a CA rotation.
    pub server_ca_certificates: BindingValue<Vec<String>>,
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

/// TLS policy for an operator-provided / BYO Postgres database.
///
/// Unlike libpq's ambiguous `prefer` mode, both choices map exactly to the
/// connection settings exposed by every supported SDK.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ExternalPostgresSslMode {
    /// Require TLS and verify the server certificate and hostname.
    #[default]
    VerifyFull,
    /// Connect over plaintext. Intended only for explicitly configured legacy servers.
    Disable,
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
    /// Connection password or Kubernetes Secret reference. Kubernetes workloads resolve
    /// the reference through secretKeyRef before the application loads its binding.
    #[serde(deserialize_with = "deserialize_external_password")]
    #[cfg_attr(feature = "openapi", schema(value_type = ExternalPasswordInput, inline))]
    #[cfg_attr(feature = "jsonschema", schemars(with = "ExternalPasswordInput"))]
    pub password: BindingValue<String>,
    /// Explicit TLS policy. Missing legacy configuration defaults to verified TLS.
    #[serde(default)]
    pub ssl_mode: ExternalPostgresSslMode,
}

// Password references extend the old string contract without accepting numeric,
// boolean, null, or array passwords through BindingValue's expression fallback.
#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(untagged)]
enum ExternalPasswordInput {
    Literal(String),
    Reference(ExternalPasswordSecretRef),
    ChartReference(ExternalPasswordSecretKeyRef),
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExternalPasswordSecretRef {
    secret_ref: super::SecretReference,
}

// Chart-provisioned passwords use a distinct reference so projection can preserve
// the published format for existing Secret references.
#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExternalPasswordSecretKeyRef {
    secret_key_ref: super::SecretReference,
}

fn deserialize_external_password<'de, D>(deserializer: D) -> Result<BindingValue<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if !value.is_string()
        && !["secretRef", "secretKeyRef"]
            .iter()
            .any(|key| value.get(key).is_some_and(serde_json::Value::is_object))
    {
        return Err(serde::de::Error::custom(
            "expected a password string or a Secret reference with name and key",
        ));
    }
    match serde_json::from_value::<ExternalPasswordInput>(value)
        .map_err(serde::de::Error::custom)?
    {
        ExternalPasswordInput::Literal(value) => Ok(BindingValue::Value(value)),
        ExternalPasswordInput::Reference(reference) => Ok(BindingValue::SecretRef {
            secret_ref: reference.secret_ref,
        }),
        ExternalPasswordInput::ChartReference(reference) => {
            Ok(BindingValue::Expression(serde_json::json!({
                "secretKeyRef": {"name": reference.secret_key_ref.name, "key": reference.secret_key_ref.key}
            })))
        }
    }
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
            .field("ssl_mode", &self.ssl_mode)
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
            password: BindingValue::value(password.into()),
            ssl_mode: ExternalPostgresSslMode::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_password_rejects_non_string_literals() {
        for password in [
            serde_json::json!(42),
            serde_json::json!(false),
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!([{"name":"credentials", "key":"password"}]),
            serde_json::json!({"secretRef":["credentials", "password"]}),
            serde_json::json!({"secretKeyRef":{"name":"credentials"}}),
            serde_json::json!({"secretKeyRef":{"key":"password"}}),
            serde_json::json!({"secretRef": {"name": "credentials"}}),
            serde_json::json!({"secretRef": {"key": "password"}}),
            serde_json::json!({"secretRef": {"name": 4, "key": "password"}}),
            serde_json::json!({"envRef": "PASSWORD"}),
        ] {
            let binding = serde_json::json!({
                "service": "external", "host": "db.internal", "port": 5432,
                "database": "app", "username": "app", "password": password
            });
            assert!(serde_json::from_value::<PostgresBinding>(binding).is_err());
        }
    }

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
        let json = serde_json::to_string(&binding).expect("external binding serializes");
        assert!(json.contains(r#""service":"external""#));
        assert!(json.contains(r#""sslMode":"verify-full""#));
        let deserialized: PostgresBinding =
            serde_json::from_str(&json).expect("external binding deserializes");
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn chart_password_reference_keeps_its_projection_mode() {
        let input = serde_json::json!({"service":"external", "host":"database", "port":5432,
            "database":"app", "username":"app", "password":{"secretKeyRef":{"name":"database", "key":"password"}}});
        let binding: PostgresBinding = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(binding).unwrap()["password"],
            input["password"]
        );
    }

    #[test]
    fn external_binding_preserves_kubernetes_password_reference() {
        let json = serde_json::json!({
            "service": "external",
            "host": "database",
            "port": 5432,
            "database": "app",
            "username": "app",
            "password": {"secretRef": {"name": "database-credentials", "key": "password"}},
            "sslMode": "disable"
        });
        let binding: PostgresBinding = serde_json::from_value(json.clone())
            .expect("external binding accepts a Kubernetes password reference");
        assert_eq!(serde_json::to_value(&binding).unwrap(), json);
        assert!(!format!("{binding:?}").contains("database-credentials"));
    }

    #[test]
    fn external_binding_without_ssl_mode_defaults_to_verified_tls() {
        let json = r#"{
            "service": "external",
            "host": "db.internal",
            "port": 5432,
            "database": "app",
            "username": "alien",
            "password": "secret"
        }"#;

        let binding: PostgresBinding =
            serde_json::from_str(json).expect("legacy external binding deserializes");

        let PostgresBinding::External(binding) = binding else {
            panic!("expected external Postgres binding");
        };
        assert_eq!(binding.ssl_mode, ExternalPostgresSslMode::VerifyFull);
    }

    #[test]
    fn external_binding_accepts_explicit_plaintext_opt_out() {
        let json = r#"{
            "service": "external",
            "host": "db.internal",
            "port": 5432,
            "database": "app",
            "username": "alien",
            "password": "secret",
            "sslMode": "disable"
        }"#;

        let binding: PostgresBinding =
            serde_json::from_str(json).expect("plaintext external binding deserializes");

        let PostgresBinding::External(binding) = binding else {
            panic!("expected external Postgres binding");
        };
        assert_eq!(binding.ssl_mode, ExternalPostgresSslMode::Disable);
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
            server_ca_certificates: BindingValue::value(vec!["cloud-sql-root".to_string()]),
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
