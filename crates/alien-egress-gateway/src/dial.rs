//! Bounded failover among addresses that the caller already checked as a complete set.
use std::{io, net::SocketAddr, time::Duration};

use tokio::{
    net::TcpStream,
    time::{timeout_at, Instant},
};

const ADDRESS_TIMEOUT: Duration = Duration::from_secs(1);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn connect(addresses: &[SocketAddr]) -> io::Result<TcpStream> {
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no destination addresses");
    for address in addresses {
        if Instant::now() >= deadline {
            break;
        }
        let attempt = (Instant::now() + ADDRESS_TIMEOUT).min(deadline);
        match timeout_at(attempt, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => last = error,
            Err(_) => last = io::Error::new(io::ErrorKind::TimedOut, "destination address timeout"),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[tokio::test]
    async fn refused_address_falls_through_to_a_working_stream() {
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let refused = closed.local_addr().unwrap();
        drop(closed);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = connect(&[refused, address]).await.unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        client.write_all(b"connected").await.unwrap();
        let mut bytes = [0; 9];
        server.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"connected");
        assert_eq!(client.peer_addr().unwrap(), address);
        assert_eq!(
            connect(&[]).await.unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
