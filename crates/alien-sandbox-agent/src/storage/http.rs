//! HTTP/1.1 over the agent's own sockets, for the push transport.
//!
//! The agent runs in the guest cgroup, so a socket it opens carries the guest mark like a
//! command's, and the in-guest redirect sends it to the egress forwarder instead of the store.
//! Every connection here goes through [`crate::gateway_egress::trusted_connect`], which marks
//! the socket (and its DNS lookup) as the agent's. One connection per request: commits send a
//! few objects a second at most.

use std::{sync::Arc, time::Duration};

use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, header::HeaderMap, Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use tokio_rustls::{
    rustls::{pki_types::ServerName, ClientConfig, RootCertStore},
    TlsConnector,
};

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
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
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
