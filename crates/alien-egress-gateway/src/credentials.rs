use std::sync::Arc;

use alien_bindings::{traits::PutOptions, Bindings, Kv, PutCondition};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http::{HeaderName, HeaderValue};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::{
    pki_types::{PrivatePkcs8KeyDer, ServerName},
    ClientConfig, RootCertStore, ServerConfig,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::{
    config::CredentialRule,
    error::{invalid, unavailable, Result},
    http_proxy::{self, Injection},
    policy::{normalize_host, Target},
    server::HEADER_TIMEOUT,
};

/// Gateway-only credential access and durable per-sandbox certificate authorities.
pub struct Credentials {
    kv: Arc<dyn Kv>,
    bindings: Bindings,
    upstream: Arc<ClientConfig>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ca {
    certificate: String,
    private_key: String,
}

impl Credentials {
    /// Use the deployment KV for CA material and deployment vault bindings for header values.
    pub fn new(kv: Arc<dyn Kv>, bindings: Bindings, additional_roots: &[String]) -> Result<Self> {
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
        let mut config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Self {
            kv,
            bindings,
            upstream: Arc::new(config),
        })
    }

    /// Return a sandbox-specific public trust certificate; its private key stays in shared KV.
    /// Creation is atomic across replicas. The admin caller retries a concurrent-create conflict.
    pub async fn issue_ca(&self, aud: &str, sub: &str) -> Result<String> {
        if let Some(ca) = self.load_ca(aud, sub).await? {
            return Ok(ca.certificate);
        }
        let key = KeyPair::generate()
            .into_alien_error()
            .context(unavailable("generate sandbox CA"))?;
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params
            .distinguished_name
            .push(DnType::CommonName, "Sandbox egress CA");
        let cert = params
            .self_signed(&key)
            .into_alien_error()
            .context(unavailable("sign sandbox CA"))?;
        let ca = Ca {
            certificate: cert.pem(),
            private_key: key.serialize_pem(),
        };
        let value = serde_json::to_vec(&ca)
            .into_alien_error()
            .context(unavailable("encode sandbox CA"))?;
        if !self
            .kv
            .put(
                &ca_key(aud, sub),
                value,
                Some(PutOptions {
                    ttl: None,
                    condition: PutCondition::Absent,
                }),
            )
            .await
            .context(unavailable("store sandbox CA"))?
        {
            return Err(AlienError::new(unavailable(
                "sandbox CA created concurrently; repeat request",
            )));
        }
        Ok(ca.certificate)
    }

    async fn load_ca(&self, aud: &str, sub: &str) -> Result<Option<Ca>> {
        self.kv
            .get(&ca_key(aud, sub))
            .await
            .context(unavailable("load sandbox CA"))?
            .map(|entry| {
                serde_json::from_slice(&entry.value)
                    .into_alien_error()
                    .context(unavailable("decode sandbox CA"))
            })
            .transpose()
    }

    async fn server_config(
        &self,
        aud: &str,
        sub: &str,
        target: &Target,
    ) -> Result<Arc<ServerConfig>> {
        let ca = self
            .load_ca(aud, sub)
            .await?
            .ok_or_else(|| invalid("sandbox CA has not been issued"))?;
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
        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .into_alien_error()
            .context(unavailable("configure guest TLS"))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
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
        let mut headers = Vec::new();
        for rule in rules {
            let vault = self
                .bindings
                .vault(&rule.vault_binding)
                .await
                .context(unavailable("load credential vault"))?;
            let secret = vault
                .get_secret(&rule.secret)
                .await
                .context(unavailable("read injection credential"))?;
            let value = HeaderValue::from_str(&format!("{}{}", rule.value_prefix, secret))
                .map_err(|_| invalid("credential is not a valid HTTP header value"))?;
            headers.push(Injection {
                method: rule.method.clone(),
                path: rule.path.clone(),
                name: HeaderName::from_bytes(rule.header.as_bytes())
                    .map_err(|_| invalid("invalid injection header"))?,
                value,
            });
        }
        http_proxy::forward(guest, upstream, target, Some(Arc::new(headers))).await
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

fn ca_key(aud: &str, sub: &str) -> String {
    let mut hash = Sha256::new();
    hash.update((aud.len() as u64).to_be_bytes());
    hash.update(aud.as_bytes());
    hash.update(sub.as_bytes());
    format!("egress-ca:{}", URL_SAFE_NO_PAD.encode(hash.finalize()))
}
