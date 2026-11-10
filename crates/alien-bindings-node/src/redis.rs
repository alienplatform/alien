//! Redis binding handle. Thin translation over the `Redis` trait.
//!
//! Like Postgres, Redis is connection-only: the Rust provider has already resolved the
//! details (including reading a cloud backend's password from its secret store) by the
//! time `BindingsHandle::redis` returns, so the handle is synchronous.

use alien_bindings::{Redis, RedisConnectionParams, RedisTlsPolicy};
use napi_derive::napi;
use std::sync::Arc;

/// Everything a Redis client needs to connect.
#[napi(object)]
pub struct RedisConnectionJs {
    /// `redis://` or `rediss://` URL with the credentials percent-encoded. Database 0.
    pub url: String,
    pub host: String,
    pub port: u32,
    pub username: String,
    /// Connection password, already resolved from the cloud secret store where applicable.
    pub password: String,
    /// Whether the connection uses TLS.
    pub tls: bool,
    /// PEM roots to verify the server with; empty means the system trust store.
    pub ca_certificates: Vec<String>,
    /// Name to verify the certificate against, when it differs from `host`.
    pub server_name: Option<String>,
}

/// Handle to a resolved Redis binding.
#[napi]
pub struct RedisHandle {
    inner: Arc<dyn Redis>,
}

impl RedisHandle {
    pub(crate) fn new(inner: Arc<dyn Redis>) -> Self {
        Self { inner }
    }
}

fn connection_to_js(params: &RedisConnectionParams) -> RedisConnectionJs {
    let server_name = match &params.tls {
        RedisTlsPolicy::Enabled { server_name, .. } => server_name.clone(),
        RedisTlsPolicy::Disabled => None,
    };
    RedisConnectionJs {
        url: params.connection_url(),
        host: params.host.clone(),
        port: u32::from(params.port),
        username: params.username.clone(),
        password: params.password.clone(),
        tls: params.tls.is_enabled(),
        ca_certificates: params.tls.ca_certificates().to_vec(),
        server_name,
    }
}

#[napi]
impl RedisHandle {
    /// Return the resolved connection details.
    #[napi]
    pub fn connection(&self) -> RedisConnectionJs {
        connection_to_js(self.inner.connection_params())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_to_js_maps_every_field() {
        let pem = "-----BEGIN CERTIFICATE-----\nroot\n-----END CERTIFICATE-----".to_string();
        let params = RedisConnectionParams {
            host: "10.0.0.3".to_string(),
            port: 6379,
            username: "app".to_string(),
            password: "p@ss/word".to_string(),
            tls: RedisTlsPolicy::with_ca_certificates(vec![pem.clone(), pem.clone()]).unwrap(),
        };
        let js = connection_to_js(&params);
        assert_eq!(js.url, "rediss://app:p%40ss%2Fword@10.0.0.3:6379/0");
        assert_eq!((js.host.as_str(), js.port), ("10.0.0.3", 6379));
        assert!(js.tls);
        assert_eq!(js.ca_certificates, vec![pem.clone(), pem]);
        assert_eq!(js.server_name, None);
    }

    #[test]
    fn plaintext_connection_has_no_tls_material() {
        let params = RedisConnectionParams {
            host: "127.0.0.1".to_string(),
            port: 6380,
            username: "default".to_string(),
            password: "pw".to_string(),
            tls: RedisTlsPolicy::Disabled,
        };
        let js = connection_to_js(&params);
        assert!(!js.tls);
        assert!(js.ca_certificates.is_empty());
        assert!(js.url.starts_with("redis://"));
    }
}
