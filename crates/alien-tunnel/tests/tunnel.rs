//! End-to-end tunnel test with real sockets: an HTTP service, the manager end
//! accepting WebSockets on a TCP listener, and the operator end dialing it
//! with the production connect path (HTTP/1.1 upgrade via reqwest).

use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use alien_tunnel::{
    manager::{TunnelBody, TunnelRegistry},
    operator::{self, TunnelClientConfig, TunnelTargets},
    websocket_io, ERROR_HEADER, SUBPROTOCOL,
};
use bytes::Bytes;
use futures::{stream, StreamExt as _, TryStreamExt as _};
use http::{Request, Response, StatusCode, Uri};
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::{body::Frame, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use sha2::{Digest, Sha256};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::tungstenite::{
    handshake::server::{ErrorResponse, Request as WsRequest, Response as WsResponse},
    http::HeaderValue,
};

const TOKEN: &str = "ax_deploy_test";
const DEPLOYMENT: &str = "dep_test";
const CHUNK: usize = 64 * 1024;

/// In-cluster service: echoes request metadata, hashes uploads, streams downloads.
async fn start_service() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let service = service_fn(|req: Request<hyper::body::Incoming>| async move {
                    Ok::<_, Infallible>(handle_service(req).await)
                });
                http1::Builder::new()
                    .serve_connection(TokioIo::new(socket), service)
                    .await
                    .unwrap();
            });
        }
    });
    addr
}

async fn handle_service(
    req: Request<hyper::body::Incoming>,
) -> Response<http_body_util::combinators::BoxBody<Bytes, Infallible>> {
    let path = req.uri().path().to_string();
    if path == "/upload" {
        let mut hasher = Sha256::new();
        let mut len = 0usize;
        let mut body = req.into_body();
        while let Some(frame) = body.frame().await {
            if let Ok(data) = frame.unwrap().into_data() {
                len += data.len();
                hasher.update(&data);
            }
        }
        let text = format!("{len} {}", hex::encode(hasher.finalize()));
        return Response::new(Full::new(Bytes::from(text)).boxed());
    }
    if let Some(size) = path.strip_prefix("/download/") {
        let size: usize = size.parse().unwrap();
        let chunks = stream::iter((0..size.div_ceil(CHUNK)).map(move |i| {
            let n = CHUNK.min(size - i * CHUNK);
            Ok::<_, Infallible>(Frame::data(Bytes::from(pattern(i, n))))
        }));
        return Response::new(BodyExt::boxed(StreamBody::new(chunks)));
    }
    let echo = serde_json::json!({
        "method": req.method().as_str(),
        "path": req.uri().path_and_query().map(|p| p.as_str()),
        "authorization": req.headers().get("authorization").and_then(|v| v.to_str().ok()),
        "host": req.headers().get("host").and_then(|v| v.to_str().ok()),
    });
    Response::new(Full::new(Bytes::from(echo.to_string())).boxed())
}

fn pattern(chunk_index: usize, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((chunk_index * 31 + i) % 251) as u8)
        .collect()
}

/// Manager end: accepts tunnel WebSockets and serves them into the registry.
/// Returns the handle of the accept loop so tests can drop live connections.
async fn start_manager(registry: Arc<TunnelRegistry>, listener: TcpListener) -> JoinHandle<()> {
    tokio::spawn(async move {
        // Dropping the set (when this task is aborted) closes every connection.
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let registry = registry.clone();
            connections.spawn(async move {
                let ws = tokio_tungstenite::accept_hdr_async(
                    socket,
                    |req: &WsRequest, mut resp: WsResponse| -> Result<WsResponse, ErrorResponse> {
                        let authorized = req.headers().get("authorization")
                            == Some(&HeaderValue::from_str(&format!("Bearer {TOKEN}")).unwrap());
                        assert!(authorized, "operator must present its deployment token");
                        assert_eq!(req.uri().path(), "/v1/tunnel/connect");
                        resp.headers_mut().insert(
                            "sec-websocket-protocol",
                            HeaderValue::from_static(SUBPROTOCOL),
                        );
                        Ok(resp)
                    },
                )
                .await
                .unwrap();
                // Ends with an error when the connection is dropped; that's expected here.
                let _closed = registry
                    .serve(DEPLOYMENT.to_string(), websocket_io(ws))
                    .await;
            });
        }
    })
}

async fn wait_for_connections(registry: &TunnelRegistry, n: usize) {
    for _ in 0..200 {
        if registry.connection_count(DEPLOYMENT) >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "expected {n} tunnel connections, have {}",
        registry.connection_count(DEPLOYMENT)
    );
}

fn request(method: &str, target_and_path: &str, body: TunnelBody) -> Request<TunnelBody> {
    Request::builder()
        .method(method)
        .uri(format!("http://{target_and_path}"))
        .version(http::Version::HTTP_2)
        .header("authorization", "Bearer app-level-token")
        .body(body)
        .unwrap()
}

fn empty() -> TunnelBody {
    Full::new(Bytes::new())
        .map_err(|never: Infallible| match never {})
        .boxed_unsync()
}

struct Harness {
    registry: Arc<TunnelRegistry>,
    manager_addr: SocketAddr,
    manager: JoinHandle<()>,
    targets: Arc<TunnelTargets>,
    _operator: JoinHandle<alien_tunnel::Result<()>>,
}

