use std::{sync::Arc, time::Duration};

use alien_bindings::Bindings;
use alien_error::{Context, IntoAlienError};
use base64::Engine;
use hmac::{Hmac, Mac};
use http::{HeaderName, HeaderValue, Request};
use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DnType, ExtendedKeyUsagePurpose,
    GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose, NameConstraints,
};
use rustls::{
    crypto::ring,
    pki_types::{PrivatePkcs8KeyDer, ServerName},
    ClientConfig, RootCertStore, ServerConfig,
};
use sha2::Sha256;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::{
    config::CredentialRule,
    credential_cache::Cache,
    error::{invalid, unavailable, Result},
    http_proxy,
    policy::{normalize_host, Target},
    server::HEADER_TIMEOUT,
};

/// Gateway-only credential access and deterministic per-sandbox certificate authorities.
pub struct Credentials {
    seed: Option<[u8; 32]>,
    bindings: Arc<Bindings>,
    upstream: Arc<ClientConfig>,
    rules: Vec<CredentialRule>,
    certificates: Cache<(String, String, String), Arc<ServerConfig>>,
    secrets: Cache<(String, String), String>,
}

struct Ca {
    certificate: String,
    private_key: String,
}

impl Credentials {
    /// Use a gateway-only seed for CA material and vault bindings for header values.
    pub fn new(
        bindings: Arc<Bindings>,
        additional_roots: &[String],
        rules: &[CredentialRule],
        seed: Option<[u8; 32]>,
    ) -> Result<Self> {
        let mut roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for root in additional_roots {
            let der = base64::engine::general_purpose::STANDARD
                .decode(root)
                .into_alien_error()
                .context(unavailable("decode upstream CA"))?;
            roots
                .add(rustls::pki_types::CertificateDer::from(der))
                .into_alien_error()
                .context(unavailable("load upstream CA"))?;
        }
        // Workspace feature unification may enable more than one TLS provider.
        let mut config = ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .into_alien_error()
            .context(unavailable("configure upstream TLS versions"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Self {
            seed,
            bindings,
            upstream: Arc::new(config),
            rules: rules.to_vec(),
            certificates: Cache::new(Duration::from_secs(3600)),
            secrets: Cache::new(Duration::from_secs(60)),
        })
    }

    /// Deterministic sandbox CAs survive replica replacement without a policy database.
    /// The deployment's gateway-only key never leaves the configured vault entry.
    pub async fn issue_ca(&self, aud: &str, sub: &str) -> Result<String> {
        Ok(self.ca(aud, sub)?.certificate)
    }

    fn ca(&self, aud: &str, sub: &str) -> Result<Ca> {
        let seed = self
            .seed
            .ok_or_else(|| invalid("credential CA requires replicaSecret"))?;
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&seed).map_err(|_| invalid("invalid replicaSecret"))?;
        mac.update(b"alien-egress-ca-v1");
        for value in std::iter::once(aud.to_string())
            .chain(std::iter::once(sub.to_string()))
            .chain(self.hosts(aud))
        {
            mac.update(&(value.len() as u64).to_be_bytes());
            mac.update(value.as_bytes());
        }
        // RFC 8410 PKCS#8 Ed25519 seed, with no optional attributes or public-key field.
        let mut der = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
            0x04, 0x20,
        ];
        der.extend_from_slice(&mac.finalize().into_bytes());
        let key = KeyPair::from_der_and_sign_algo(
            &PrivatePkcs8KeyDer::from(der).into(),
            &rcgen::PKCS_ED25519,
        )
        .into_alien_error()
        .context(unavailable("derive sandbox CA"))?;
        let mut params = CertificateParams::default();
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: self
                .hosts(aud)
                .into_iter()
                .map(GeneralSubtree::DnsName)
                .collect(),
            excluded_subtrees: vec![
                GeneralSubtree::IpAddress(CidrSubnet::V4([0; 4], [0; 4])),
                GeneralSubtree::IpAddress(CidrSubnet::V6([0; 16], [0; 16])),
            ],
        });
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params
            .distinguished_name
            .push(DnType::CommonName, "Sandbox egress CA");
        let cert = params
            .self_signed(&key)
            .into_alien_error()
            .context(unavailable("sign sandbox CA"))?;
        Ok(Ca {
            certificate: cert.pem(),
            private_key: key.serialize_pem(),
        })
    }

    fn hosts(&self, aud: &str) -> Vec<String> {
        let mut hosts: Vec<_> = self
            .rules
            .iter()
            .filter(|rule| rule.aud == aud)
            .map(|rule| rule.host.clone())
            .collect();
        hosts.sort();
        hosts.dedup();
        // Empty permitted subtrees mean unrestricted trust. Keep an unused CA constrained too.
        if hosts.is_empty() {
            hosts.push("no-credential-host.invalid".into());
        }
        hosts
    }

    async fn secret(&self, binding: &str, name: &str) -> Result<String> {
        let key = (binding.to_string(), name.to_string());
        if let Some(secret) = self.secrets.get(&key) {
            return Ok(secret);
        }
        let vault = self
            .bindings
            .vault(binding)
            .await
            .context(unavailable("load credential vault"))?;
        let secret = vault
            .get_secret(name)
            .await
            .context(unavailable("read injection credential"))?;
        self.secrets.insert(key, secret.clone());
        Ok(secret)
    }

    async fn server_config(
        &self,
        aud: &str,
        sub: &str,
        target: &Target,
    ) -> Result<Arc<ServerConfig>> {
        if !self.hosts(aud).contains(&target.host) {
            return Err(invalid("host outside sandbox CA scope"));
        }
        let cache_key = (aud.to_string(), sub.to_string(), target.host.clone());
        if let Some(config) = self.certificates.get(&cache_key) {
            return Ok(config);
        }
        let ca = self.ca(aud, sub)?;
        let key = KeyPair::from_pem(&ca.private_key)
            .into_alien_error()
            .context(unavailable("load CA key"))?;
        let issuer = Issuer::from_ca_cert_pem(&ca.certificate, key)
            .into_alien_error()
            .context(unavailable("load CA certificate"))?;
        let key = KeyPair::generate()
            .into_alien_error()
            .context(unavailable("generate leaf key"))?;
        let mut params = CertificateParams::new(vec![target.host.clone()])
            .into_alien_error()
            .context(unavailable("prepare leaf certificate"))?;
        params.use_authority_key_identifier_extension = true;
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let cert = params
            .signed_by(&key, &issuer)
            .into_alien_error()
            .context(unavailable("sign leaf certificate"))?;
        let mut config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .into_alien_error()
            .context(unavailable("configure guest TLS versions"))?
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .into_alien_error()
            .context(unavailable("configure guest TLS"))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let config = Arc::new(config);
        self.certificates.insert(cache_key, config.clone());
        Ok(config)
    }

    /// Terminate guest TLS, obtain vault values, and re-originate verified TLS to the pinned address.
    pub async fn forward<G>(
        &self,
        guest: G,
        upstream: TcpStream,
        identity: (&str, &str),
        target: Target,
        rules: &[CredentialRule],
    ) -> Result<()>
    where
        G: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let server = self.server_config(identity.0, identity.1, &target).await?;
        let name = ServerName::try_from(target.host.clone())
            .map_err(|_| invalid("invalid TLS server name"))?;
        let upstream = timeout(
            HEADER_TIMEOUT,
            TlsConnector::from(self.upstream.clone()).connect(name, upstream),
        )
        .await
        .into_alien_error()
        .context(unavailable("upstream TLS timeout"))?
        .into_alien_error()
        .context(unavailable("verify upstream TLS"))?;
        let guest = timeout(HEADER_TIMEOUT, TlsAcceptor::from(server).accept(guest))
            .await
            .into_alien_error()
            .context(unavailable("guest TLS timeout"))?
            .into_alien_error()
            .context(unavailable("accept guest TLS"))?;
        http_proxy::forward(guest, upstream, target, Some((self, rules))).await
    }

    /// Resolve only the secrets whose rules match this validated HTTP request.
    pub(crate) async fn inject<B>(
        &self,
        request: &mut Request<B>,
        rules: &[CredentialRule],
    ) -> Result<()> {
        for rule in rules {
            let path = request.uri().path();
            if request.method().as_str() != rule.method
                || !(path == rule.path
                    || (rule.path.ends_with('/') && path.starts_with(&rule.path)))
            {
                continue;
            }
            let secret = self.secret(&rule.vault_binding, &rule.secret).await?;
            let value = HeaderValue::from_str(&format!("{}{}", rule.value_prefix, secret))
                .map_err(|_| invalid("credential is not a valid HTTP header value"))?;
            let name = HeaderName::from_bytes(rule.header.as_bytes())
                .map_err(|_| invalid("invalid injection header"))?;
            request.headers_mut().insert(name, value);
        }
        Ok(())
    }
}

