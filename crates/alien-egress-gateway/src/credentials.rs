use std::{sync::Arc, time::Duration};

use alien_error::{Context, IntoAlienError};
use base64::Engine;
use hmac::{Hmac, Mac};
use http::Request;
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
    credential_cache::Cache,
    error::{denied, invalid, unavailable, Result},
    http_proxy,
    policy::Target,
    sandbox_credentials::{CredentialStore, Rule, RuleTarget},
    server::HEADER_TIMEOUT,
};

/// Upstream TLS trust and deterministic per-sandbox certificate authorities.
pub struct Credentials {
    seed: Option<[u8; 32]>,
    upstream: Arc<ClientConfig>,
    certificates: Cache<(String, String, String), Arc<ServerConfig>>,
}

struct Ca {
    certificate: String,
    private_key: String,
}

/// The credential rules one connection may use: read from the store at the moment each
/// request is forwarded, so a replaced set applies to the next request.
pub struct Injection {
    pub store: Arc<CredentialStore>,
    pub aud: String,
    pub sub: String,
    pub target: RuleTarget,
    pub port: u16,
}
impl Injection {
    pub fn current(&self) -> Vec<Rule> {
        self.store
            .rules_for(&self.aud, &self.sub, &self.target, self.port)
    }

    /// Set the header of every rule that scopes this request, replacing whatever the sandbox
    /// sent. The first matching rule for a header wins. With any rule for this destination, an
    /// ambiguous path is refused so `/v1/../admin` cannot pass a `/v1/` rule.
    pub(crate) fn apply<B>(&self, request: &mut Request<B>) -> Result<()> {
        let rules = self.current();
        if rules.is_empty() {
            return Ok(());
        }
        let path = request.uri().path().to_string();
        if path.contains(['%', '\\']) || path.split('/').any(|part| part == "." || part == "..") {
            return Err(denied("ambiguous credential path"));
        }
        let mut set = Vec::new();
        for rule in rules {
            if set.contains(&rule.header) || !rule.matches_request(request.method(), &path) {
                continue;
            }
            request
                .headers_mut()
                .insert(rule.header.clone(), rule.value.clone());
            set.push(rule.header);
        }
        Ok(())
    }
}

impl Credentials {
    /// Use a gateway-only seed for CA material.
    pub fn new(additional_roots: &[String], seed: Option<[u8; 32]>) -> Result<Self> {
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
            upstream: Arc::new(config),
            certificates: Cache::new(Duration::from_secs(3600)),
        })
    }

    /// Deterministic sandbox CAs survive replica replacement without a database, and do not
    /// change when the sandbox's rules do: processes keep trusting the CA they started with.
    pub async fn issue_ca(&self, aud: &str, sub: &str) -> Result<String> {
        Ok(self.ca(aud, sub)?.certificate)
    }

    fn ca(&self, aud: &str, sub: &str) -> Result<Ca> {
        let seed = self
            .seed
            .ok_or_else(|| invalid("credential CA requires replicaSecret"))?;
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&seed).map_err(|_| invalid("invalid replicaSecret"))?;
        mac.update(b"alien-egress-ca-v2");
        for value in [aud, sub] {
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
        // DNS names stay open: a sandbox's hosts change every turn. The gateway issues a leaf
        // only for a host that has a current rule for this sandbox, and the CA is trusted only
        // inside this sandbox. IP addresses are never covered.
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: Vec::new(),
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

    async fn server_config(
        &self,
        aud: &str,
        sub: &str,
        target: &Target,
    ) -> Result<Arc<ServerConfig>> {
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

    /// Terminate guest TLS and re-originate verified TLS to the pinned address. A leaf is issued
    /// only for a host that has a current rule for this sandbox.
    pub async fn forward<G>(
        &self,
        guest: G,
        upstream: TcpStream,
        target: Target,
        injection: &Injection,
    ) -> Result<()>
    where
        G: AsyncRead + AsyncWrite + Unpin + Send,
    {
        if injection.target != RuleTarget::Host(target.host.clone())
            || injection.current().is_empty()
        {
            return Err(invalid("host outside sandbox credential scope"));
        }
        let server = self
            .server_config(&injection.aud, &injection.sub, &target)
            .await?;
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
        http_proxy::forward(guest, upstream, target, Some(injection)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::client::danger::ServerCertVerifier;

    /// The CA is the same on every replica and across rule changes, differs per sandbox, and
    /// never covers an IP address.
    #[tokio::test(start_paused = true)]
    async fn the_sandbox_ca_is_stable_per_sandbox_and_never_covers_addresses() {
        let credentials = Credentials::new(&[], Some([7; 32])).unwrap();
        let replica = Credentials::new(&[], Some([7; 32])).unwrap();
        let certificate = credentials.issue_ca("p/r", "box").await.unwrap();
        assert_eq!(certificate, replica.issue_ca("p/r", "box").await.unwrap());
        assert_ne!(
            certificate,
            replica.issue_ca("p/r", "different").await.unwrap()
        );
        assert_ne!(
            certificate,
            replica.issue_ca("p/other", "box").await.unwrap()
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
            ("another.example.org", true),
            ("1.1.1.1", false),
        ] {
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
    }
}
