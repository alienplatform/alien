use crate::{
    error::{ErrorData, Result},
    grpc::{
        artifact_registry_service::{
            alien_bindings::artifact_registry::FILE_DESCRIPTOR_SET as ARTIFACT_REGISTRY_FILE_DESCRIPTOR_SET,
            ArtifactRegistryGrpcServer,
        },
        build_service::{
            alien_bindings::build::FILE_DESCRIPTOR_SET as BUILD_FILE_DESCRIPTOR_SET,
            BuildGrpcServer,
        },
        container_service::{
            alien_bindings::container::FILE_DESCRIPTOR_SET as CONTAINER_FILE_DESCRIPTOR_SET,
            ContainerGrpcServer,
        },
        control_service::{
            alien_bindings::control::FILE_DESCRIPTOR_SET as CONTROL_FILE_DESCRIPTOR_SET,
            ControlGrpcServer,
        },
        function_service::{
            alien_bindings::function::FILE_DESCRIPTOR_SET as FUNCTION_FILE_DESCRIPTOR_SET,
            FunctionGrpcServer,
        },
        kv_service::{
            alien_bindings::kv::FILE_DESCRIPTOR_SET as KV_FILE_DESCRIPTOR_SET, KvGrpcServer,
        },
        queue_service::{
            alien_bindings::queue::FILE_DESCRIPTOR_SET as QUEUE_FILE_DESCRIPTOR_SET,
            QueueGrpcServer,
        },
        service_account_service::{
            alien_bindings::service_account::FILE_DESCRIPTOR_SET as SERVICE_ACCOUNT_FILE_DESCRIPTOR_SET,
            ServiceAccountGrpcServer,
        },
        storage_service::{
            alien_bindings::storage::FILE_DESCRIPTOR_SET as STORAGE_FILE_DESCRIPTOR_SET,
            StorageGrpcServer,
        },
        vault_service::{
            alien_bindings::vault::FILE_DESCRIPTOR_SET as VAULT_FILE_DESCRIPTOR_SET,
            VaultGrpcServer,
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

/// Configures and runs the main alien-bindings gRPC server.
///
/// This server will host all implemented gRPC services (Storage, KV, Queue, Control, etc.).
/// Returns handles for drain coordination, control service, and readiness notification.
pub async fn run_grpc_server(
    provider: Arc<dyn BindingsProviderApi>,
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

    // The bindings gRPC server is unauthenticated by design — it's intra-machine
    // IPC between alien-runtime and the application code it manages, and they
    // share a trust boundary. Binding to a non-loopback address erases that
    // assumption (think `0.0.0.0` from a misconfigured ALIEN_BINDINGS_ADDRESS,
    // `--network=host` Docker, or shared-pod sidecar). Make the misconfiguration
    // loud at startup rather than silently exposing Vault/Storage/KV.
    if !addr.ip().is_loopback() {
        tracing::warn!(
            address = %addr,
            "alien-runtime gRPC server is binding to a NON-LOOPBACK address. \
            This exposes the bindings server (Vault, Storage, KV, Control) to anyone who can \
            reach this network interface. The server has no authentication. Set \
            ALIEN_BINDINGS_ADDRESS=127.0.0.1:51351 unless you have a specific reason to expose it."
        );
    }

    let wait_until_server = Arc::new(WaitUntilGrpcServer::new(provider.clone()));
    let wait_until_server_handle = wait_until_server.clone();

    let control_server = Arc::new(ControlGrpcServer::new());
    let control_server_handle = control_server.clone();

    let mut router = Server::builder()
        .concurrency_limit_per_connection(256) // Allow many concurrent requests per connection
        .add_service(
            StorageGrpcServer::new(provider.clone())
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        )
        .add_service(
            BuildGrpcServer::new(provider.clone())
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        )
        .add_service(
            ArtifactRegistryGrpcServer::new(provider.clone())
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        )
        .add_service(
            VaultGrpcServer::new(provider.clone())
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        )
        .add_service(
            KvGrpcServer::new(provider.clone())
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        )
        .add_service(
            QueueGrpcServer::new(provider.clone())
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        )
        .add_service(
            FunctionGrpcServer::new(provider.clone())
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        )
        .add_service(
            ContainerGrpcServer::new(provider.clone())
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        )
        .add_service(
            ServiceAccountGrpcServer::new(provider.clone())
                .into_service()
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE),
        )
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
        .register_encoded_file_descriptor_set(STORAGE_FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(BUILD_FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(ARTIFACT_REGISTRY_FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(VAULT_FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(KV_FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(QUEUE_FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(FUNCTION_FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(CONTAINER_FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(SERVICE_ACCOUNT_FILE_DESCRIPTOR_SET)
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
