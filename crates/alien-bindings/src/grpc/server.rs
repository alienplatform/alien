use crate::{
    error::{ErrorData, Result},
    grpc::{
        control_service::{
            alien_bindings::control::FILE_DESCRIPTOR_SET as CONTROL_FILE_DESCRIPTOR_SET,
            ControlGrpcServer,
        },
        wait_until_service::{
            alien_bindings::wait_until::FILE_DESCRIPTOR_SET as WAIT_UNTIL_FILE_DESCRIPTOR_SET,
            WaitUntilGrpcServer,
        },
        MAX_GRPC_MESSAGE_SIZE,
    },
    BindingsProviderApi,
};
use alien_error::{Context, IntoAlienError};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tracing::info;

/// Handles returned from run_grpc_server
pub struct GrpcServerHandles {
    pub wait_until_server: Arc<WaitUntilGrpcServer>,
    pub control_server: Arc<ControlGrpcServer>,
    pub server_task: tokio::task::JoinHandle<Result<()>>,
    pub readiness_receiver: oneshot::Receiver<()>,
}

/// Configures and runs the alien-bindings worker-protocol gRPC server.
///
/// This server hosts the worker-protocol services (Control + WaitUntil) that
/// coordinate the runtime with the application it manages. The bindings
/// themselves are now resolved in-process by the direct provider, so no
/// per-binding gRPC service is registered here.
///
/// The `provider` argument is retained for signature compatibility with the
/// runtime callers; the worker-protocol services do not consult it.
pub async fn run_grpc_server(
    _provider: Arc<dyn BindingsProviderApi>,
    addr_str: &str,
) -> Result<GrpcServerHandles> {
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
    // intra-machine IPC between alien-runtime and the application code it
    // manages, and they share a trust boundary. Binding to a non-loopback
    // address erases that assumption (think `0.0.0.0` from a misconfigured
    // ALIEN_BINDINGS_ADDRESS, `--network=host` Docker, or shared-pod sidecar).
    // Make the misconfiguration loud at startup rather than silently exposing
    // the control/drain channel.
    if !addr.ip().is_loopback() {
        tracing::warn!(
            address = %addr,
            "alien-runtime gRPC server is binding to a NON-LOOPBACK address. \
            This exposes the worker-protocol server (Control, WaitUntil) to anyone who can \
            reach this network interface. The server has no authentication. Set \
            ALIEN_BINDINGS_ADDRESS=127.0.0.1:51351 unless you have a specific reason to expose it."
        );
    }

    let wait_until_server = Arc::new(WaitUntilGrpcServer::new());
    let wait_until_server_handle = wait_until_server.clone();

    let control_server = Arc::new(ControlGrpcServer::new());
    let control_server_handle = control_server.clone();

    let mut router = Server::builder()
        .concurrency_limit_per_connection(256) // Allow many concurrent requests per connection
        .add_service(
            (*wait_until_server)
                .clone()
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        )
        .add_service(
            (*control_server)
                .clone()
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        );

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
