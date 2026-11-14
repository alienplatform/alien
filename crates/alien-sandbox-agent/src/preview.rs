//! Preview: forwarding an HTTP request to a port the sandbox's own code listens on.
//!
//! Serves only Kubernetes sessions, where the agent port is reachable from the workloads that
//! link the sandbox. The caller presents a `preview` capability the session broker minted for one
//! port of this one pod. Everything else about the request is checked here, against what the
//! agent knows from its own start, not from the request:
//!
//! - the capability must verify for this session (the pod's uid) and the preview class, and name
//!   the port in the path;
//! - the port must be one the sandbox declared (`ALIEN_SANDBOX_PREVIEW_PORTS`, set by the
//!   Operator from the declared `previewPorts`), and never the agent's own port;
//! - the forward goes to `127.0.0.1` only, and the capability's `Authorization` header never
//!   reaches the code inside.
//!
//! The upstream speaks HTTP/1.0, so its response is never chunked and ends at its length or the
//! close. A WebSocket upgrade speaks HTTP/1.1 and is then copied byte for byte until the
//! capability expires.

use std::time::Duration;

use alien_core::sandbox_image::AGENT_PORT;
use axum::body::Body;
use axum::http::{header, HeaderName, HeaderValue, Request, Response, StatusCode};
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Comma separated ports the sandbox declared for preview.
pub const ENV_PREVIEW_PORTS: &str = "ALIEN_SANDBOX_PREVIEW_PORTS";

/// A request body held in memory so the upstream can be sent a length.
const MAX_REQUEST_BODY: usize = 32 * 1024 * 1024;
const MAX_RESPONSE_HEAD: usize = 64 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Declared preview ports, read from the pod's own environment.
pub fn declared_ports() -> Vec<u16> {
    std::env::var(ENV_PREVIEW_PORTS)
        .unwrap_or_default()
        .split(',')
        .filter_map(|port| port.trim().parse().ok())
        .collect()
}

/// Whether `port` may be previewed on this pod.
pub fn port_is_declared(port: u16, declared: &[u16], own_ports: &[u16]) -> bool {
    port != AGENT_PORT && !own_ports.contains(&port) && declared.contains(&port)
}

fn fail(status: StatusCode, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::from(message.to_string()))
        .expect("a static response")
}

const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Headers that belong to the cloud edge, never to the code inside. Every `x-alien-*` header is
/// the agent's and is stripped too.
const STRIPPED: [&str; 2] = ["x-sandbox-routing-token", "x-sandbox-port"];

/// How the caller's own bearer reaches the code inside a previewed port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallerAuth {
    /// `Authorization` belongs to the agent (the Kubernetes route); it is dropped.
    Drop,
    /// The caller's bearer is `Authorization` on this backend. `X-Alien-Caller-Authorization` is
    /// also accepted, but both at once is ambiguous and refused.
    Authorization,
    /// The cloud edge consumes `Authorization` (GCP), so whatever arrives there is not the
    /// caller's and is ignored. The caller's bearer is in `X-Alien-Caller-Authorization`.
    EdgeConsumesAuthorization,
}

impl CallerAuth {
    /// The mode for this sandbox, from the header its backend names for the caller's bearer.
    pub fn from_env() -> Self {
        match std::env::var(alien_core::sandbox_image::ENV_CALLER_AUTHORIZATION_HEADER) {
            Ok(name)
                if name.eq_ignore_ascii_case(
                    alien_core::sandbox_image::CALLER_AUTHORIZATION_HEADER,
                ) =>
            {
                Self::EdgeConsumesAuthorization
            }
            _ => Self::Authorization,
        }
    }
}

