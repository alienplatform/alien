use axum::response::{
    sse::{Event, Sse},
    IntoResponse, Response,
};
use futures_util::StreamExt;
use std::time::Duration;
use tokio::time::interval;
use tokio_stream::wrappers::IntervalStream;
use utoipa::path;

use crate::Result;

/// Server-Sent Events endpoint
#[utoipa::path(
    get,
    path = "/sse",
    tag = "testing",
    responses(
        (status = 200, description = "Server-sent events stream", content_type = "text/event-stream"),
    ),
    operation_id = "sse_endpoint",
    summary = "Server-Sent Events",
    description = "Provides a stream of server-sent events for testing SSE functionality"
)]
pub async fn sse_endpoint() -> Result<Response> {
    let stream = IntervalStream::new(interval(Duration::from_millis(500)))
        .take(10) // Send 10 events
        .enumerate()
        .map(|(i, _)| {
            Ok::<_, axum::Error>(
                Event::default()
                    .data(format!("sse_message_{}", i))
                    .id(i.to_string())
                    .event("message"),
            )
        });

    let sse_response = Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(5))
            .text("keep-alive"),
    );

    Ok(sse_response.into_response())
}
