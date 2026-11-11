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
pub fn port_is_declared(port: u16, declared: &[u16]) -> bool {
    port != AGENT_PORT && declared.contains(&port)
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

/// Forwards `request` to `127.0.0.1:{port}` with `path_and_query` as its target. `close_at` is
/// the unix time an upgraded stream is closed at.
pub async fn forward(
    mut request: Request<Body>,
    port: u16,
    path_and_query: String,
    close_at: i64,
) -> Response<Body> {
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
        if lower == "host" || lower == header::AUTHORIZATION.as_str() || lower == "content-length" {
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

    // Read the response head.
    let mut buffer = Vec::with_capacity(8192);
    let (status, headers, head_len) = loop {
        let mut chunk = [0u8; 8192];
        let read = match tokio::time::timeout(HEAD_TIMEOUT, upstream.read(&mut chunk)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => {
                return fail(
                    StatusCode::BAD_GATEWAY,
                    "the sandbox's server sent no response",
                )
            }
            Ok(Ok(read)) => read,
        };
        buffer.extend_from_slice(&chunk[..read]);
        let mut storage = [httparse::EMPTY_HEADER; 96];
        let mut parsed = httparse::Response::new(&mut storage);
        match parsed.parse(&buffer) {
            Ok(httparse::Status::Complete(len)) => {
                let status = parsed.code.unwrap_or(502);
                let headers: Vec<(String, Vec<u8>)> = parsed
                    .headers
                    .iter()
                    .map(|header| (header.name.to_string(), header.value.to_vec()))
                    .collect();
                break (status, headers, len);
            }
            Ok(httparse::Status::Partial) if buffer.len() < MAX_RESPONSE_HEAD => continue,
            _ => {
                return fail(
                    StatusCode::BAD_GATEWAY,
                    "the sandbox's server sent a bad response",
                )
            }
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
        let size = usize::from_str_radix(size_text, 16).ok()?;
        buffer.drain(..line_end + 2);
        if size == 0 {
            return Some(decoded);
        }
        if decoded.len() + size > MAX_CHUNKED_BODY {
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

    #[test]
    fn the_agents_own_port_and_undeclared_ports_are_never_previewable() {
        assert!(port_is_declared(8080, &[8080]));
        assert!(!port_is_declared(8081, &[8080]));
        assert!(!port_is_declared(AGENT_PORT, &[AGENT_PORT, 8080]));
    }
}
