use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors raised while starting or serving the embedded AI gateway.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The gateway could not bind its loopback listener.
    #[error(
        code = "GATEWAY_BIND_FAILED",
        message = "Failed to bind the AI gateway on {address}",
        retryable = "false",
        internal = "true"
    )]
    BindFailed { address: String },

    /// The request named a binding the gateway does not serve.
    #[error(
        code = "GATEWAY_UNKNOWN_BINDING",
        message = "No AI binding named '{binding}'",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    UnknownBinding { binding: String },

    /// The request body was not a usable model request.
    #[error(
        code = "GATEWAY_INVALID_REQUEST",
        message = "Invalid AI request: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    InvalidRequest { message: String },

    /// The requested model is not in the catalog for this binding's cloud.
    #[error(
        code = "GATEWAY_MODEL_NOT_AVAILABLE",
        message = "Model '{model}' is not available on binding '{binding}'",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    ModelNotAvailable { model: String, binding: String },

    /// The upstream cloud endpoint could not be reached. Not internal: the 502 and
    /// its message (a cloud endpoint host, never a credential) are safe to surface,
    /// and `internal = true` would collapse this to a generic 500 at the client.
    #[error(
        code = "GATEWAY_UPSTREAM_FAILED",
        message = "Upstream AI endpoint request failed: {message}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    UpstreamFailed { message: String },

    /// The workload's ambient cloud identity could not be obtained (no instance
    /// role, or the metadata service is unreachable) — transient, so retryable.
    #[error(
        code = "GATEWAY_AMBIENT_CREDENTIAL_UNAVAILABLE",
        message = "Could not obtain the workload's ambient cloud credential: {message}",
        retryable = "true",
        internal = "true"
    )]
    AmbientCredentialUnavailable { message: String },

    /// The workload's resolved identity cannot authorize this binding's cloud (e.g. an
    /// AWS binding whose workload identity is not an AWS credential). A permanent
    /// invariant failure, so retrying cannot succeed.
    #[error(
        code = "GATEWAY_WORKLOAD_IDENTITY_INVALID",
        message = "The workload identity cannot authorize this binding: {message}",
        retryable = "false",
        internal = "true"
    )]
    WorkloadIdentityInvalid { message: String },

    /// A binding's projected configuration is unusable (missing a required field, or
    /// a cloud the gateway does not serve). User-fixable, so not internal.
    #[error(
        code = "GATEWAY_BINDING_CONFIG_INVALID",
        message = "AI binding '{binding}' is invalid: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    BindingConfigInvalid { binding: String, message: String },

    /// Generic catch-all for unexpected, non-retryable gateway failures (build,
    /// serialization, or configuration bugs). Transient cases get their own variant.
    #[error(
        code = "GATEWAY_ERROR",
        message = "AI gateway error: {message}",
        retryable = "false",
        internal = "true"
    )]
    Other { message: String },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
