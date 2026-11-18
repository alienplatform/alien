//! HTTP/1.1 over the agent's own sockets, for the push transport.
//!
//! The agent runs in the guest cgroup, so a socket it opens carries the guest mark like a
//! command's, and the in-guest redirect sends it to the egress forwarder instead of the store.
//! Every connection here goes through [`crate::gateway_egress::trusted_connect`], which marks
//! the socket (and its DNS lookup) as the agent's. One connection per request: commits send a
//! few objects a second at most.
//!
//! Trust: the bundled webpki roots, plus the bundle the platform names in `SSL_CERT_FILE` when
//! the agent starts. Azure's sandbox egress proxy inspects TLS (`trafficInspection: Full`, which
//! host rules need) and re-signs every connection, store included, with a CA it hands the guest
//! that way. The bundle is read once at boot, before any command runs, so a command cannot
//! change what the agent trusts afterwards.

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, header::HeaderMap, Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use tokio_rustls::{
    rustls::{
        pki_types::{pem::PemObject, CertificateDer, ServerName},
        ClientConfig, RootCertStore,
    },
    TlsConnector,
};

/// The platform's extra roots, read at boot. Empty when it names none.
static PLATFORM_ROOTS: OnceLock<Vec<CertificateDer<'static>>> = OnceLock::new();

/// Reads the bundle `SSL_CERT_FILE` names, once. Call at boot, before the workload starts.
/// An error when the variable is set but the file cannot be read or holds no certificate (the
/// caller logs it; the bundled roots still apply).
pub fn load_platform_roots_from_env() -> std::io::Result<usize> {
    let roots = match std::env::var_os("SSL_CERT_FILE") {
        None => Vec::new(),
        Some(path) => parse_bundle(&std::fs::read(&path).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!(
                    "cannot read SSL_CERT_FILE {}: {error}",
                    path.to_string_lossy()
                ),
            )
        })?)?,
    };
    let count = roots.len();
    let _ = PLATFORM_ROOTS.set(roots);
    Ok(PLATFORM_ROOTS.get().map_or(count, Vec::len))
}

fn parse_bundle(pem: &[u8]) -> std::io::Result<Vec<CertificateDer<'static>>> {
    let roots = CertificateDer::pem_slice_iter(pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| std::io::Error::other(format!("SSL_CERT_FILE: {error}")))?;
    if roots.is_empty() {
        return Err(std::io::Error::other("SSL_CERT_FILE holds no certificate"));
    }
    Ok(roots)
}

