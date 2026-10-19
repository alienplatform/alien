//! Bounded buffered framing and resilient listener acceptance.
use std::{net::SocketAddr, time::Duration};

use alien_error::{Context, IntoAlienError};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt},
    net::{TcpListener, TcpStream},
};

use super::{failed, invalid, Result, MAX_HEADER};

pub(super) async fn accept(listener: &TcpListener) -> (TcpStream, SocketAddr) {
    loop {
        match listener.accept().await {
            Ok(connection) => return connection,
            Err(error) => {
                tracing::warn!(%error, "agent accept failed; retrying");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

pub(super) async fn read_header(stream: &mut (impl AsyncBufRead + Unpin)) -> Result<Vec<u8>> {
    let mut input = Vec::new();
    loop {
        let available = stream
            .fill_buf()
            .await
            .into_alien_error()
            .context(failed("read proxy header"))?;
        if available.is_empty() {
            return Err(invalid("incomplete proxy header"));
        }
        let before = input.len();
        let consumed = available.len().min(MAX_HEADER + 1 - before);
        input.extend_from_slice(&available[..consumed]);
        if let Some(end) = input[before.saturating_sub(3)..]
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .map(|index| before.saturating_sub(3) + index + 4)
        {
            if end > MAX_HEADER {
                return Err(invalid("proxy header too large"));
            }
            stream.consume(end - before);
            input.truncate(end);
            return Ok(input);
        }
        stream.consume(consumed);
        if input.len() > MAX_HEADER {
            return Err(invalid("proxy header too large"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn framing_preserves_coalesced_payload_and_split_delimiters() {
        let (mut writer, reader) = tokio::io::duplex(128);
        let task = tokio::spawn(async move {
            for part in [b"HTTP/1.1 200 OK\r\n\r".as_slice(), b"\npayload"] {
                writer.write_all(part).await.unwrap();
                tokio::task::yield_now().await;
            }
        });
        let mut reader = BufReader::with_capacity(7, reader);
        assert_eq!(
            read_header(&mut reader).await.unwrap(),
            b"HTTP/1.1 200 OK\r\n\r\n"
        );
        let mut payload = Vec::new();
        reader.read_to_end(&mut payload).await.unwrap();
        assert_eq!(payload, b"payload");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn oversized_and_truncated_heads_are_refused() {
        for bytes in [vec![b'x'; MAX_HEADER + 1], b"GET / HTTP/1.1\r\n".to_vec()] {
            let mut reader = BufReader::new(bytes.as_slice());
            assert!(read_header(&mut reader).await.is_err());
        }
    }
}
