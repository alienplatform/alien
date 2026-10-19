use std::{convert::Infallible, future::Future, sync::Arc, time::Duration};

#[cfg(all(feature = "gcp", feature = "azure"))]
use alien_worker_protocol::ControlGrpcServer;
use axum::{
    body::{Body, Bytes},
    http::{Request, Response, StatusCode},
    routing::any,
    Router,
};
use http_body_util::BodyExt;
#[cfg(all(feature = "gcp", feature = "azure"))]
use tokio::sync::broadcast;
use tokio::{net::TcpListener, sync::mpsc, task::JoinHandle, time::timeout};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use super::{create_forward_client, forward_http_request};

struct Server {
    port: u16,
    task: JoinHandle<()>,
}

impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { port, task }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// What a transport handed back to its caller for one forwarded request.
pub(crate) struct Forwarded {
    pub status: u16,
    pub location: Option<String>,
    pub cookies: Vec<String>,
    pub body: Bytes,
}

impl Forwarded {
    async fn from_response(response: Response<Body>) -> Self {
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get("location")
            .map(|v| v.to_str().unwrap().to_string());
        let cookies = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        Self {
            status,
            location,
            cookies,
            body,
        }
    }
}

/// Every redirect status, with a relative and an external Location, for GET and POST.
/// `forward(method, app_port)` must send `method /redirect?keep=query` with the body
/// `original request body` through the transport under test to the app on `app_port`.
/// The transport must hand the redirect back unchanged and never contact its target.
pub(crate) async fn assert_application_redirects_are_returned<F, Fut>(forward: F)
where
    F: Fn(&'static str, u16) -> Fut,
    Fut: Future<Output = Forwarded>,
{
    let (destination_tx, mut destination_rx) = mpsc::unbounded_channel();
    let destination = Server::start(Router::new().fallback(any(move || {
        let tx = destination_tx.clone();
        async move {
            tx.send(()).unwrap();
            "unexpected redirect follow"
        }
    })))
    .await;
    let mut failures = Vec::new();
    for status in [301, 302, 303, 307, 308] {
        for location in [
            "/destination".to_string(),
            format!("http://127.0.0.1:{}/destination", destination.port),
        ] {
            for method in ["GET", "POST"] {
                let (seen_tx, mut seen_rx) = mpsc::unbounded_channel();
                let expected_location = location.clone();
                let app =
                    Server::start(Router::new().fallback(any(move |request: Request<Body>| {
                        let tx = seen_tx.clone();
                        let location = expected_location.clone();
                        async move {
                            let (parts, body) = request.into_parts();
                            tx.send((
                                parts.method,
                                parts.uri.clone(),
                                body.collect().await.unwrap().to_bytes(),
                            ))
                            .unwrap();
                            if parts.uri.path() == "/destination" {
                                return Response::new(Body::from("unexpected relative follow"));
                            }
                            Response::builder()
                                .status(status)
                                .header("location", location)
                                .header("set-cookie", "first=one; HttpOnly")
                                .header("set-cookie", "second=two; SameSite=Lax")
                                .body(Body::from("application redirect body"))
                                .unwrap()
                        }
                    })))
                    .await;
                let forwarded = timeout(Duration::from_secs(5), forward(method, app.port))
                    .await
                    .expect("forward request completes");
                let (seen_method, seen_uri, seen_body) = seen_rx.try_recv().unwrap();
                assert_eq!(seen_method, method);
                assert_eq!(seen_uri, "/redirect?keep=query");
                assert_eq!(seen_body, "original request body");
                let replayed = seen_rx.try_recv().is_ok() || destination_rx.try_recv().is_ok();
                if forwarded.status != status
                    || forwarded.location.as_deref() != Some(location.as_str())
                    || forwarded.cookies != ["first=one; HttpOnly", "second=two; SameSite=Lax"]
                    || forwarded.body != "application redirect body"
                    || replayed
                {
                    failures.push(format!(
                        "{method} {status} {location}: status={}, location={:?}, cookies={:?}, replayed={replayed}",
                        forwarded.status, forwarded.location, forwarded.cookies
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "redirect forwarding failures: {failures:#?}"
    );
}

#[tokio::test]
async fn redirects_preserve_status_location_cookies_body_and_never_replay() {
    let client = create_forward_client();
    assert_application_redirects_are_returned(|method, app_port| {
        let client = client.clone();
        async move {
            let response = forward_http_request(
                &client,
                Request::builder()
                    .method(method)
                    .uri("/redirect?keep=query")
                    .body(Body::from("original request body"))
                    .unwrap(),
                app_port,
                CancellationToken::new(),
            )
            .await;
            Forwarded::from_response(response).await
        }
    })
    .await;
}

#[tokio::test]
async fn normal_response_is_preserved() {
    let app = Server::start(Router::new().fallback(any(|| async {
        Response::builder()
            .status(StatusCode::CREATED)
            .header("x-app", "preserved")
            .body(Body::from("normal response"))
            .unwrap()
    })))
    .await;
    let response = forward_http_request(
        &create_forward_client(),
        Request::new(Body::empty()),
        app.port,
        CancellationToken::new(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()["x-app"], "preserved");
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "normal response"
    );
}

#[tokio::test]
async fn streaming_is_progressive_and_disconnect_releases_upstream() {
    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(1);
    let rx = Arc::new(tokio::sync::Mutex::new(Some(rx)));
    let app = Server::start(Router::new().fallback(any(move || {
        let rx = rx.clone();
        async move { Body::from_stream(ReceiverStream::new(rx.lock().await.take().unwrap())) }
    })))
    .await;
    tx.send(Ok(Bytes::from_static(b"first"))).await.unwrap();
    let mut response = timeout(
        Duration::from_secs(5),
        forward_http_request(
            &create_forward_client(),
            Request::new(Body::empty()),
            app.port,
            CancellationToken::new(),
        ),
    )
    .await
    .expect("headers arrive before the upstream stream ends");
    let first = timeout(Duration::from_secs(5), response.body_mut().frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(first, "first");
    tx.send(Ok(Bytes::from_static(b"second"))).await.unwrap();
    let second = timeout(Duration::from_secs(5), response.body_mut().frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(second, "second");
    drop(response);
    timeout(Duration::from_secs(5), tx.closed())
        .await
        .expect("dropping the downstream response closes the upstream body");
}

#[cfg(all(feature = "gcp", feature = "azure"))]
#[tokio::test]
async fn transports_return_application_redirects_over_http() {
    let app = Server::start(Router::new().fallback(any(|| async {
        Response::builder()
            .status(StatusCode::FOUND)
            .header("location", "/destination")
            .body(Body::from("redirect from app"))
            .unwrap()
    })))
    .await;
    for transport in ["local", "cloudrun", "containerapp"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let (shutdown, receiver) = broadcast::channel(1);
        let control = Arc::new(ControlGrpcServer::new());
        let app_port = app.port;
        let task = tokio::spawn(async move {
            match transport {
                "local" => {
                    crate::transports::local::LocalTransport::new(port, control, receiver)
                        .with_app_port(app_port)
                        .run()
                        .await
                }
                #[cfg(feature = "gcp")]
                "cloudrun" => {
                    crate::transports::cloudrun::CloudRunTransport::new(port, control, receiver)
                        .with_app_port(app_port)
                        .run()
                        .await
                }
                #[cfg(feature = "azure")]
                "containerapp" => {
                    crate::transports::containerapp::ContainerAppTransport::new(
                        port, control, receiver,
                    )
                    .with_app_port(app_port)
                    .run()
                    .await
                }
                _ => panic!("transport feature required for this test"),
            }
        });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let response = timeout(Duration::from_secs(5), async {
            loop {
                match client
                    .get(format!("http://127.0.0.1:{port}/redirect"))
                    .send()
                    .await
                {
                    Ok(response) => break response,
                    Err(error) if error.is_connect() => tokio::task::yield_now().await,
                    Err(error) => panic!("transport request failed: {error}"),
                }
            }
        })
        .await
        .expect("transport responds");
        let status = response.status();
        let location = response.headers().get("location").cloned();
        let body = response.bytes().await.unwrap();
        shutdown.send(()).unwrap();
        timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(status, StatusCode::FOUND, "{transport}");
        assert_eq!(location.unwrap(), "/destination");
        assert_eq!(body, "redirect from app");
    }
}
