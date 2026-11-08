//! Redis binding providers.
//!
//! Redis is connection-only: the provider resolves connection details and the
//! application connects with its own client. There is no gRPC service.
//!
//! `Local` and `External` carry the password inline (or as a Secret reference the
//! workload has already materialized). Cloud variants carry a pointer to the password
//! in the cloud's secret store and are resolved by their own provider.

pub(crate) mod cloud;

use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Redis, RedisConnectionParams, RedisTlsPolicy};
use alien_core::bindings::{BindingValue, RedisBinding};
use alien_error::{AlienError, Context};

/// A Redis handle whose cloud-specific work has already been completed.
#[derive(Debug)]
pub struct ResolvedRedis {
    params: RedisConnectionParams,
}

impl ResolvedRedis {
    pub fn new(params: RedisConnectionParams) -> Self {
        Self { params }
    }

    /// Resolves a binding that carries its password inline: Local and External.
    pub fn from_inline_binding(binding_name: &str, binding: &RedisBinding) -> Result<Self> {
        let params = match binding {
            RedisBinding::Local(b) => resolve_params(
                binding_name,
                &b.host,
                &b.port,
                &b.username,
                &b.password,
                RedisTlsPolicy::Disabled,
            )?,
            RedisBinding::External(b) => {
                let password = b
                    .password
                    .clone()
                    .into_value(binding_name, "password")
                    .context(ErrorData::config_invalid(
                        binding_name,
                        "Redis password reference must be resolved before loading the binding",
                    ))?;
                let tls = if b.tls {
                    RedisTlsPolicy::system_roots()
                } else {
                    RedisTlsPolicy::Disabled
                };
                resolve_params(binding_name, &b.host, &b.port, &b.username, &password, tls)?
            }
            // Listed explicitly so a new variant forces a compile error. Reaching this arm
            // means a cloud binding was dispatched to the inline provider: a bug, not bad
            // user configuration.
            RedisBinding::ElastiCache(_)
            | RedisBinding::Memorystore(_)
            | RedisBinding::AzureManagedRedis(_) => {
                return Err(AlienError::new(ErrorData::config_invalid(
                    binding_name,
                    "Cloud Redis bindings carry a password locator and must be resolved by \
                     their own cloud provider, not the inline-password provider",
                )));
            }
        };
        Ok(Self::new(params))
    }
}

impl Binding for ResolvedRedis {}

impl Redis for ResolvedRedis {
    fn connection_params(&self) -> &RedisConnectionParams {
        &self.params
    }
}

/// Combines a binding's concrete connection fields with an already-resolved password.
/// An unresolved template or `SecretRef` is a user-fixable configuration problem.
pub(crate) fn resolve_params(
    binding_name: &str,
    host: &BindingValue<String>,
    port: &BindingValue<u16>,
    username: &BindingValue<String>,
    password: &str,
    tls: RedisTlsPolicy,
) -> Result<RedisConnectionParams> {
    let invalid = |field: &str| ErrorData::BindingConfigInvalid {
        env_var: crate::error::binding_env_var(binding_name),
        binding_name: binding_name.to_string(),
        reason: format!("Failed to extract '{}' from Redis binding", field),
    };
    Ok(RedisConnectionParams {
        host: host
            .clone()
            .into_value(binding_name, "host")
            .context(invalid("host"))?,
        port: port
            .clone()
            .into_value(binding_name, "port")
            .context(invalid("port"))?,
        username: username
            .clone()
            .into_value(binding_name, "username")
            .context(invalid("username"))?,
        password: password.to_string(),
        tls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_binding_is_plaintext_and_encodes_the_password() {
        let binding = RedisBinding::local("127.0.0.1", 6380, "default", "p@ss/word");
        let redis = ResolvedRedis::from_inline_binding("cache", &binding).unwrap();
        assert!(!redis.connection_params().tls.is_enabled());
        assert_eq!(
            redis.connection_url(),
            "redis://default:p%40ss%2Fword@127.0.0.1:6380/0"
        );
    }

    #[test]
    fn external_binding_uses_tls_unless_told_not_to() {
        let json = |tls: Option<bool>| {
            let mut v = serde_json::json!({
                "service": "external", "host": "cache.internal", "port": 6379,
                "username": "default", "password": "pw"
            });
            if let Some(tls) = tls {
                v["tls"] = tls.into();
            }
            let binding: RedisBinding = serde_json::from_value(v).unwrap();
            ResolvedRedis::from_inline_binding("cache", &binding).unwrap()
        };
        assert_eq!(
            json(None).connection_url(),
            "rediss://default:pw@cache.internal:6379/0"
        );
        assert_eq!(
            json(Some(false)).connection_url(),
            "redis://default:pw@cache.internal:6379/0"
        );
    }

    #[test]
    fn unresolved_secret_reference_is_a_configuration_error() {
        let binding: RedisBinding = serde_json::from_value(serde_json::json!({
            "service": "external", "host": "cache", "port": 6379, "username": "default",
            "password": {"secretRef": {"name": "redis", "key": "password"}}
        }))
        .unwrap();
        let err = ResolvedRedis::from_inline_binding("cache", &binding).unwrap_err();
        assert!(err.to_string().contains("must be resolved"), "{err}");
    }

    #[test]
    fn debug_never_prints_the_password() {
        let binding = RedisBinding::local("127.0.0.1", 6380, "default", "hunter2");
        let redis = ResolvedRedis::from_inline_binding("cache", &binding).unwrap();
        assert!(!format!("{redis:?}").contains("hunter2"));
    }

    #[test]
    fn ca_certificates_must_be_pem() {
        assert!(RedisTlsPolicy::with_ca_certificates(vec![]).is_err());
        assert!(RedisTlsPolicy::with_ca_certificates(vec!["not a cert".into()]).is_err());
        let pem = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----";
        let tls = RedisTlsPolicy::with_ca_certificates(vec![pem.into(), pem.into()]).unwrap();
        assert_eq!(tls.ca_certificates().len(), 2);
    }
}
