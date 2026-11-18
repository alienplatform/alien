//! HTTP/1.1 over the agent's own sockets, for the push transport.
//!
//! The agent runs in the guest cgroup, so a socket it opens carries the guest mark like a
//! command's, and the in-guest redirect sends it to the egress forwarder instead of the store.
//! Every connection here goes through [`crate::gateway_egress::trusted_connect`], which marks
//! the socket (and its DNS lookup) as the agent's. One connection per request: commits send a
//! few objects a second at most.
//!
//! Trust: the bundled webpki roots, plus the root the platform's egress proxy signs with when
//! the binding names one (`ALIEN_SANDBOX_STORE_TRUST_ROOTS`, Azure). Azure's proxy terminates
//! every HTTPS connection under an enforcing policy, the store's included, and no policy option
//! exempts a host (evidence in the Azure persistent storage PR). The file is read once at boot,
//! before any command runs, from a descriptor whose owner and mode are checked: only root may
//! have been able to write it or replace it.

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

/// The platform's extra roots, read at boot. Empty when the binding names none.
static PLATFORM_ROOTS: OnceLock<Vec<CertificateDer<'static>>> = OnceLock::new();

/// Reads the roots `ALIEN_SANDBOX_STORE_TRUST_ROOTS` names, once; returns how many. Call at
/// boot, before the workload starts. Refused when the file, or its directory, is not owned by
/// root or is writable by anyone else, or holds no certificate.
pub fn load_platform_roots_from_env() -> std::io::Result<usize> {
    let roots = match std::env::var_os(alien_core::ENV_ALIEN_SANDBOX_STORE_TRUST_ROOTS) {
        None => Vec::new(),
        Some(path) => read_root_owned(std::path::Path::new(&path), 0)
            .and_then(|pem| parse_bundle(&pem))
            .map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!(
                        "{} {}: {error}",
                        alien_core::ENV_ALIEN_SANDBOX_STORE_TRUST_ROOTS,
                        path.to_string_lossy()
                    ),
                )
            })?,
    };
    let count = roots.len();
    let _ = PLATFORM_ROOTS.set(roots);
    Ok(PLATFORM_ROOTS.get().map_or(count, Vec::len))
}

/// The file's bytes, when it is an absolute path to a regular file owned by `owner` that no one
/// else can write, in a directory with the same property. Ownership is checked on the open
/// descriptor the bytes are read from.
fn read_root_owned(path: &std::path::Path, owner: u32) -> std::io::Result<Vec<u8>> {
    use std::{io::Read, os::unix::fs::MetadataExt};
    let writable_by_others =
        |meta: &std::fs::Metadata| meta.uid() != owner || meta.mode() & 0o022 != 0;
    if !path.is_absolute() {
        return Err(std::io::Error::other("not an absolute path"));
    }
    let directory = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no directory"))?;
    if writable_by_others(&std::fs::metadata(directory)?) {
        return Err(std::io::Error::other(
            "its directory may be written by a user other than root",
        ));
    }
    let mut file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || writable_by_others(&meta) {
        return Err(std::io::Error::other(
            "not a regular file only root may write",
        ));
    }
    let mut pem = Vec::new();
    file.read_to_end(&mut pem)?;
    Ok(pem)
}

fn parse_bundle(pem: &[u8]) -> std::io::Result<Vec<CertificateDer<'static>>> {
    let roots = CertificateDer::pem_slice_iter(pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(std::io::Error::other)?;
    if roots.is_empty() {
        return Err(std::io::Error::other("holds no certificate"));
    }
    Ok(roots)
}

fn root_store(extra: &[CertificateDer<'static>]) -> RootCertStore {
    let mut roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    // A certificate rustls cannot use as a trust anchor is skipped; the rest stay.
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

    /// Only a file and directory the owner alone may write are read. The test uses its own uid
    /// as the owner; the agent passes root's.
    #[test]
    fn roots_are_read_only_from_a_file_no_one_else_can_write() {
        use std::os::unix::fs::PermissionsExt;
        let me = unsafe { libc::geteuid() };
        let directory = tempfile::tempdir().unwrap();
        let set = |path: &std::path::Path, mode: u32| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap()
        };
        set(directory.path(), 0o755);
        let file = directory.path().join("proxy-ca.crt");
        std::fs::write(&file, b"pem").unwrap();
        set(&file, 0o644);
        assert_eq!(read_root_owned(&file, me).unwrap(), b"pem");

        // Another owner, a file others may write, a directory others may write, a relative
        // path, a directory in its place.
        assert!(read_root_owned(&file, me + 1).is_err());
        set(&file, 0o664);
        assert!(read_root_owned(&file, me).is_err());
        set(&file, 0o644);
        set(directory.path(), 0o777);
        assert!(read_root_owned(&file, me).is_err());
        set(directory.path(), 0o755);
        assert!(read_root_owned(std::path::Path::new("proxy-ca.crt"), me).is_err());
        assert!(read_root_owned(directory.path(), me).is_err());
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
