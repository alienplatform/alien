use std::{
    io::{self, Cursor, Read},
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Stream counters used by the connection audit record.
#[derive(Default)]
pub struct Counters {
    /// Bytes received from the guest, including protocol framing.
    pub up: AtomicU64,
    /// Bytes sent to the guest, including protocol framing.
    pub down: AtomicU64,
}

/// Count actual I/O, including failures after a partial transfer.
pub struct Metered<S> {
    pub inner: S,
    pub counters: Arc<Counters>,
}
impl<S: AsyncRead + Unpin> AsyncRead for Metered<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        self.counters
            .up
            .fetch_add((buf.filled().len() - before) as u64, Ordering::Relaxed);
        result
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Metered<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = result {
            self.counters.down.fetch_add(n as u64, Ordering::Relaxed);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Replay inspected bytes to the HTTP/TLS library without reading them twice from the socket.
pub struct Prefixed<S> {
    prefix: Cursor<Vec<u8>>,
    inner: S,
}
impl<S> Prefixed<S> {
    /// Wrap a stream with bytes already read from it.
    pub fn new(prefix: Vec<u8>, inner: S) -> Self {
        Self {
            prefix: Cursor::new(prefix),
            inner,
        }
    }
}
impl<S: AsyncRead + Unpin> AsyncRead for Prefixed<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.prefix.position() < self.prefix.get_ref().len() as u64 {
            let n = Read::read(&mut self.prefix, buf.initialize_unfilled())?;
            buf.advance(n);
            Poll::Ready(Ok(()))
        } else {
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Prefixed<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