/// Forwards `request` to `127.0.0.1:{port}` with `path_and_query` as its target. `close_at` is
/// the unix time an upgraded stream is closed at.
///
/// `caller_auth` is the one header contract difference left between backends (see
/// [`CallerAuth`]). Whatever the mode, the caller's bearer reaches the code inside as
/// `Authorization` only, and every `x-alien-*` header and the cloud edge's routing headers are
/// stripped.
pub async fn forward(
    mut request: Request<Body>,
    port: u16,
    path_and_query: String,
    close_at: i64,
    caller_auth: CallerAuth,
    own_ports: &[u16],
) -> Response<Body> {
    // Defense in depth: this connection runs as the agent's uid, so it must never be made to
    // the agent's own listeners, whatever the caller verified before.
    if port == AGENT_PORT || own_ports.contains(&port) {
        return fail(StatusCode::FORBIDDEN, "that port is not previewable");
    }
    let caller_bearer = {
        let headers = request.headers();
        let own = headers.get(header::AUTHORIZATION);
        let caller = headers.get(alien_core::sandbox_image::CALLER_AUTHORIZATION_HEADER);
        match caller_auth {
            CallerAuth::Drop => None,
            CallerAuth::Authorization => match (own, caller) {
                (Some(_), Some(_)) => {
                    return fail(
                        StatusCode::BAD_REQUEST,
                        "send the bearer in one header, not both Authorization and X-Alien-Caller-Authorization",
                    )
                }
                (own, caller) => own.or(caller).cloned(),
            },
            CallerAuth::EdgeConsumesAuthorization => caller.cloned(),
        }
    };
    let upgrading = request
        .headers()
        .get(header::UPGRADE)
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"websocket"));
    let on_upgrade = upgrading.then(|| hyper::upgrade::on(&mut request));
    let (parts, body) = request.into_parts();
    let body = match axum::body::to_bytes(body, MAX_REQUEST_BODY).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return fail(
                StatusCode::PAYLOAD_TOO_LARGE,
                "the request body is too large",
            )
        }
    };

    let mut upstream = match tokio::time::timeout(
        Duration::from_secs(5),
        TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        _ => {
            return fail(
                StatusCode::BAD_GATEWAY,
                "nothing is listening on that port in the sandbox",
            )
        }
    };

    let version = if upgrading { "1.1" } else { "1.0" };
    let mut head = format!(
        "{} {} HTTP/{version}\r\nHost: 127.0.0.1:{port}\r\n",
        parts.method, path_and_query
    )
    .into_bytes();
    for (name, value) in &parts.headers {
        let lower = name.as_str();
        if lower == "host" || lower == "content-length" {
            continue;
        }
        if lower == header::AUTHORIZATION.as_str() || lower.starts_with("x-alien-") {
            continue;
        }
        if STRIPPED.contains(&lower) {
            continue;
        }
        if HOP_BY_HOP.contains(&lower)
            && !(upgrading && (lower == "connection" || lower == "upgrade"))
        {
            continue;
        }
        head.extend_from_slice(name.as_str().as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(value.as_bytes());
        head.extend_from_slice(b"\r\n");
    }
    if let Some(bearer) = caller_bearer {
        head.extend_from_slice(b"Authorization: ");
        head.extend_from_slice(bearer.as_bytes());
        head.extend_from_slice(b"\r\n");
    }
    if !body.is_empty() || !matches!(parts.method.as_str(), "GET" | "HEAD") {
        head.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    head.extend_from_slice(b"\r\n");
    if upstream.write_all(&head).await.is_err() || upstream.write_all(&body).await.is_err() {
        return fail(
            StatusCode::BAD_GATEWAY,
            "the sandbox's server closed the connection",
        );
    }

    // Read the response head. Interim `1xx` responses (100 Continue, 103 Early Hints) are not the
    // answer: they are skipped, and the real status and body follow them. `101` is the answer to
    // an upgrade and is kept.
    let mut buffer = Vec::with_capacity(8192);
    let (status, headers, head_len) = loop {
        if !buffer.is_empty() {
            let mut storage = [httparse::EMPTY_HEADER; 96];
            let mut parsed = httparse::Response::new(&mut storage);
            match parsed.parse(&buffer) {
                Ok(httparse::Status::Complete(len)) => {
                    let status = parsed.code.unwrap_or(502);
                    if (100..200).contains(&status) && status != 101 {
                        buffer.drain(..len);
                        continue;
                    }
                    let headers: Vec<(String, Vec<u8>)> = parsed
                        .headers
                        .iter()
                        .map(|header| (header.name.to_string(), header.value.to_vec()))
                        .collect();
                    break (status, headers, len);
                }
                Ok(httparse::Status::Partial) if buffer.len() < MAX_RESPONSE_HEAD => {}
                _ => {
                    return fail(
                        StatusCode::BAD_GATEWAY,
                        "the sandbox's server sent a bad response",
                    )
                }
            }
        }
        let mut chunk = [0u8; 8192];
        match tokio::time::timeout(HEAD_TIMEOUT, upstream.read(&mut chunk)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => {
                return fail(
                    StatusCode::BAD_GATEWAY,
                    "the sandbox's server sent no response",
                )
            }
            Ok(Ok(read)) => buffer.extend_from_slice(&chunk[..read]),
        }
    };
    let leftover = buffer.split_off(head_len);
    let mut response =
        Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY));
    let switching = status == 101;
    for (name, value) in &headers {
        let lower = name.to_ascii_lowercase();
        if HOP_BY_HOP.contains(&lower.as_str())
            && !(switching && (lower == "connection" || lower == "upgrade"))
        {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_bytes(value),
        ) {
            response = response.header(name, value);
        }
    }

    if switching {
        let Some(on_upgrade) = on_upgrade else {
            return fail(
                StatusCode::BAD_GATEWAY,
                "the sandbox's server upgraded a plain request",
            );
        };
        tokio::spawn(async move {
            let Ok(upgraded) = on_upgrade.await else {
                return;
            };
            let mut client = hyper_util::rt::TokioIo::new(upgraded);
            if !leftover.is_empty() && client.write_all(&leftover).await.is_err() {
                return;
            }
            let remaining = u64::try_from(close_at - chrono::Utc::now().timestamp()).unwrap_or(0);
            // The stream is closed when the capability expires, whichever side is still talking.
            let _ = tokio::time::timeout(
                Duration::from_secs(remaining),
                tokio::io::copy_bidirectional(&mut client, &mut upstream),
            )
            .await;
        });
        return response
            .body(Body::empty())
            .expect("a valid upgrade response");
    }

    // Only an upgrade request is sent as HTTP/1.1, so only a refused upgrade can come back
    // chunked. The chunk framing is decoded here: `Transfer-Encoding` is not forwarded, so the
    // client would otherwise read the chunk markers as body bytes.
    let chunked = headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding")
            && String::from_utf8_lossy(value)
                .to_ascii_lowercase()
                .contains("chunked")
    });
    if chunked {
        return match read_chunked(&mut upstream, leftover).await {
            Some(decoded) => response
                .header(header::CONTENT_LENGTH, decoded.len())
                .body(Body::from(decoded))
                .unwrap_or_else(|_| {
                    fail(
                        StatusCode::BAD_GATEWAY,
                        "the sandbox's server sent a bad response",
                    )
                }),
            None => fail(
                StatusCode::BAD_GATEWAY,
                "the sandbox's server sent a bad response",
            ),
        };
    }

    let first = futures::stream::iter(
        (!leftover.is_empty()).then(|| Ok::<_, std::io::Error>(bytes::Bytes::from(leftover))),
    );
    let rest = futures::stream::unfold(upstream, |mut upstream| async move {
        let mut chunk = vec![0u8; 16 * 1024];
        match upstream.read(&mut chunk).await {
            Ok(0) | Err(_) => None,
            Ok(read) => {
                chunk.truncate(read);
                Some((Ok(bytes::Bytes::from(chunk)), upstream))
            }
        }
    });
    response
        .body(Body::from_stream(first.chain(rest)))
        .unwrap_or_else(|_| {
            fail(
                StatusCode::BAD_GATEWAY,
                "the sandbox's server sent a bad response",
            )
        })
}

