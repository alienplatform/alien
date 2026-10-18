//! Proxy-port recovery is authenticated by a live sandbox token and only fills absent state.
use crate::{
    error::{denied, invalid, unavailable, Result},
    record::{Mutation, Record, MAX_RECORD_BYTES},
    server::Gateway,
    token::Claims,
};
use alien_error::{Context, IntoAlienError};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub async fn handle(
    stream: &mut (impl AsyncRead + AsyncWrite + Unpin),
    gateway: &Gateway,
    claims: &Claims,
    input: &[u8],
    head_len: usize,
) -> Result<bool> {
    if !gateway.config.audiences.contains(&claims.aud) {
        return reply(stream, 403).await;
    }
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    request
        .parse(input)
        .map_err(|_| invalid("invalid recovery request"))?;
    if request.method == Some("GET") {
        let status = match gateway.policies.record(&claims.aud, &claims.sub)? {
            None => 404,
            Some(record) if record.claims.deleted => 403,
            Some(_) => 200,
        };
        return reply(stream, status).await;
    }
    let mut length = None;
    for header in request.headers {
        if header.name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid("recovery requires Content-Length"));
        }
        if header.name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(invalid("duplicate Content-Length"));
            }
            length = Some(
                std::str::from_utf8(header.value)
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or_else(|| invalid("invalid Content-Length"))?,
            );
        }
    }
    let length = length.ok_or_else(|| invalid("Content-Length required"))?;
    if length > MAX_RECORD_BYTES + 1024 {
        return Err(invalid("recovery record too large"));
    }
    let buffered = &input[head_len..];
    if buffered.len() > length {
        return Err(invalid("unexpected recovery payload"));
    }
    let mut body = vec![0; length];
    body[..buffered.len()].copy_from_slice(buffered);
    stream
        .read_exact(&mut body[buffered.len()..])
        .await
        .into_alien_error()
        .context(unavailable("read recovery record"))?;
    let mutation: Mutation =
        serde_json::from_slice(&body).map_err(|_| invalid("invalid recovery record"))?;
    let record = Record::verify(
        mutation.record,
        &gateway.keys,
        &gateway.config.audiences,
        chrono::Utc::now().timestamp_millis().max(0) as u64,
    )?;
    if record.claims.aud != claims.aud || record.claims.sub != claims.sub || record.claims.deleted {
        return Err(denied("recovery identity mismatch"));
    }
    // Applying locally never overwrites a policy or a tombstone, even after a racing update.
    gateway.policies.apply(record.clone(), true)?;
    if gateway.replication.fanout(&record, true).await.is_err() {
        return reply(stream, 503).await;
    }
    let status = match gateway.policies.record(&claims.aud, &claims.sub)? {
        Some(current) if current.claims.ver == record.claims.ver && !current.claims.deleted => 204,
        _ => 409,
    };
    reply(stream, status).await
}

async fn reply(stream: &mut (impl AsyncWrite + Unpin), status: u16) -> Result<bool> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Service Unavailable",
    };
    stream
        .write_all(
            format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .into_alien_error()
        .context(unavailable("reply to recovery request"))?;
    Ok(matches!(status, 200 | 204))
}
