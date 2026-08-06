//! Response-side half of the AWS Bedrock arm: Claude's classic InvokeModel reply is
//! event-stream framed, so it decodes here, apart from the request path in `bedrock.rs`.

use aws_smithy_eventstream::frame::{DecodedFrame, MessageFrameDecoder};
use aws_smithy_types::event_stream::Message;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

/// Decoder turning Bedrock's `vnd.amazon.eventstream` framing into Anthropic SSE.
/// A normal chunk frame's payload is `{"bytes": base64(<anthropic event json>)}`;
/// the decoded event carries a `type` we surface as the SSE `event:` name. Network
/// chunks can split or merge frames, so bytes are buffered until each frame is
/// whole. Frame parsing (prelude, headers, both CRC32 checks) is
/// aws-smithy-eventstream's `MessageFrameDecoder` — AWS's own decoder for this
/// wire format — so a corrupted or desynced stream fails its CRCs instead of
/// decoding to garbage.
#[derive(Default)]
pub(crate) struct EventStreamToSse {
    buf: Vec<u8>,
    /// Set once a frame fails to decode (a CRC mismatch or malformed prelude).
    /// From then on the buffer can never be drained, so we stop parsing rather than
    /// spin on it forever.
    failed: bool,
}

impl EventStreamToSse {
    /// Append a network chunk and return the SSE for every frame it now completes.
    pub(crate) fn push(&mut self, chunk: &[u8]) -> String {
        if self.failed {
            return String::new();
        }
        self.buf.extend_from_slice(chunk);
        let mut out = String::new();
        // A fresh decoder scans the buffer from the top each push, and only the bytes
        // of fully decoded frames are drained: MessageFrameDecoder consumes a prelude
        // into internal state before its frame completes, so reusing one across
        // pushes would strand those bytes between the two buffers.
        let mut decoder = MessageFrameDecoder::new();
        let mut cursor: &[u8] = &self.buf;
        let mut consumed = 0;
        loop {
            match decoder.decode_frame(&mut cursor) {
                Ok(DecodedFrame::Complete(message)) => {
                    consumed = self.buf.len() - cursor.len();
                    out.push_str(&message_to_sse(&message));
                }
                Ok(DecodedFrame::Incomplete) => break,
                Err(_) => {
                    // A CRC mismatch or malformed prelude can never recover: the byte
                    // stream desynced. Surface it loudly and stop, rather than
                    // silently stall on bytes we can never drain (which would
                    // truncate the reply under an already-sent 200).
                    self.failed = true;
                    out.push_str(&error_sse("the model response stream could not be decoded"));
                    break;
                }
            }
        }
        self.buf.drain(0..consumed);
        out
    }

    /// Flush at end of stream. A non-empty buffer here means the upstream closed
    /// mid-frame (a truncated or desynced stream), so surface a loud error rather
    /// than drop the tail; a clean boundary (empty buffer) emits nothing.
    pub(crate) fn finish(&mut self) -> String {
        if self.failed || self.buf.is_empty() {
            return String::new();
        }
        self.failed = true;
        error_sse("the model response stream ended before the final frame completed")
    }
}

/// One event-stream message rendered as the Anthropic SSE the client expects.
///
/// A normal chunk wraps the event as `{"bytes": base64(...)}`. Anything else on an
/// InvokeModelWithResponseStream reply is an exception frame: Bedrock signals
/// mid-stream failures (throttlingException, modelStreamErrorException,
/// internalServerException) this way, with the exception body as the raw payload.
/// Such a frame is surfaced as an Anthropic `error` SSE event rather than dropped,
/// because dropping it would truncate the reply under an already-sent 200 with no
/// error reaching the client. Its provider body is never forwarded because it may
/// contain customer or provider-account data.
fn message_to_sse(message: &Message) -> String {
    let outer: Option<Value> = serde_json::from_slice(message.payload()).ok();
    if let Some(sse) = outer.as_ref().and_then(chunk_to_sse) {
        return sse;
    }
    error_sse("The customer model provider interrupted the response")
}

/// A normal `{"bytes": base64(<anthropic event>)}` chunk rendered as its SSE line,
/// or `None` if the frame is not a well-formed chunk.
fn chunk_to_sse(outer: &Value) -> Option<String> {
    let event_bytes = STANDARD.decode(outer.get("bytes")?.as_str()?).ok()?;
    let event: Value = serde_json::from_slice(&event_bytes).ok()?;
    let event_type = event.get("type")?.as_str()?;
    let data = std::str::from_utf8(&event_bytes).ok()?;
    Some(format!("event: {event_type}\ndata: {data}\n\n"))
}

/// An Anthropic `error` SSE event carrying `message`, so a mid-stream failure
/// reaches the client as a loud error instead of a silently truncated reply.
fn error_sse(message: &str) -> String {
    let event = json!({ "type": "error", "error": { "type": "api_error", "message": message } });
    format!("event: error\ndata: {event}\n\n")
}
