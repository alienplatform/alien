use crate::{
    control::FILE_DESCRIPTOR_SET as CONTROL_FILE_DESCRIPTOR_SET,
    control_service::ControlGrpcServer,
    error::{ErrorData, Result},
    wait_until::FILE_DESCRIPTOR_SET as WAIT_UNTIL_FILE_DESCRIPTOR_SET,
    wait_until_service::WaitUntilGrpcServer,
    MAX_GRPC_MESSAGE_SIZE,
};
use alien_error::{Context, IntoAlienError};
use std::{
    sync::Arc,
    task::{Context as TaskContext, Poll},
};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    codegen::{
        http::{Request as HttpRequest, Uri},
        Service,
    },
    server::NamedService,
    transport::Server,
};
use tracing::info;

const LEGACY_CONTROL_SERVICE_NAME: &str = "alien_bindings.control.ControlService";
const LEGACY_WAIT_UNTIL_SERVICE_NAME: &str = "alien_bindings.wait_until.WaitUntilService";

#[derive(Clone)]
struct LegacyControlService<S> {
    inner: S,
}

impl<S> LegacyControlService<S> {
    fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S> NamedService for LegacyControlService<S> {
    const NAME: &'static str = LEGACY_CONTROL_SERVICE_NAME;
}

impl<S> Service<HttpRequest<tonic::body::Body>> for LegacyControlService<S>
where
    S: Service<HttpRequest<tonic::body::Body>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        context: &mut TaskContext<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, mut request: HttpRequest<tonic::body::Body>) -> Self::Future {
        if let Some(uri) = current_worker_protocol_uri(request.uri().path()) {
            *request.uri_mut() = uri;
        }
        self.inner.call(request)
    }
}

#[derive(Clone)]
struct LegacyWaitUntilService<S> {
    inner: S,
}

impl<S> LegacyWaitUntilService<S> {
    fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S> NamedService for LegacyWaitUntilService<S> {
    const NAME: &'static str = LEGACY_WAIT_UNTIL_SERVICE_NAME;
}

impl<S> Service<HttpRequest<tonic::body::Body>> for LegacyWaitUntilService<S>
where
    S: Service<HttpRequest<tonic::body::Body>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        context: &mut TaskContext<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, mut request: HttpRequest<tonic::body::Body>) -> Self::Future {
        if let Some(uri) = current_worker_protocol_uri(request.uri().path()) {
            *request.uri_mut() = uri;
        }
        self.inner.call(request)
    }
}

fn current_worker_protocol_uri(path: &str) -> Option<Uri> {
    match path {
        "/alien_bindings.control.ControlService/RegisterHttpServer" => Some(Uri::from_static(
            "/alien_worker.control.ControlService/RegisterHttpServer",
        )),
        "/alien_bindings.control.ControlService/RegisterEventHandler" => Some(Uri::from_static(
            "/alien_worker.control.ControlService/RegisterEventHandler",
        )),
        "/alien_bindings.control.ControlService/WaitForTasks" => Some(Uri::from_static(
            "/alien_worker.control.ControlService/WaitForTasks",
        )),
        "/alien_bindings.control.ControlService/SendTaskResult" => Some(Uri::from_static(
            "/alien_worker.control.ControlService/SendTaskResult",
        )),
        "/alien_bindings.wait_until.WaitUntilService/NotifyTaskRegistered" => Some(
            Uri::from_static("/alien_worker.wait_until.WaitUntilService/NotifyTaskRegistered"),
        ),
        "/alien_bindings.wait_until.WaitUntilService/WaitForDrainSignal" => Some(Uri::from_static(
            "/alien_worker.wait_until.WaitUntilService/WaitForDrainSignal",
        )),
        "/alien_bindings.wait_until.WaitUntilService/NotifyDrainComplete" => Some(
            Uri::from_static("/alien_worker.wait_until.WaitUntilService/NotifyDrainComplete"),
        ),
        "/alien_bindings.wait_until.WaitUntilService/GetTaskCount" => Some(Uri::from_static(
            "/alien_worker.wait_until.WaitUntilService/GetTaskCount",
        )),
        _ => None,
    }
}