async fn harness() -> Harness {
    let service = start_service().await;
    let registry = TunnelRegistry::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let manager_addr = listener.local_addr().unwrap();
    let manager = start_manager(registry.clone(), listener).await;

    let targets = TunnelTargets::new();
    targets.replace(HashMap::from([(
        "api".to_string(),
        format!("http://{service}").parse::<Uri>().unwrap(),
    )]));
    let operator = tokio::spawn(operator::run(
        TunnelClientConfig {
            manager_url: format!("http://{manager_addr}").parse().unwrap(),
            token: TOKEN.to_string(),
            connections: 2,
        },
        targets.clone(),
    ));
    wait_for_connections(&registry, 2).await;
    Harness {
        registry,
        manager_addr,
        manager,
        targets,
        _operator: operator,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forwards_requests_with_the_callers_own_headers() {
    let h = harness().await;
    let response = h
        .registry
        .send(DEPLOYMENT, request("PUT", "api/repos/a/b?x=1", empty()))
        .await
        .expect("request should reach the service");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(ERROR_HEADER).is_none());
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let echo: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(echo["method"], "PUT");
    assert_eq!(echo["path"], "/repos/a/b?x=1");
    assert_eq!(echo["authorization"], "Bearer app-level-token");
    assert!(
        echo["host"].as_str().unwrap().starts_with("127.0.0.1:"),
        "Host must be the service address, got {}",
        echo["host"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_large_bodies_in_both_directions() {
    let h = harness().await;
    const SIZE: usize = 256 * 1024 * 1024;

    // Upload: generated chunk by chunk, never held in memory as a whole.
    let mut expected = Sha256::new();
    for i in 0..SIZE / CHUNK {
        expected.update(pattern(i, CHUNK));
    }
    let upload = StreamBody::new(stream::iter((0..SIZE / CHUNK).map(|i| {
        Ok::<_, alien_tunnel::manager::BoxError>(Frame::data(Bytes::from(pattern(i, CHUNK))))
    })))
    .boxed_unsync();
    let response = h
        .registry
        .send(DEPLOYMENT, request("POST", "api/upload", upload))
        .await
        .expect("upload should be accepted");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        String::from_utf8(body.to_vec()).unwrap(),
        format!("{SIZE} {}", hex::encode(expected.finalize())),
        "service must receive every byte in order"
    );

    // Download: consumed incrementally.
    let response = h
        .registry
        .send(
            DEPLOYMENT,
            request("GET", &format!("api/download/{SIZE}"), empty()),
        )
        .await
        .expect("download should start");
    assert_eq!(response.status(), StatusCode::OK);
    let mut received = Sha256::new();
    let mut len = 0usize;
    let mut stream = response.into_body().into_data_stream();
    while let Some(chunk) = stream.try_next().await.unwrap() {
        len += chunk.len();
        received.update(&chunk);
    }
    let mut expected = Sha256::new();
    for i in 0..SIZE / CHUNK {
        expected.update(pattern(i, CHUNK));
    }
    assert_eq!(len, SIZE);
    assert_eq!(received.finalize(), expected.finalize());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serves_many_concurrent_requests() {
    let h = harness().await;
    let ok = Arc::new(AtomicUsize::new(0));
    stream::iter(0..500)
        // 100 in flight: macOS caps the test service's accept backlog at 128.
        .for_each_concurrent(100, |i| {
            let registry = h.registry.clone();
            let ok = ok.clone();
            async move {
                let response = registry
                    .send(
                        DEPLOYMENT,
                        request("GET", &format!("api/item/{i}"), empty()),
                    )
                    .await
                    .expect("concurrent request should succeed");
                let status = response.status();
                let body = response.into_body().collect().await.unwrap().to_bytes();
                assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
                let echo: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(echo["path"], format!("/item/{i}"));
                ok.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
    assert_eq!(ok.load(Ordering::Relaxed), 500);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_targets_the_stack_does_not_declare() {
    let h = harness().await;
    let response = h
        .registry
        .send(DEPLOYMENT, request("GET", "kube-apiserver/api", empty()))
        .await
        .expect("the tunnel itself should answer");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get(ERROR_HEADER).unwrap(),
        "TUNNEL_TARGET_NOT_FOUND"
    );

    // Removing a target makes it unreachable immediately.
    h.targets.replace(HashMap::new());
    let response = h
        .registry
        .send(DEPLOYMENT, request("GET", "api/x", empty()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reports_an_unavailable_service_as_a_tunnel_error() {
    let h = harness().await;
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead = closed.local_addr().unwrap();
    drop(closed);
    h.targets.replace(HashMap::from([(
        "api".to_string(),
        format!("http://{dead}").parse::<Uri>().unwrap(),
    )]));
    let response = h
        .registry
        .send(DEPLOYMENT, request("GET", "api/x", empty()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        response.headers().get(ERROR_HEADER).unwrap(),
        "TUNNEL_UPSTREAM_UNAVAILABLE"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reconnects_after_the_manager_restarts() {
    let h = harness().await;

    // Manager goes away: listener and every live connection.
    h.manager.abort();
    let _ = h.manager.await;
    for _ in 0..200 {
        if h.registry.connection_count(DEPLOYMENT) == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let error = h
        .registry
        .send(DEPLOYMENT, request("GET", "api/x", empty()))
        .await
        .expect_err("no connection while the manager is down");
    assert_eq!(error.code, "TUNNEL_NOT_CONNECTED");

    // Manager comes back on the same address; the operator reconnects.
    let listener = TcpListener::bind(h.manager_addr).await.unwrap();
    let _manager = start_manager(h.registry.clone(), listener).await;
    wait_for_connections(&h.registry, 2).await;
    let response = h
        .registry
        .send(DEPLOYMENT, request("GET", "api/after-restart", empty()))
        .await
        .expect("request after reconnect should succeed");
    assert_eq!(response.status(), StatusCode::OK);
}
