//! The gateway's JSON log format.
use tracing::Subscriber;
use tracing_subscriber::{
    fmt::{self, time::ChronoUtc, MakeWriter},
    registry::LookupSpan,
    Layer,
};

/// One flattened JSON record per line, with event fields beside `message`.
///
/// `timestamp` is RFC 3339 with nanoseconds, so separate connections in the same
/// microsecond still produce distinct records.
pub fn json_layer<S, W>(writer: W) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    fmt::layer()
        .json()
        .flatten_event(true)
        .with_timer(ChronoUtc::rfc_3339())
        .with_writer(writer)
}
