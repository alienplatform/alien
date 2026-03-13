pub mod error;
pub mod request_utils;

// Re-export commonly used types for convenience
pub use error::{Error, ErrorData, Result};
pub use request_utils::{
    handle_json_response, handle_no_response, handle_xml_response, RequestBuilderExt,
    RetriableRequestBuilder,
};
