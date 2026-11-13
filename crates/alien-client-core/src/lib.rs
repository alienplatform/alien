pub mod error;
pub mod request_utils;
#[cfg(not(target_arch = "wasm32"))]
pub mod runtime_credentials;
#[cfg(not(target_arch = "wasm32"))]
pub mod runtime_proofs;

// Re-export commonly used types for convenience
pub use error::{Error, ErrorData, Result};
pub use request_utils::{
    handle_json_response, handle_no_response, handle_xml_response, redact_request_body,
    redact_response_body, redact_secret_bodies, RequestBuilderExt, RetriableRequestBuilder,
};
