pub mod error;
pub mod interceptor;
pub mod request_utils;
#[cfg(feature = "test-utils")]
pub mod testing;

// Re-export commonly used types for convenience
pub use error::{Error, ErrorData, Result};
pub use request_utils::{
    handle_json_response, handle_no_response, handle_xml_response, redact_request_body,
    redact_response_body, redact_secret_bodies, InterceptedRequestBuilder, RequestBuilderExt,
    RetriableRequestBuilder,
};
