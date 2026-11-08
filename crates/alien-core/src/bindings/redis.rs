//! Redis binding definitions across platforms.
//!
//! The binding carries only connection details. Cloud variants keep the password
//! out of state by pointing at the cloud secret store (or, on Azure, at the
//! database whose access key is read at load time); Local and External carry the
//! password as a `BindingValue`.

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Connection details for a Redis-compatible server, one variant per backend.
///
/// Service tags are unique across all binding enums (serde dispatches on
/// `service` alone), so the key-value store's `redis` tag is not reused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(tag = "service")]
pub enum RedisBinding {
    /// AWS ElastiCache for Valkey (primary endpoint + AUTH token secret ARN).
    #[serde(rename = "elasticache")]
    ElastiCache(ElastiCacheRedisBinding),
    /// GCP Memorystore for Valkey (PSC address + CA + secret name).
    #[serde(rename = "memorystore")]
    Memorystore(MemorystoreRedisBinding),
    /// Azure Managed Redis (private endpoint host + database resource ID).
    #[serde(rename = "azure-managed-redis")]
    AzureManagedRedis(AzureManagedRedisBinding),
    /// Operator-provided server (Kubernetes, on-prem, or cloud override).
    #[serde(rename = "external")]
    External(ExternalRedisBinding),
    /// Local Valkey process.
    #[serde(rename = "local-redis")]
    Local(LocalRedisBinding),
}

/// AWS ElastiCache binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ElastiCacheRedisBinding {
    pub host: BindingValue<String>,
    pub port: BindingValue<u16>,
    pub username: BindingValue<String>,
    /// Secrets Manager ARN of the AUTH token; resolved at load time.
    pub password_secret_arn: BindingValue<String>,
}

/// GCP Memorystore binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct MemorystoreRedisBinding {
    pub host: BindingValue<String>,
    pub port: BindingValue<u16>,
    pub username: BindingValue<String>,
    /// Instance CA certificates (PEM), including every root accepted during a rotation.
    pub server_ca_certificates: BindingValue<Vec<String>>,
    /// Secret Manager secret name of the token; resolved at load time.
    pub password_secret_name: BindingValue<String>,
}

/// Azure Managed Redis binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureManagedRedisBinding {
    pub host: BindingValue<String>,
    pub port: BindingValue<u16>,
    pub username: BindingValue<String>,
    /// ARM resource ID of the database; its access key is read with `listKeys`
    /// at load time.
    pub database_resource_id: BindingValue<String>,
}

/// Operator-provided server binding.
// No derived `Debug`: the inline password would print in cleartext.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ExternalRedisBinding {
    pub host: BindingValue<String>,
    pub port: BindingValue<u16>,
    pub username: BindingValue<String>,
    /// Password literal or a Secret reference.
    pub password: BindingValue<String>,
    /// Connect over TLS. Defaults to true.
    #[serde(default = "default_true")]
    pub tls: bool,
}

/// Local Valkey binding.
// No derived `Debug`: the inline password would print in cleartext.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalRedisBinding {
    pub host: BindingValue<String>,
    pub port: BindingValue<u16>,
    pub username: BindingValue<String>,
    pub password: String,
}

fn default_true() -> bool {
    true
}

impl std::fmt::Debug for ExternalRedisBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalRedisBinding")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("tls", &self.tls)
            .finish()
    }
}

impl std::fmt::Debug for LocalRedisBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalRedisBinding")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

impl RedisBinding {
    /// Creates a Local Valkey binding.
    pub fn local(
        host: impl Into<BindingValue<String>>,
        port: u16,
        username: impl Into<BindingValue<String>>,
        password: impl Into<String>,
    ) -> Self {
        Self::Local(LocalRedisBinding {
            host: host.into(),
            port: BindingValue::value(port),
            username: username.into(),
            password: password.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_distinct_from_the_key_value_store() {
        let local = RedisBinding::local("127.0.0.1", 6379, "default", "secret");
        let json = serde_json::to_value(&local).unwrap();
        assert_eq!(json["service"], "local-redis");
        assert_eq!(serde_json::from_value::<RedisBinding>(json).unwrap(), local);

        // A key-value `redis` binding must not parse as a Redis resource binding.
        let kv = serde_json::json!({"service": "redis", "host": "h", "port": 1});
        assert!(serde_json::from_value::<RedisBinding>(kv).is_err());
    }

    #[test]
    fn external_binding_defaults_to_tls_and_keeps_secret_refs() {
        let input = serde_json::json!({
            "service": "external", "host": "cache.internal", "port": 6379,
            "username": "default",
            "password": {"secretRef": {"name": "redis-credentials", "key": "password"}}
        });
        let binding: RedisBinding = serde_json::from_value(input.clone()).unwrap();
        let RedisBinding::External(external) = &binding else {
            panic!("expected external binding");
        };
        assert!(external.tls);
        assert!(!format!("{binding:?}").contains("redis-credentials"));
        assert_eq!(
            serde_json::to_value(&binding).unwrap()["password"],
            input["password"]
        );
    }

    #[test]
    fn debug_output_never_prints_passwords() {
        let local = RedisBinding::local("127.0.0.1", 6379, "default", "hunter2");
        assert!(!format!("{local:?}").contains("hunter2"));
    }
}
