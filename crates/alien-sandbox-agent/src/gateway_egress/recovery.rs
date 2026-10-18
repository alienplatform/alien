//! The agent retains one signed record in supervisor memory, outside command environments.
use super::{failed, invalid, read_header, GatewayEgress, Result, TIMEOUT};
use alien_error::{Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    time::timeout,
};

#[derive(Deserialize)]
struct Claims {
    v: u8,
    aud: String,
    sub: String,
    ver: (u64, String),
    #[serde(default)]
    deleted: bool,
}

fn claims(record: &str) -> Result<Claims> {
    if record.len() > 384 * 1024
        || record.split('.').count() != 3
        || !record
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(invalid("invalid policy record encoding"));
    }
    let payload = record
        .split('.')
        .nth(1)
        .ok_or_else(|| invalid("missing policy record claims"))?;
    let decoded = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| invalid("invalid policy record claims"))?;
    serde_json::from_slice(&decoded).map_err(|_| invalid("invalid policy record claims"))
}

impl GatewayEgress {
    pub(super) fn retain_record(&self, record: String, aud: &str, sub: &str) -> Result<()> {
        let incoming = claims(&record)?;
        if incoming.v != 1 || incoming.aud != aud || incoming.sub != sub || incoming.deleted {
            return Err(invalid("policy record identity mismatch"));
        }
        let mut current = self
            .policy
            .write()
            .map_err(|_| invalid("policy lock poisoned"))?;
        if let Some(previous) = current.as_ref() {
            if claims(previous)?.ver > incoming.ver {
                return Err(invalid("policy record would regress"));
            }
        }
        *current = Some(record);
        Ok(())
    }
    async fn policy_request(&self, record: Option<String>) -> Result<u16> {
        let address = self.gateway_address()?;
        let token = self.current_token()?;
        let mut gateway = BufReader::new(self.connect(address).await?);
        let request = if let Some(record) = record {
            let body = serde_json::to_string(&serde_json::json!({"record":record}))
                .into_alien_error()
                .context(failed("encode recovery record"))?;
            format!("PUT /v1/policies/register HTTP/1.1\r\nHost: {address}\r\nProxy-Authorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
        } else {
            format!("GET /health HTTP/1.1\r\nHost: {address}\r\nProxy-Authorization: Bearer {token}\r\nConnection: close\r\n\r\n")
        };
        timeout(TIMEOUT, async {
            gateway
                .write_all(request.as_bytes())
                .await
                .into_alien_error()
                .context(failed("send policy recovery request"))?;
            let response = read_header(&mut gateway).await?;
            std::str::from_utf8(&response)
                .ok()
                .and_then(|value| value.split_whitespace().nth(1))
                .and_then(|status| status.parse().ok())
                .ok_or_else(|| invalid("invalid recovery response"))
        })
        .await
        .into_alien_error()
        .context(failed("policy recovery timeout"))?
    }
    pub(super) async fn check_policy(&self) -> Result<()> {
        match self.policy_request(None).await? {
            200 => Ok(()),
            404 => {
                let record = self
                    .policy
                    .read()
                    .map_err(|_| invalid("policy lock poisoned"))?
                    .clone()
                    .ok_or_else(|| invalid("policy delivery pending"))?;
                let status = self.policy_request(Some(record)).await?;
                if matches!(status, 204 | 409) && self.policy_request(None).await? == 200 {
                    Ok(())
                } else {
                    Err(invalid("policy recovery refused"))
                }
            }
            _ => Err(invalid("gateway policy unavailable")),
        }
    }
    pub(super) async fn monitor_policy(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let _update = self.update_lock.lock().await;
            if self.awaiting_delivery.load(Ordering::Acquire)
                || !self.path_verified.load(Ordering::Acquire)
                || !self.current_token().is_ok_and(|token| !token.is_empty())
            {
                continue;
            }
            match self.check_policy().await {
                Ok(()) => self.ready.store(true, Ordering::Release),
                Err(error) => {
                    self.ready.store(false, Ordering::Release);
                    tracing::warn!(code=%error.code, "gateway policy check failed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway_egress::dns::Names;
    use std::sync::{atomic::AtomicBool, RwLock};
    use tokio::{io::AsyncReadExt, net::TcpListener};

    fn record(version: u64) -> String {
        format!("e30.{}.eA", URL_SAFE_NO_PAD.encode(serde_json::to_vec(&serde_json::json!({"v":1,"aud":"p/r","sub":"box","ver":[version,"00000000000000000000000000000000"],"policy":"internet"})).unwrap()))
    }
    fn agent(address: std::net::SocketAddr) -> GatewayEgress {
        GatewayEgress {
            address: RwLock::new(Some(address)),
            token: RwLock::new("token".into()),
            policy: RwLock::new(Some(record(2))),
            awaiting_delivery: AtomicBool::new(false),
            path_verified: AtomicBool::new(true),
            privileged: false,
            command_identity: crate::exec::ExecIdentity {
                uid: 60000,
                gid: 60000,
            },
            routing_ready: AtomicBool::new(true),
            declared_none: false,
            ready: AtomicBool::new(false),
            listeners_live: AtomicBool::new(true),
            names: Names::default(),
            image_started: AtomicBool::new(false),
            ca_path: RwLock::new(None),
            identity: RwLock::new(Some(("p/r".into(), "box".into()))),
            update_lock: tokio::sync::Mutex::new(()),
        }
    }
    #[tokio::test]
    async fn only_missing_state_triggers_reregistration_with_the_retained_record() {
        for statuses in [vec![403], vec![200], vec![404, 204, 200]] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let agent = agent(listener.local_addr().unwrap());
            let expected = record(2);
            let should_succeed = statuses[0] != 403;
            let server = tokio::spawn(async move {
                for (index, status) in statuses.into_iter().enumerate() {
                    let (socket, _) = listener.accept().await.unwrap();
                    let mut socket = BufReader::new(socket);
                    let header = read_header(&mut socket).await.unwrap();
                    let header = String::from_utf8(header).unwrap();
                    assert!(header.contains("Proxy-Authorization: Bearer token"));
                    if index == 1 {
                        assert!(header.starts_with("PUT /v1/policies/register "));
                        let length: usize = header
                            .lines()
                            .find_map(|line| line.strip_prefix("Content-Length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        let mut body = vec![0; length];
                        socket.read_exact(&mut body).await.unwrap();
                        assert_eq!(
                            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["record"],
                            expected
                        );
                    } else {
                        assert!(header.starts_with("GET /health "));
                    }
                    socket
                        .write_all(
                            format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\n\r\n")
                                .as_bytes(),
                        )
                        .await
                        .unwrap();
                }
                assert!(
                    tokio::time::timeout(Duration::from_millis(25), listener.accept())
                        .await
                        .is_err()
                );
            });
            assert_eq!(agent.check_policy().await.is_ok(), should_succeed);
            server.await.unwrap();
        }
    }
    #[test]
    fn saved_policy_never_regresses_and_pending_delivery_keeps_commands_closed() {
        let agent = agent("127.0.0.1:1".parse().unwrap());
        assert!(agent.retain_record(record(1), "p/r", "box").is_err());
        assert!(agent.retain_record(record(3), "p/r", "other").is_err());
        agent.retain_record(record(3), "p/r", "box").unwrap();
        agent.ready.store(true, Ordering::Release);
        assert!(agent.ensure_ready().is_ok());
        agent.awaiting_delivery.store(true, Ordering::Release);
        assert!(agent.ensure_ready().is_err());
        assert_eq!(agent.status().unwrap(), "awaiting-egress-policy");
        agent.path_verified.store(false, Ordering::Release);
        assert!(agent.status().is_err());
    }
}
