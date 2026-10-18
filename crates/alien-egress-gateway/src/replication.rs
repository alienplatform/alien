//! Plain HTTP replication of signed policy records, authenticated by a gateway-only key.
use crate::{
    error::{denied, invalid, unavailable, Result},
    record::{Mutation, Record, Summary},
    runtime::LiveGateway,
};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

pub const MAX_SNAPSHOT: usize = 128 * 1024 * 1024;
pub struct Replication {
    service: Option<crate::config::Peers>,
    secret: Option<[u8; 32]>,
    port: u16,
    resolver: hickory_resolver::TokioResolver,
    client: reqwest::Client,
}
impl Replication {
    pub fn new(
        service: Option<crate::config::Peers>,
        secret: Option<[u8; 32]>,
        port: u16,
        resolver: hickory_resolver::TokioResolver,
    ) -> Result<Self> {
        if service.is_some() && secret.is_none() {
            return Err(invalid("peerService requires replicaSecret"));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .into_alien_error()
            .context(unavailable("build replica client"))?;
        Ok(Self {
            service,
            secret,
            port,
            resolver,
            client,
        })
    }
    pub fn enabled(&self) -> bool {
        self.service.is_some()
    }
    fn mac(&self, method: &str, path: &str, time: u64, body: &[u8]) -> Result<Hmac<Sha256>> {
        let mut mac = Hmac::<Sha256>::new_from_slice(
            &self
                .secret
                .ok_or_else(|| denied("replica exchange disabled"))?,
        )
        .map_err(|_| invalid("invalid replicaSecret"))?;
        mac.update(b"alien-egress-replica-v1\n");
        mac.update(method.as_bytes());
        mac.update(b"\n");
        mac.update(path.as_bytes());
        mac.update(b"\n");
        mac.update(&time.to_be_bytes());
        mac.update(&Sha256::digest(body));
        Ok(mac)
    }
    pub fn authenticate(
        &self,
        headers: &axum::http::HeaderMap,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Result<()> {
        let time: u64 = headers
            .get("x-alien-replica-time")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| denied("replica timestamp missing"))?;
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        if now.abs_diff(time) > 60 {
            return Err(denied("replica timestamp expired"));
        }
        let signature = headers
            .get("x-alien-replica-signature")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| denied("replica signature missing"))?;
        let signature = STANDARD
            .decode(signature)
            .map_err(|_| denied("invalid replica signature"))?;
        self.mac(method, path, time, body)?
            .verify_slice(&signature)
            .map_err(|_| denied("invalid replica signature"))
    }
    async fn peers(&self) -> Result<Vec<SocketAddr>> {
        let Some(service) = &self.service else {
            return Ok(Vec::new());
        };
        if let crate::config::Peers::Static(peers) = service {
            let mut addresses = Vec::new();
            for address in &peers.r#static {
                addresses.extend(
                    tokio::net::lookup_host(address)
                        .await
                        .into_alien_error()
                        .context(unavailable("resolve static peer"))?,
                );
            }
            addresses.sort();
            addresses.dedup();
            return Ok(addresses);
        }
        let crate::config::Peers::Dns(service) = service else {
            unreachable!()
        };
        let answers = tokio::time::timeout(
            Duration::from_secs(5),
            self.resolver
                .ipv4_lookup(format!("{}.", service.dns.trim_end_matches('.'))),
        )
        .await
        .into_alien_error()
        .context(unavailable("replica discovery timeout"))?
        .into_alien_error()
        .context(unavailable("discover replicas"))?;
        let mut peers = Vec::new();
        for ip in answers
            .answers()
            .iter()
            .filter_map(|record| record.data.ip_addr())
            .filter(IpAddr::is_ipv4)
        {
            // The selected route identifies our source interface even on multi-homed containers.
            // UDP connect sends no packet; it avoids an extra configured replica identifier.
            let socket = tokio::net::UdpSocket::bind("0.0.0.0:0")
                .await
                .into_alien_error()
                .context(unavailable("identify replica address"))?;
            socket
                .connect(SocketAddr::new(ip, self.port))
                .await
                .into_alien_error()
                .context(unavailable("route to replica"))?;
            if socket
                .local_addr()
                .into_alien_error()
                .context(unavailable("read replica address"))?
                .ip()
                != ip
            {
                peers.push(SocketAddr::new(ip, service.port));
            }
        }
        peers.sort();
        peers.dedup();
        Ok(peers)
    }
    async fn request(&self, peer: SocketAddr, path: &str, body: Vec<u8>) -> Result<(u16, Vec<u8>)> {
        let time = chrono::Utc::now().timestamp().max(0) as u64;
        let signature =
            STANDARD.encode(self.mac("POST", path, time, &body)?.finalize().into_bytes());
        let mut response = self
            .client
            .post(format!("http://{peer}{path}"))
            .header("content-type", "application/json")
            .header("x-alien-replica-time", time)
            .header("x-alien-replica-signature", signature)
            .body(body)
            .send()
            .await
            .into_alien_error()
            .context(unavailable("contact replica"))?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .into_alien_error()
            .context(unavailable("read replica response"))?
        {
            if bytes.len() + chunk.len() > MAX_SNAPSHOT {
                return Err(invalid("replica snapshot too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((status, bytes))
    }
    /// A successful mutation has reached every discovered healthy replica.
    pub async fn fanout(&self, record: &Record, recovery: bool) -> Result<()> {
        let body = serde_json::to_vec(&Mutation {
            record: record.signed.clone(),
        })
        .into_alien_error()
        .context(unavailable("encode replica record"))?;
        for peer in self.peers().await? {
            let path = if recovery {
                "/v1/replicas/register"
            } else {
                "/v1/replicas/write"
            };
            let (status, _) = self.request(peer, path, body.clone()).await?;
            if status != 204 {
                return Err(AlienError::new(unavailable(
                    "replica did not acknowledge policy",
                )));
            }
        }
        Ok(())
    }
    pub async fn catch_up(&self, live: &LiveGateway) -> Result<()> {
        let gateway = live
            .snapshot()
            .ok_or_else(|| denied("config unavailable"))?;
        let summary = gateway.policies.summary()?;
        let request = serde_json::to_vec(&summary)
            .into_alien_error()
            .context(unavailable("encode replica summary"))?;
        for peer in self.peers().await? {
            let (status, bytes) = self
                .request(peer, "/v1/replicas/pull", request.clone())
                .await?;
            if status != 200 {
                return Err(AlienError::new(unavailable("replica snapshot unavailable")));
            }
            let records: Vec<String> = serde_json::from_slice(&bytes)
                .into_alien_error()
                .context(unavailable("decode replica snapshot"))?;
            for record in records {
                let record = Record::verify(
                    record,
                    &gateway.keys,
                    &gateway.config.audiences,
                    chrono::Utc::now().timestamp_millis().max(0) as u64,
                )?;
                gateway.policies.apply(record, false)?;
            }
            // Send our newer records too, so a write racing this peer's restart is repaired.
            let (status, bytes) = self
                .request(peer, "/v1/replicas/summary", b"null".to_vec())
                .await?;
            if status != 200 {
                return Err(AlienError::new(unavailable("replica summary unavailable")));
            }
            let known: Vec<Summary> = serde_json::from_slice(&bytes)
                .into_alien_error()
                .context(unavailable("decode replica summary"))?;
            for record in gateway.policies.newer_than(&known)? {
                let bytes = serde_json::to_vec(&Mutation {
                    record: record.signed.clone(),
                })
                .into_alien_error()
                .context(unavailable("encode replica update"))?;
                if self.request(peer, "/v1/replicas/write", bytes).await?.0 != 204 {
                    return Err(AlienError::new(unavailable("replica catch-up refused")));
                }
            }
        }
        gateway.policies.set_ready();
        Ok(())
    }
}

pub async fn run(live: Arc<LiveGateway>) -> Result<()> {
    loop {
        let delay = if let Some(gateway) = live.snapshot() {
            if let Err(error) = gateway.replication.catch_up(&live).await {
                tracing::warn!(code=%error.code,"replica synchronization failed");
            }
            if gateway.policies.ready() {
                Duration::from_secs(30)
            } else {
                Duration::from_secs(1)
            }
        } else {
            Duration::from_secs(1)
        };
        tokio::time::sleep(delay).await;
    }
}
