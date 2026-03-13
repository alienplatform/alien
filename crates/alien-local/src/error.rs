use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors related to local platform services.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Failed to create or access local directory.
    #[error(
        code = "LOCAL_DIRECTORY_ERROR",
        message = "Failed to {operation} directory '{path}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    LocalDirectoryError {
        /// Directory path that failed
        path: String,
        /// Operation that was attempted (create, read, write, delete)
        operation: String,
        /// Reason for the failure
        reason: String,
    },

    /// Failed to start or manage a local process.
    #[error(
        code = "LOCAL_PROCESS_ERROR",
        message = "Failed to {operation} process '{process_id}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    LocalProcessError {
        /// Process identifier
        process_id: String,
        /// Operation that was attempted (start, stop, query)
        operation: String,
        /// Reason for the failure
        reason: String,
    },

    /// No free ports available for service allocation.
    #[error(
        code = "NO_FREE_PORTS",
        message = "No free ports available for service '{service_name}'",
        retryable = "true",
        internal = "false"
    )]
    NoFreePorts {
        /// Name of the service that needed a port
        service_name: String,
    },

    /// Failed to start or manage local registry server.
    #[error(
        code = "LOCAL_REGISTRY_ERROR",
        message = "Failed to {operation} registry '{registry_id}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    LocalRegistryError {
        /// Registry identifier
        registry_id: String,
        /// Operation that was attempted (start, stop, query)
        operation: String,
        /// Reason for the failure
        reason: String,
    },

    /// Failed to access or update bindings store.
    #[error(
        code = "BINDINGS_STORE_ERROR",
        message = "Failed to {operation} binding '{binding_id}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    BindingsStoreError {
        /// Binding identifier
        binding_id: String,
        /// Operation that was attempted (get, set, remove)
        operation: String,
        /// Reason for the failure
        reason: String,
    },

    /// Binding not found in bindings store.
    #[error(
        code = "BINDING_NOT_FOUND",
        message = "Binding '{binding_id}' not found in bindings store",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    BindingNotFound {
        /// Binding identifier that was not found
        binding_id: String,
    },

    /// Failed to open or access sled database.
    #[error(
        code = "LOCAL_DATABASE_ERROR",
        message = "Failed to {operation} database '{database_path}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    LocalDatabaseError {
        /// Path to the database
        database_path: String,
        /// Operation that was attempted (open, close, read, write)
        operation: String,
        /// Reason for the failure
        reason: String,
    },

    /// Service resource not found.
    #[error(
        code = "SERVICE_RESOURCE_NOT_FOUND",
        message = "Service resource '{resource_id}' of type '{resource_type}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    ServiceResourceNotFound {
        /// Resource identifier
        resource_id: String,
        /// Type of resource (function, registry, etc.)
        resource_type: String,
    },

    /// Service already exists with the same ID.
    #[error(
        code = "SERVICE_RESOURCE_ALREADY_EXISTS",
        message = "Service resource '{resource_id}' of type '{resource_type}' already exists",
        retryable = "false",
        internal = "false"
    )]
    ServiceResourceAlreadyExists {
        /// Resource identifier
        resource_id: String,
        /// Type of resource (function, registry, etc.)
        resource_type: String,
    },

    /// Failed to load runtime configuration file.
    #[error(
        code = "RUNTIME_CONFIG_LOAD_FAILED",
        message = "Failed to load runtime configuration for function '{function_id}' from '{config_path}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    RuntimeConfigLoadFailed {
        /// Function identifier
        function_id: String,
        /// Path to the configuration file
        config_path: String,
        /// Reason for the failure
        reason: String,
    },

    /// Failed to build or apply runtime configuration.
    #[error(
        code = "RUNTIME_CONFIG_INVALID",
        message = "Invalid runtime configuration for function '{function_id}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    RuntimeConfigInvalid {
        /// Function identifier
        function_id: String,
        /// Reason why the configuration is invalid
        reason: String,
    },

    /// Failed to parse network address.
    #[error(
        code = "ADDRESS_PARSE_FAILED",
        message = "Failed to parse address '{address}' for {service_type} '{service_id}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    AddressParseFailed {
        /// Service identifier (function ID, etc.)
        service_id: String,
        /// Type of service (function, registry, etc.)
        service_type: String,
        /// The address that failed to parse
        address: String,
        /// Parse error details
        reason: String,
    },

    /// Runtime configuration file not found at expected location.
    #[error(
        code = "RUNTIME_CONFIG_NOT_FOUND",
        message = "Runtime configuration not found for function '{function_id}' at expected location: {expected_path}",
        retryable = "false",
        internal = "false"
    )]
    RuntimeConfigNotFound {
        /// Function identifier
        function_id: String,
        /// Expected path where config should exist
        expected_path: String,
    },

    /// Failed to connect to Docker daemon.
    #[error(
        code = "DOCKER_CONNECTION_FAILED",
        message = "Failed to connect to Docker daemon: {reason}",
        retryable = "false",
        internal = "false"
    )]
    DockerConnectionFailed {
        /// Reason for the failure
        reason: String,
    },

    /// Failed to create or manage Docker network.
    #[error(
        code = "DOCKER_NETWORK_ERROR",
        message = "Failed to {operation} Docker network '{network}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    DockerNetworkError {
        /// Network name
        network: String,
        /// Operation that was attempted (create, inspect, remove)
        operation: String,
        /// Reason for the failure
        reason: String,
    },

    /// Failed to create, start, stop, or remove Docker container.
    #[error(
        code = "DOCKER_CONTAINER_ERROR",
        message = "Failed to {operation} Docker container '{container}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    DockerContainerError {
        /// Container name or ID
        container: String,
        /// Operation that was attempted (create, start, stop, remove)
        operation: String,
        /// Reason for the failure
        reason: String,
    },

    /// Failed to create or manage Docker volume.
    #[error(
        code = "DOCKER_VOLUME_ERROR",
        message = "Failed to {operation} Docker volume '{volume}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    DockerVolumeError {
        /// Volume name
        volume: String,
        /// Operation that was attempted (create, inspect, remove)
        operation: String,
        /// Reason for the failure
        reason: String,
    },

    /// No free ports available for container allocation.
    #[error(
        code = "NO_FREE_PORTS_AVAILABLE",
        message = "No free ports available for container allocation",
        retryable = "true",
        internal = "false"
    )]
    NoFreePortsAvailable,

    /// Container is not running.
    #[error(
        code = "CONTAINER_NOT_RUNNING",
        message = "Container '{container_id}' is not running",
        retryable = "true",
        internal = "false"
    )]
    ContainerNotRunning {
        /// Container ID
        container_id: String,
    },

    /// Generic local platform error.
    #[error(
        code = "LOCAL_PLATFORM_ERROR",
        message = "Local platform error: {message}",
        retryable = "false",
        internal = "true"
    )]
    Other {
        /// Human-readable description of the error
        message: String,
    },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