/// Largest chunked response body the agent decodes (a refused upgrade is a short error page).
const MAX_CHUNKED_BODY: usize = 1024 * 1024;

/// Reads a chunked body from `source`, starting with the bytes already read (`pending`), and
/// returns it decoded. `None` when the framing is invalid, the body is too large, or the stream
/// ends first.
async fn read_chunked<R: tokio::io::AsyncRead + Unpin>(
    source: &mut R,
    pending: Vec<u8>,
) -> Option<Vec<u8>> {
    let mut buffer = pending;
    let mut decoded = Vec::new();
    loop {
        // Chunk size line.
        let line_end = loop {
            if let Some(at) = buffer.windows(2).position(|pair| pair == b"\r\n") {
                break at;
            }
            if buffer.len() > 1024 {
                return None;
            }
            fill(source, &mut buffer).await?;
        };
        let size_text = std::str::from_utf8(&buffer[..line_end]).ok()?;
        let size_text = size_text.split(';').next()?.trim();
        // The size is checked against the cap before any arithmetic on it: a hostile server can
        // announce a chunk as large as a `usize`, and adding to that overflows.
        let size = usize::from_str_radix(size_text, 16).ok()?;
        buffer.drain(..line_end + 2);
        if size == 0 {
            return Some(decoded);
        }
        if size > MAX_CHUNKED_BODY || decoded.len().checked_add(size)? > MAX_CHUNKED_BODY {
            return None;
        }
        while buffer.len() < size + 2 {
            fill(source, &mut buffer).await?;
        }
        if &buffer[size..size + 2] != b"\r\n" {
            return None;
        }
        decoded.extend_from_slice(&buffer[..size]);
        buffer.drain(..size + 2);
    }
}