fn root_store(extra: &[CertificateDer<'static>]) -> RootCertStore {
    let mut roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    // A bundle (often the whole system store) can carry a certificate rustls cannot use as a
    // trust anchor; skip that one, keep the rest.
    let _ = roots.add_parsable_certificates(extra.iter().cloned());
    roots
}

/// A response read to its end.
pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

/// Sends requests over trusted sockets.
#[derive(Clone)]
pub struct TrustedHttp {
    tls: TlsConnector,
    timeout: Duration,
}

impl TrustedHttp {
    pub fn new(timeout: Duration) -> Self {
        Self::with_roots(
            timeout,
            root_store(PLATFORM_ROOTS.get().map_or(&[], Vec::as_slice)),
        )
    }

    fn with_roots(timeout: Duration, roots: RootCertStore) -> Self {
        let config = ClientConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
        Self {
            tls: TlsConnector::from(Arc::new(config)),
            timeout,
        }
    }

    /// Sends `method url` with `headers` and `body`, and reads the whole response.
    pub async fn send(
        &self,
        method: Method,
        url: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> std::io::Result<Response> {
        tokio::time::timeout(self.timeout, self.exchange(method, url, headers, body))
            .await
            .map_err(|_| {
                std::io::Error::other(format!(
                    "request to the store timed out after {:?}",
                    self.timeout
                ))
            })?
    }

    async fn exchange(
        &self,
        method: Method,
        url: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> std::io::Result<Response> {
        let url = reqwest::Url::parse(url).map_err(std::io::Error::other)?;
        let host = url
            .host_str()
            .ok_or_else(|| std::io::Error::other("the store URL has no host"))?
            .to_string();
        let https = match url.scheme() {
            "https" => true,
            "http" => false,
            other => return Err(std::io::Error::other(format!("unsupported scheme {other}"))),
        };
        let port = url
            .port_or_known_default()
            .ok_or_else(|| std::io::Error::other("the store URL has no port"))?;
        let mut target = url.path().to_string();
        if let Some(query) = url.query() {
            target.push('?');
            target.push_str(query);
        }
        let authority = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.clone(),
        };
        let mut request = Request::builder()
            .method(method)
            .uri(target)
            .header(hyper::header::HOST, authority);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let request = request
            .body(Full::new(Bytes::from(body)))
            .map_err(std::io::Error::other)?;

        let stream = crate::gateway_egress::trusted_connect(&host, port).await?;
        if https {
            let name = ServerName::try_from(host).map_err(std::io::Error::other)?;
            let stream = self.tls.connect(name, stream).await?;
            exchange(TokioIo::new(stream), request).await
        } else {
            exchange(TokioIo::new(stream), request).await
        }
    }
}

async fn exchange<T>(io: T, request: Request<Full<Bytes>>) -> std::io::Result<Response>
where
    T: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    let (mut sender, connection) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(std::io::Error::other)?;
    let driver = tokio::spawn(connection);
    let response = sender
        .send_request(request)
        .await
        .map_err(std::io::Error::other)?;
    let (parts, body) = response.into_parts();
    let body = body
        .collect()
        .await
        .map_err(std::io::Error::other)?
        .to_bytes();
    driver.abort();
    Ok(Response {
        status: parts.status,
        headers: parts.headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A TLS server whose certificate is signed by a private CA, the way Azure's egress proxy
    /// re-signs the store: the bundled roots alone refuse it; with the CA as a platform root the
    /// exchange completes.
    #[tokio::test]
    async fn trusts_the_platform_bundle_and_only_with_it() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_rustls::rustls::{pki_types::PrivateKeyDer, ServerConfig};

        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let ca_issuer = rcgen::Issuer::new(ca_params, ca_key);
        let leaf_key = rcgen::KeyPair::generate().unwrap();
        let leaf = rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()])
            .unwrap()
            .signed_by(&leaf_key, &ca_issuer)
            .unwrap();

        let server = ServerConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivateKeyDer::try_from(leaf_key.serialize_der()).unwrap(),
        )
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(socket).await else {
                        return;
                    };
                    let mut buffer = [0u8; 1024];
                    let _ = tls.read(&mut buffer).await;
                    let _ = tls
                        .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                        .await;
                    let _ = tls.shutdown().await;
                });
            }
        });
        let url = format!("https://127.0.0.1:{port}/keys/k1/HEAD");

        let refused = TrustedHttp::new(Duration::from_secs(10))
            .send(Method::GET, &url, &[], Vec::new())
            .await
            .err()
            .expect("the bundled roots alone do not trust the private CA");
        assert!(refused.to_string().contains("UnknownIssuer"), "{refused}");

        let bundle = parse_bundle(ca.pem().as_bytes()).unwrap();
        let response = TrustedHttp::with_roots(Duration::from_secs(10), root_store(&bundle))
            .send(Method::GET, &url, &[], Vec::new())
            .await
            .expect("trusted with the platform bundle");
        assert_eq!(response.status, StatusCode::NO_CONTENT);
    }

    #[test]
    fn a_bundle_without_certificates_is_refused() {
        assert!(parse_bundle(b"").is_err());
        assert!(parse_bundle(b"not a certificate\n").is_err());
    }

    /// TLS to a real S3 endpoint with the bundled roots: any HTTP answer proves the handshake
    /// and the exchange.
    #[tokio::test]
    #[ignore = "needs the network"]
    async fn reaches_s3_over_tls() {
        let response = TrustedHttp::new(Duration::from_secs(20))
            .send(
                Method::GET,
                "https://s3.us-east-2.amazonaws.com/alien-no-such-bucket-v3035/x?list-type=2",
                &[],
                Vec::new(),
            )
            .await
            .expect("an answer");
        assert!(response.status.is_client_error(), "{}", response.status);
        assert!(String::from_utf8_lossy(&response.body).contains("<Code>"));
    }
}
