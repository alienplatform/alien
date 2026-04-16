//! Core types for alien-runtime.

use bytes::Bytes;
use chrono::{DateTime, Utc};
use http_body_util::combinators::BoxBody;
use hyper::Error as HyperError;
use hyper::{Request as HyperRequest, Response as HyperResponse};

/// Standardized representation of an incoming request.
#[derive(Debug)]
pub struct Request {
    /// Unique identifier for tracing and correlation.
    pub id: String,
    /// Timestamp when the request was received.
    pub received_at: DateTime<Utc>,
    /// The underlying Hyper request, if this is an HTTP request.
    pub hyper_request: Option<HyperRequest<BoxBody<Bytes, HyperError>>>,
}

/// Standardized representation of an outgoing response.
#[derive(Debug)]
pub struct Response {
    /// The ID of the request this response corresponds to.
    pub request_id: String,
    /// The underlying Hyper response, if this is an HTTP response.
    pub hyper_response: Option<HyperResponse<BoxBody<Bytes, crate::error::Error>>>,
    /// Optional hint about the response body size.
    pub content_length_hint: Option<u64>,
}