/// Validate rules at startup, including fields that affect HTTP framing or trust boundaries.
pub fn validate_rules(rules: &[CredentialRule]) -> Result<()> {
    for rule in rules {
        if normalize_host(&rule.host)? != rule.host
            || rule.port == 0
            || rule.aud.is_empty()
            || !rule.path.starts_with('/')
            || rule.path.contains(['?', '#', '%'])
            || rule.path.contains("..")
            || rule.method.parse::<http::Method>().is_err()
            || rule.secret.is_empty()
            || rule.vault_binding.is_empty()
        {
            return Err(invalid("invalid credential scope"));
        }
        let name = HeaderName::from_bytes(rule.header.as_bytes())
            .map_err(|_| invalid("invalid injection header"))?;
        if [
            "host",
            "content-length",
            "transfer-encoding",
            "connection",
            "proxy-authorization",
            "upgrade",
            "trailer",
            "te",
        ]
        .contains(&name.as_str())
        {
            return Err(invalid(
                "credential cannot replace routing or framing headers",
            ));
        }
    }
    Ok(())
}

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;
    use rustls::client::danger::ServerCertVerifier;
    use std::collections::HashMap;

    #[tokio::test(start_paused = true)]
    async fn constrained_ca_and_expiring_caches_preserve_credential_scope() {
        let dir = tempfile::tempdir().unwrap();
        let bindings = Arc::new(Bindings::from_env_map(HashMap::from([
            ("ALIEN_DEPLOYMENT_TYPE".into(), "local".into()),
            ("ALIEN_TEST_VAULT_BINDING".into(), serde_json::json!({"service":"local-vault","dataDir":dir.path().join("vault"),"vaultName":"test"}).to_string()),
        ])).unwrap());
        let rule: CredentialRule = serde_json::from_value(serde_json::json!({"aud":"p/r","host":"inject.example.com","port":443,"method":"GET","path":"/","header":"Authorization","vaultBinding":"test-vault","secret":"key"})).unwrap();
        let credentials =
            Credentials::new(bindings.clone(), &[], &[rule.clone()], Some([7; 32])).unwrap();
        let certificate = credentials.issue_ca("p/r", "box").await.unwrap();
        let replica = Credentials::new(bindings.clone(), &[], &[rule], Some([7; 32])).unwrap();
        assert_eq!(certificate, replica.issue_ca("p/r", "box").await.unwrap());
        assert_ne!(
            certificate,
            replica.issue_ca("p/r", "different").await.unwrap()
        );
        let ca = credentials.ca("p/r", "box").unwrap();
        let issuer =
            Issuer::from_ca_cert_pem(&ca.certificate, KeyPair::from_pem(&ca.private_key).unwrap())
                .unwrap();
        let encoded: String = certificate
            .lines()
            .filter(|line| !line.starts_with("---"))
            .collect();
        let mut roots = RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .unwrap(),
            ))
            .unwrap();
        let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
            Arc::new(roots),
            Arc::new(ring::default_provider()),
        )
        .build()
        .unwrap();
        for (host, allowed) in [
            ("inject.example.com", true),
            ("unrelated.example.com", false),
            ("1.1.1.1", false),
        ] {
            // Deliberately sign an out-of-scope certificate using the fixture CA key: the client
            // must reject it even when its SAN matches the requested name and signature is valid.
            let key = KeyPair::generate().unwrap();
            let params = CertificateParams::new(vec![host.into()]).unwrap();
            let leaf = params.signed_by(&key, &issuer).unwrap();
            let result = verifier.verify_server_cert(
                leaf.der(),
                &[],
                &ServerName::try_from(host).unwrap(),
                &[],
                rustls::pki_types::UnixTime::now(),
            );
            assert_eq!(result.is_ok(), allowed, "{host}: {result:?}");
        }
        let target = Target {
            host: "inject.example.com".into(),
            port: 443,
        };
        let first = credentials
            .server_config("p/r", "box", &target)
            .await
            .unwrap();
        assert!(Arc::ptr_eq(
            &first,
            &credentials
                .server_config("p/r", "box", &target)
                .await
                .unwrap()
        ));
        tokio::time::advance(Duration::from_secs(3601)).await;
        assert!(!Arc::ptr_eq(
            &first,
            &credentials
                .server_config("p/r", "box", &target)
                .await
                .unwrap()
        ));
        let vault = bindings.vault("test-vault").await.unwrap();
        vault
            .set_secret("key", "first-fixture-secret")
            .await
            .unwrap();
        assert_eq!(
            credentials.secret("test-vault", "key").await.unwrap(),
            "first-fixture-secret"
        );
        vault
            .set_secret("key", "rotated-fixture-secret")
            .await
            .unwrap();
        assert_eq!(
            credentials.secret("test-vault", "key").await.unwrap(),
            "first-fixture-secret"
        );
        tokio::time::advance(Duration::from_secs(60)).await;
        assert_eq!(
            credentials.secret("test-vault", "key").await.unwrap(),
            "rotated-fixture-secret"
        );
        vault.delete_secret("key").await.unwrap();
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(credentials.secret("test-vault", "key").await.is_err());
    }
}
