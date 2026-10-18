use std::io::Cursor;

use alien_error::{Context, IntoAlienError};
use rustls::server::Acceptor;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::{
    error::{denied, invalid, unavailable, Result},
    policy::Target,
};

/// Limit for the entire CONNECT/request header block.
pub const MAX_HEADER_BYTES: usize = 32 * 1024;
/// Limit for a fragmented TLS ClientHello, including TLS record framing.
pub const MAX_HELLO_BYTES: usize = 64 * 1024;

/// Inspect a complete HTTP request header. Used by fuzzing and the transparent forwarder.
pub fn request_target(input: &[u8]) -> Result<Option<Target>> {
    if input.len() > MAX_HEADER_BYTES {
        return Err(invalid("request headers too large"));
    }
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    if request
        .parse(input)
        .map_err(|_| invalid("malformed HTTP"))?
        .is_partial()
    {
        return Ok(None);
    }
    let method = request.method.ok_or_else(|| invalid("missing method"))?;
    let path = request
        .path
        .ok_or_else(|| invalid("missing request target"))?;
    let target = if method == "CONNECT" {
        Target::parse(path, None)?
    } else {
        let uri: http::Uri = path.parse().map_err(|_| invalid("invalid request URI"))?;
        if uri.scheme_str() != Some("http") {
            return Err(invalid("plain proxy requires http scheme"));
        }
        Target::parse(
            uri.authority()
                .ok_or_else(|| invalid("missing authority"))?
                .as_str(),
            Some(80),
        )?
    };
    let hosts: Vec<_> = request
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("host"))
        .collect();
    if hosts.len() != 1 {
        return Err(invalid("exactly one Host required"));
    }
    let host = std::str::from_utf8(hosts[0].value).map_err(|_| invalid("invalid Host"))?;
    if Target::parse(host, Some(target.port))? != target {
        return Err(denied("Host mismatch"));
    }
    Ok(Some(target))
}

/// Parse a bounded ClientHello using rustls' protocol parser, including fragmented records.
/// None means the input is incomplete; malformed or missing SNI is an error.
pub fn client_hello_name(input: &[u8]) -> Result<Option<String>> {
    if input.len() > MAX_HELLO_BYTES {
        return Err(denied("ClientHello too large"));
    }
    let mut acceptor = Acceptor::default();
    let mut cursor = Cursor::new(input);
    loop {
        let n = acceptor
            .read_tls(&mut cursor)
            .into_alien_error()
            .context(unavailable("parse ClientHello"))?;
        if let Some(accepted) = acceptor
            .accept()
            .map_err(|_| denied("invalid ClientHello"))?
        {
            return Ok(Some(
                accepted
                    .client_hello()
                    .server_name()
                    .ok_or_else(|| denied("SNI required"))?
                    .to_string(),
            ));
        }
        if n == 0 {
            return Ok(None);
        }
    }
}

/// Read only as far as the ClientHello before any bytes reach the destination.
pub async fn read_client_hello(
    stream: &mut (impl AsyncRead + Unpin),
    target: &Target,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut acceptor = Acceptor::default();
    loop {
        let mut chunk = [0u8; 4096];
        let n = stream
            .read(&mut chunk)
            .await
            .into_alien_error()
            .context(unavailable("read ClientHello"))?;
        if n == 0 {
            return Err(denied("incomplete ClientHello"));
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > MAX_HELLO_BYTES {
            return Err(denied("ClientHello too large"));
        }
        // Feed each byte once, even when a peer fragments every TLS record.
        let mut cursor = Cursor::new(&chunk[..n]);
        while (cursor.position() as usize) < n {
            let read = acceptor
                .read_tls(&mut cursor)
                .into_alien_error()
                .context(unavailable("parse ClientHello"))?;
            if let Some(accepted) = acceptor
                .accept()
                .map_err(|_| denied("invalid ClientHello"))?
            {
                let hello = accepted.client_hello();
                let name = hello.server_name().ok_or_else(|| denied("SNI required"))?;
                if Target::parse(name, Some(target.port))? != *target {
                    return Err(denied("SNI mismatch"));
                }
                return Ok(bytes);
            }
            if read == 0 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn client_hello_can_arrive_one_byte_at_a_time() {
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
        let mut client =
            rustls::ClientConnection::new(Arc::new(config), "example.com".try_into().unwrap())
                .unwrap();
        let mut hello = Vec::new();
        client.write_tls(&mut hello).unwrap();
        let (mut sender, mut receiver) = tokio::io::duplex(1);
        let expected = hello.clone();
        let writer = tokio::spawn(async move {
            sender.write_all(&hello).await.unwrap();
        });
        assert_eq!(
            read_client_hello(
                &mut receiver,
                &Target::parse("example.com:443", None).unwrap()
            )
            .await
            .unwrap(),
            expected
        );
        writer.await.unwrap();
    }

    #[test]
    fn connect_requires_unambiguous_matching_host() {
        assert_eq!(
            request_target(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
                .unwrap()
                .unwrap()
                .host,
            "example.com"
        );
        for input in [
            b"CONNECT example.com:443 HTTP/1.1\r\nHost: other.com:443\r\n\r\n".as_slice(),
            b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\nHost: example.com:443\r\n\r\n",
            b"GET http://example.com/ HTTP/1.1\r\nHost: other.com\r\n\r\n",
        ] { assert!(request_target(input).is_err()); }
    }
}
