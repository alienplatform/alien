//! Byte stream over a WebSocket.
//!
//! HTTP/2 needs an ordered, reliable byte stream. A WebSocket provides one as
//! a sequence of binary messages, and it is the one long-lived connection type
//! that load balancers and corporate proxies reliably pass through. Each write
//! becomes one binary message; reads concatenate binary messages. Ping/pong
//! frames are handled by tungstenite and never surface as data.

use std::io;

use bytes::Bytes;
use futures::{SinkExt, StreamExt, TryStreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};
use tokio_util::io::{CopyToBytes, SinkWriter, StreamReader};

/// Turn a WebSocket into an `AsyncRead + AsyncWrite` byte stream.
pub fn websocket_io<S>(ws: WebSocketStream<S>) -> impl AsyncRead + AsyncWrite + Send + Unpin
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (sink, stream) = ws.split();

    let reader = StreamReader::new(Box::pin(stream.map_err(io::Error::other).try_filter_map(
        |message| async move {
            match message {
                Message::Binary(data) => Ok(Some(data)),
                // Control frames are answered by tungstenite; a close frame
                // is followed by the end of the stream.
                Message::Ping(_) | Message::Pong(_) | Message::Close(_) | Message::Frame(_) => {
                    Ok(None)
                }
                Message::Text(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "tunnel connections carry binary frames only",
                )),
            }
        },
    )));

    let writer = SinkWriter::new(CopyToBytes::new(Box::pin(
        sink.sink_map_err(io::Error::other)
            .with(|data: Bytes| async move { Ok::<_, io::Error>(Message::Binary(data)) }),
    )));

    tokio::io::join(reader, writer)
}
