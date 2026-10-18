use std::{convert::Infallible, sync::Arc};

use alien_error::{Context, IntoAlienError};
use bytes::Bytes;
use http::{
    header::{CONNECTION, HOST, PROXY_AUTHORIZATION},
    HeaderName, HeaderValue, Request, Response, StatusCode,
};
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{body::Incoming, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::Mutex,
};

use crate::{
    config::CredentialRule,
    credentials::Credentials,
    error::{denied, invalid, unavailable, Result},
    policy::Target,
    server::HEADER_TIMEOUT,
};

type Body = BoxBody<Bytes, hyper::Error>;

/// Forward exactly one HTTP request with library-owned framing and Host validation.
/// A single request per connection avoids policy changes hidden inside keepalive tunnels.
pub async fn forward<G, U>(
    guest: G,
    upstream: U,
    target: Target,
    injection: Option<(&Credentials, &[CredentialRule])>,
) -> Result<()>
where
    G: AsyncRead + AsyncWrite + Unpin + Send,
    U: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(upstream))
        .await
        .into_alien_error()
        .context(unavailable("initialize upstream HTTP"))?;
    let driver = tokio::spawn(connection);
    let sender = Arc::new(Mutex::new(sender));
    let failure = Arc::new(Mutex::new(None));
    let service_failure = failure.clone();
    let service = service_fn(move |mut request: Request<Incoming>| {
        let sender = sender.clone();
        let target = target.clone();
        let failure = service_failure.clone();
        async move {
            if let Err(error) = prepare(&mut request, &target, injection.is_some()) {
                *failure.lock().await = Some(error);
                return Ok::<_, Infallible>(empty(StatusCode::FORBIDDEN));
            }
            if let Some((credentials, rules)) = injection {
                if let Err(error) = credentials.inject(&mut request, rules).await {
                    *failure.lock().await = Some(error);
                    return Ok(empty(StatusCode::BAD_GATEWAY));
                }
            }
            let response = match sender.lock().await.send_request(request).await {
                Ok(response) => response.map(BodyExt::boxed),
                Err(_) => {
                    *failure.lock().await = Some(alien_error::AlienError::new(unavailable(
                        "upstream HTTP request",
                    )));
                    empty(StatusCode::BAD_GATEWAY)
                }
            };
            Ok(response)
        }
    });
    let result = hyper::server::conn::http1::Builder::new()
        .keep_alive(false)
        .max_buf_size(crate::protocol::MAX_HEADER_BYTES)
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT)
        .serve_connection(TokioIo::new(guest), service)
        .await;
    driver.abort();
    if let Some(error) = failure.lock().await.take() {
        return Err(error);
    }
    result
        .into_alien_error()
        .context(unavailable("serve proxied HTTP"))
}

fn empty(status: StatusCode) -> Response<Body> {
    let mut response = Response::new(
        Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed(),
    );
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONNECTION, HeaderValue::from_static("close"));
    response
}

fn prepare(request: &mut Request<Incoming>, target: &Target, injection: bool) -> Result<()> {
    let headers = request.headers();
    if headers.get_all(HOST).iter().count() != 1 {
        return Err(invalid("exactly one Host required"));
    }
    let host = headers
        .get(HOST)
        .ok_or_else(|| invalid("Host required"))?
        .to_str()
        .map_err(|_| invalid("invalid Host"))?;
    if Target::parse(host, Some(target.port))? != *target {
        return Err(denied("Host mismatch"));
    }
    if let Some(authority) = request.uri().authority() {
        if Target::parse(authority.as_str(), Some(target.port))? != *target {
            return Err(denied("URI authority mismatch"));
        }
    }
    if request.method() == http::Method::CONNECT || headers.contains_key("upgrade") {
        return Err(denied("nested tunnel or upgrade"));
    }
    // Normalize hop-by-hop fields; do this before injection so a guest cannot remove it via Connection.
    let connection_headers = request
        .headers()
        .get_all(CONNECTION)
        .iter()
        .map(|v| {
            v.to_str()
                .map(str::to_owned)
                .map_err(|_| invalid("invalid Connection"))
        })
        .collect::<Result<Vec<_>>>()?;
    for line in connection_headers {
        for name in line.split(',') {
            let name = HeaderName::from_bytes(name.trim().as_bytes())
                .map_err(|_| invalid("invalid connection field"))?;
            // Framing and routing fields must not be removed after validation.
            if ["host", "content-length", "transfer-encoding"].contains(&name.as_str()) {
                return Err(invalid("connection names a framing field"));
            }
            request.headers_mut().remove(name);
        }
    }
    for name in [
        "proxy-connection",
        "proxy-authenticate",
        "keep-alive",
        "te",
        "trailer",
        "upgrade",
    ] {
        request.headers_mut().remove(name);
    }
    request.headers_mut().remove(PROXY_AUTHORIZATION);
    request
        .headers_mut()
        .insert(CONNECTION, HeaderValue::from_static("close"));
    if injection {
        let path = request.uri().path();
        if path.contains(['%', '\\']) || path.split('/').any(|part| part == "." || part == "..") {
            return Err(denied("ambiguous credential path"));
        }
    }
    let path = request
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/");
    *request.uri_mut() = path.parse().map_err(|_| invalid("invalid path"))?;
    Ok(())
}