/// Handles returned from run_grpc_server
pub struct GrpcServerHandles {
    pub wait_until_server: Arc<WaitUntilGrpcServer>,
    pub control_server: Arc<ControlGrpcServer>,
    pub server_task: tokio::task::JoinHandle<Result<()>>,
    pub readiness_receiver: oneshot::Receiver<()>,
}

/// Configures and runs the worker app protocol gRPC server.
///
/// This server hosts the worker-protocol services (Control + WaitUntil) that
/// coordinate the Worker runtime with the application it manages. Binding calls
/// are resolved directly by the application and never traverse this server.
pub async fn run_grpc_server(addr_str: &str) -> Result<GrpcServerHandles> {
    let addr: std::net::SocketAddr =
        addr_str
            .parse()
            .into_alien_error()
            .context(ErrorData::ServerBindFailed {
                address: addr_str.to_string(),
                reason: "Invalid address format".to_string(),
            })?;

    info!("Configuring gRPC server for {}", addr);

    // The worker-protocol gRPC server is unauthenticated by design — it's
    // intra-machine IPC between the Worker runtime and the application code it
    // manages, and they share a trust boundary. Binding to a non-loopback
    // address erases that assumption (think `0.0.0.0` from a misconfigured
    // ALIEN_WORKER_GRPC_ADDRESS, `--network=host` Docker, or shared-pod sidecar).
    // Make the misconfiguration loud at startup rather than silently exposing
    // the control/drain channel.
    if !addr.ip().is_loopback() {
        tracing::warn!(
            address = %addr,
            "worker-protocol gRPC server is binding to a NON-LOOPBACK address. \
            This exposes the worker-protocol server (Control, WaitUntil) to anyone who can \
            reach this network interface. The server has no authentication. Set \
            ALIEN_WORKER_GRPC_ADDRESS=127.0.0.1:51351 unless you have a specific reason to expose it."
        );
    }

    let wait_until_server = Arc::new(WaitUntilGrpcServer::new());
    let wait_until_server_handle = wait_until_server.clone();

    let control_server = Arc::new(ControlGrpcServer::new());
    let control_server_handle = control_server.clone();

    let wait_until_service = (*wait_until_server)
        .clone()
        .into_service()
        .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE);
    let control_service = (*control_server)
        .clone()
        .into_service()
        .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE);

    let mut router = Server::builder()
        .concurrency_limit_per_connection(256) // Allow many concurrent requests per connection
        .add_service(wait_until_service.clone())
        .add_service(LegacyWaitUntilService::new(wait_until_service))
        .add_service(control_service.clone())
        .add_service(LegacyControlService::new(control_service));

    // Add reflection service
    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(WAIT_UNTIL_FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(CONTROL_FILE_DESCRIPTOR_SET)
        .build_v1()
        .into_alien_error()
        .context(ErrorData::GrpcServiceUnavailable {
            service: "reflection".to_string(),
            endpoint: addr.to_string(),
            reason: "Failed to build reflection service".to_string(),
        })?;

    router = router.add_service(reflection_service);

    // Create a oneshot channel to signal when the server is ready
    let (readiness_sender, readiness_receiver) = oneshot::channel();

    let server_task = tokio::spawn(async move {
        // Bind to the address first to ensure it's available
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .into_alien_error()
            .context(ErrorData::ServerBindFailed {
                address: addr.to_string(),
                reason: "Failed to bind to address".to_string(),
            })?;

        info!("gRPC server successfully bound to {}", addr);

        // Signal that the server is ready to accept connections
        if let Err(_) = readiness_sender.send(()) {
            // The receiver was dropped, but we can still continue
            info!("Readiness receiver was dropped, continuing with server startup");
        }

        // Convert the TcpListener to a stream of incoming connections
        let incoming = TcpListenerStream::new(listener);

        info!("gRPC server is now serving requests on {}", addr);

        // Serve with the incoming connection stream
        router
            .serve_with_incoming(incoming)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcServiceUnavailable {
                service: "main".to_string(),
                endpoint: addr.to_string(),
                reason: "gRPC server failed to serve".to_string(),
            })
    });

    Ok(GrpcServerHandles {
        wait_until_server: wait_until_server_handle,
        control_server: control_server_handle,
        server_task,
        readiness_receiver,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control::{
            control_service_client::ControlServiceClient, RegisterHttpServerRequest,
            RegisterHttpServerResponse,
        },
        wait_until::{
            wait_until_service_client::WaitUntilServiceClient, NotifyTaskRegisteredRequest,
            NotifyTaskRegisteredResponse,
        },
    };

    struct AbortOnDrop(tokio::task::JoinHandle<Result<()>>);

    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    #[tokio::test]
    async fn grpc_server_routes_current_and_legacy_service_names_to_shared_state() {
        let reserved_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("reserve protocol server port");
        let address = reserved_listener.local_addr().expect("reserved address");
        drop(reserved_listener);

        let GrpcServerHandles {
            wait_until_server,
            control_server,
            server_task,
            readiness_receiver,
        } = run_grpc_server(&address.to_string())
            .await
            .expect("configure protocol server");
        let _server_guard = AbortOnDrop(server_task);
        readiness_receiver
            .await
            .expect("protocol server should become ready");

        let channel = tonic::transport::Channel::from_shared(format!("http://{address}"))
            .expect("valid protocol endpoint")
            .connect()
            .await
            .expect("connect to protocol server");

        let mut current_control_client = ControlServiceClient::new(channel.clone());
        let current_control_response = current_control_client
            .register_http_server(RegisterHttpServerRequest { port: 4100 })
            .await
            .expect("current control namespace should be served")
            .into_inner();
        assert!(current_control_response.success);

        let mut legacy_control_client = tonic::client::Grpc::new(channel.clone());
        legacy_control_client
            .ready()
            .await
            .expect("legacy control service should be ready");
        let legacy_control_response: tonic::Response<RegisterHttpServerResponse> =
            legacy_control_client
                .unary(
                    tonic::Request::new(RegisterHttpServerRequest { port: 4200 }),
                    tonic::codegen::http::uri::PathAndQuery::from_static(
                        "/alien_bindings.control.ControlService/RegisterHttpServer",
                    ),
                    tonic::codec::ProstCodec::default(),
                )
                .await
                .expect("legacy control namespace should be served");
        assert!(legacy_control_response.into_inner().success);
        assert_eq!(control_server.get_http_port().await, Some(4200));

        let mut current_wait_until_client = WaitUntilServiceClient::new(channel.clone());
        let current_wait_until_response = current_wait_until_client
            .notify_task_registered(NotifyTaskRegisteredRequest {
                application_id: "current-app".to_string(),
                task_description: None,
            })
            .await
            .expect("current wait-until namespace should be served")
            .into_inner();
        assert!(current_wait_until_response.success);

        let mut legacy_wait_until_client = tonic::client::Grpc::new(channel);
        legacy_wait_until_client
            .ready()
            .await
            .expect("legacy wait-until service should be ready");
        let legacy_wait_until_response: tonic::Response<NotifyTaskRegisteredResponse> =
            legacy_wait_until_client
                .unary(
                    tonic::Request::new(NotifyTaskRegisteredRequest {
                        application_id: "legacy-app".to_string(),
                        task_description: None,
                    }),
                    tonic::codegen::http::uri::PathAndQuery::from_static(
                        "/alien_bindings.wait_until.WaitUntilService/NotifyTaskRegistered",
                    ),
                    tonic::codec::ProstCodec::default(),
                )
                .await
                .expect("legacy wait-until namespace should be served");
        assert!(legacy_wait_until_response.into_inner().success);
        assert_eq!(wait_until_server.get_total_task_count().await, 2);
    }
}