async fn fill<R: tokio::io::AsyncRead + Unpin>(source: &mut R, buffer: &mut Vec<u8>) -> Option<()> {
    let mut chunk = [0u8; 8192];
    match tokio::time::timeout(HEAD_TIMEOUT, source.read(&mut chunk)).await {
        Ok(Ok(read)) if read > 0 => {
            buffer.extend_from_slice(&chunk[..read]);
            Some(())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_chunked_error_body_is_decoded_before_it_is_returned() {
        // The first chunk already arrived with the head; the rest comes from the stream.
        let mut rest: &[u8] = b"lo wor\r\n3\r\nld!\r\n0\r\n\r\n";
        let decoded = read_chunked(&mut rest, b"5\r\nhel".to_vec()).await;
        assert!(
            decoded.is_none(),
            "5 bytes were announced, only 3 + rest arrive in order"
        );
        let mut stream: &[u8] = b"\r\n6\r\n world\r\n0\r\n\r\n";
        let decoded = read_chunked(&mut stream, b"5\r\nhello".to_vec()).await;
        assert_eq!(decoded.as_deref(), Some(&b"hello world"[..]));
    }

    /// An upstream that answers every connection with `reply`, then closes.
    async fn upstream_replying(reply: &'static [u8]) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request).await;
                let _ = stream.write_all(reply).await;
            }
        });
        port
    }

    /// `100 Continue` and `103 Early Hints` come before the answer; the caller gets the answer's
    /// status and body, not the interim one.
    #[tokio::test]
    async fn interim_responses_are_skipped_before_the_real_status_and_body() {
        let port = upstream_replying(
            b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 103 Early Hints\r\nLink: </a.css>\r\n\r\nHTTP/1.0 200 OK\r\nContent-Length: 5\r\n\r\nhello",
        )
        .await;
        let request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let response = forward(
            request,
            port,
            "/".to_string(),
            i64::MAX,
            CallerAuth::Drop,
            &[],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], b"hello");
    }

    /// A chunk size near `usize::MAX` is refused; it must not overflow the arithmetic that checks
    /// the cap, and a size over the cap is refused before anything is read for it.
    #[tokio::test]
    async fn an_oversized_chunk_is_refused_without_panicking() {
        for size in [
            "ffffffffffffffff",
            "fffffffffffffff0",
            "7fffffffffffffff",
            "100001",
        ] {
            let mut source: &[u8] = b"";
            let framing = format!("{size}\r\n");
            assert!(
                read_chunked(&mut source, framing.into_bytes())
                    .await
                    .is_none(),
                "{size} is refused"
            );
        }
        // The same size after a chunk was already decoded: adding it to the length overflows.
        let mut source: &[u8] = b"";
        assert!(
            read_chunked(&mut source, b"1\r\na\r\nffffffffffffffff\r\n".to_vec())
                .await
                .is_none()
        );
        // A size that fits the cap alone but not on top of what was already decoded.
        let mut source: &[u8] = b"";
        let mut first = format!("{:x}\r\n", MAX_CHUNKED_BODY - 1).into_bytes();
        first.extend(vec![b'a'; MAX_CHUNKED_BODY - 1]);
        first.extend_from_slice(b"\r\n2\r\nbb\r\n0\r\n\r\n");
        assert!(read_chunked(&mut source, first).await.is_none());
    }

    #[test]
    fn the_agents_own_port_and_undeclared_ports_are_never_previewable() {
        assert!(port_is_declared(8080, &[8080], &[]));
        assert!(!port_is_declared(8081, &[8080], &[]));
        assert!(!port_is_declared(8080, &[8080], &[8080]));
        assert!(!port_is_declared(AGENT_PORT, &[AGENT_PORT, 8080], &[]));
    }
}
