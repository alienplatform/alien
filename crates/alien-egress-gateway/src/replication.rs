//! Plain HTTP replication of signed policy records, authenticated by a gateway-only key.
use crate::{
    error::{denied, invalid, unavailable, Result},
    record::{Mutation, Record, Summary},
    runtime::LiveGateway,
};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::TryStreamExt;
use hickory_resolver::{
    net::{DnsError, NetError},
    proto::op::ResponseCode,
};
use hmac::{Hmac, Mac};
use serde::de::{Deserializer as _, SeqAccess, Visitor};
use sha2::{Digest, Sha256};
use std::io::{BufReader, Read};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

pub const MAX_SNAPSHOT: usize = 128 * 1024 * 1024;
pub struct Replication {
    service: Option<crate::config::Peers>,
    /// Every replica secret this process accepts, the current one first. Requests carry one
    /// signature per secret and any match authenticates, so replicas that know different
    /// subsets during a secret change still verify each other.
    secrets: Vec<[u8; 32]>,
    port: u16,
    resolver: hickory_resolver::TokioResolver,
    client: reqwest::Client,
}
impl Replication {
    pub fn new(
        service: Option<crate::config::Peers>,
        secrets: Vec<[u8; 32]>,
        port: u16,
        resolver: hickory_resolver::TokioResolver,
    ) -> Result<Self> {
        if service.is_some() && secrets.is_empty() {
            return Err(invalid("peers requires replicaSecret"));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .into_alien_error()
            .context(unavailable("build replica client"))?;
        Ok(Self {
            service,
            secrets,
            port,
            resolver,
            client,
        })
    }
    pub fn enabled(&self) -> bool {
        self.service.is_some()
    }
    /// The replica secrets this process accepts, the current one first.
    pub fn secrets(&self) -> &[[u8; 32]] {
        &self.secrets
    }
    fn mac(
        secret: &[u8; 32],
        method: &str,
        path: &str,
        time: u64,
        body: &[u8],
    ) -> Result<Hmac<Sha256>> {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(secret).map_err(|_| invalid("invalid replicaSecret"))?;
        mac.update(b"alien-egress-replica-v1\n");
        mac.update(method.as_bytes());
        mac.update(b"\n");
        mac.update(path.as_bytes());
        mac.update(b"\n");
        mac.update(&time.to_be_bytes());
        mac.update(&Sha256::digest(body));
        Ok(mac)
    }
    /// One base64 signature per accepted secret, comma-separated. A replica that knows a
    /// single secret sends one signature, the format every gateway version reads.
    fn signatures(&self, method: &str, path: &str, time: u64, body: &[u8]) -> Result<String> {
        if self.secrets.is_empty() {
            return Err(denied("replica exchange disabled"));
        }
        let mut signatures = Vec::with_capacity(self.secrets.len());
        for secret in &self.secrets {
            signatures.push(STANDARD.encode(
                Self::mac(secret, method, path, time, body)?
                    .finalize()
                    .into_bytes(),
            ));
        }
        Ok(signatures.join(","))
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
        let signatures = headers
            .get("x-alien-replica-signature")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| denied("replica signature missing"))?;
        if self.secrets.is_empty() {
            return Err(denied("replica exchange disabled"));
        }
        for signature in signatures.split(',').take(4) {
            let Ok(signature) = STANDARD.decode(signature.trim()) else {
                continue;
            };
            for secret in &self.secrets {
                if Self::mac(secret, method, path, time, body)?
                    .verify_slice(&signature)
                    .is_ok()
                {
                    return Ok(());
                }
            }
        }
        Err(denied("invalid replica signature"))
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
        .context(unavailable("replica discovery timeout"))?;
        let answers = match answers {
            // Ready-only service discovery has no records at first boot or after total loss.
            // Only DNS negative answers from the configured resolver count as empty;
            // transport errors and DNS failure codes must keep us unready.
            Err(NetError::Dns(DnsError::NoRecordsFound(ref negative)))
                if matches!(
                    negative.response_code,
                    ResponseCode::NXDomain | ResponseCode::NoError
                ) =>
            {
                return Ok(Vec::new());
            }
            result => result
                .into_alien_error()
                .context(unavailable("discover replicas"))?,
        };
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
    async fn send(&self, peer: SocketAddr, path: &str, body: Vec<u8>) -> Result<reqwest::Response> {
        let time = chrono::Utc::now().timestamp().max(0) as u64;
        let signature = self.signatures("POST", path, time, &body)?;
        self.client
            .post(format!("http://{peer}{path}"))
            .header("content-type", "application/json")
            .header("x-alien-replica-time", time)
            .header("x-alien-replica-signature", signature)
            .body(body)
            .send()
            .await
            .into_alien_error()
            .context(unavailable("contact replica"))
    }
    async fn request(&self, peer: SocketAddr, path: &str, body: Vec<u8>) -> Result<(u16, Vec<u8>)> {
        let mut response = self.send(peer, path, body).await?;
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
    async fn pull(
        &self,
        peer: SocketAddr,
        body: Vec<u8>,
        gateway: Arc<crate::server::Gateway>,
    ) -> Result<(usize, Vec<Summary>)> {
        let response = self.send(peer, "/v1/replicas/pull", body).await?;
        if response.status().as_u16() != 200 {
            return Err(AlienError::new(unavailable("replica snapshot unavailable")));
        }
        let stream = response.bytes_stream().map_err(std::io::Error::other);
        let bridge = tokio_util::io::SyncIoBridge::new(tokio_util::io::StreamReader::new(stream));
        // Serde consumes one string at a time on a blocking worker; no complete snapshot buffer.
        tokio::task::spawn_blocking(move || {
            let mut decoder = serde_json::Deserializer::from_reader(BufReader::with_capacity(
                64 * 1024,
                bridge.take(MAX_SNAPSHOT as u64),
            ));
            let count = decoder
                .deserialize_seq(RecordVisitor(gateway))
                .into_alien_error()
                .context(unavailable("stream replica snapshot"))?;
            decoder
                .end()
                .into_alien_error()
                .context(unavailable("finish replica snapshot"))?;
            Ok(count)
        })
        .await
        .into_alien_error()
        .context(unavailable("join snapshot reader"))?
    }
    /// A successful mutation has reached every discovered healthy replica.
    pub async fn fanout(&self, record: &Record) -> Result<()> {
        if record.expired(chrono::Utc::now().timestamp_millis().max(0) as u64) {
            return Ok(());
        }
        let body = serde_json::to_vec(&Mutation {
            record: record.signed.clone(),
        })
        .into_alien_error()
        .context(unavailable("encode replica record"))?;
        for peer in self.peers().await? {
            let (status, _) = self
                .request(peer, "/v1/replicas/write", body.clone())
                .await?;
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
        gateway.policies.expire_tombstones()?;
        for peer in self.peers().await? {
            let mut ignored_expired = Vec::new();
            loop {
                let mut known = gateway.policies.summary()?;
                known.extend(ignored_expired.iter().cloned());
                let request = serde_json::to_vec(&known)
                    .into_alien_error()
                    .context(unavailable("encode replica summary"))?;
                let (received, expired) = self.pull(peer, request, gateway.clone()).await?;
                ignored_expired.extend(expired);
                if received == 0 {
                    break;
                }
            }
            // Fetch a new summary after each bounded page so all stored policies can synchronize.
            loop {
                let (status, bytes) = self
                    .request(peer, "/v1/replicas/summary", b"null".to_vec())
                    .await?;
                if status != 200 {
                    return Err(AlienError::new(unavailable("replica summary unavailable")));
                }
                let known: Vec<Summary> = serde_json::from_slice(&bytes)
                    .into_alien_error()
                    .context(unavailable("decode replica summary"))?;
                let records = gateway
                    .policies
                    .newer_than(&known, &gateway.config.audiences)?;
                if records.is_empty() {
                    break;
                }
                for record in records {
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
        }
        gateway.policies.set_ready();
        Ok(())
    }
}

struct RecordVisitor(Arc<crate::server::Gateway>);
impl<'de> Visitor<'de> for RecordVisitor {
    type Value = (usize, Vec<Summary>);
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an array of compact signed records")
    }
    fn visit_seq<A: SeqAccess<'de>>(
        self,
        mut sequence: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        let mut count = 0;
        let mut expired = Vec::new();
        while let Some(signed) = sequence.next_element::<String>()? {
            let record = Record::verify(
                signed,
                &self.0.keys,
                &self.0.config.audiences,
                chrono::Utc::now().timestamp_millis().max(0) as u64,
            )
            .map_err(serde::de::Error::custom)?;
            if record.expired(chrono::Utc::now().timestamp_millis().max(0) as u64) {
                expired.push(Summary {
                    aud: record.claims.aud,
                    sub: record.claims.sub,
                    ver: record.claims.ver,
                });
            } else {
                self.0
                    .policies
                    .apply(record, false)
                    .map_err(serde::de::Error::custom)?;
            }
            count += 1;
        }
        Ok((count, expired))
    }
}

pub async fn run(live: Arc<LiveGateway>) -> Result<()> {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_sync = None;
    loop {
        tick.tick().await;
        if let Some(gateway) = live.snapshot() {
            if gateway.policies.ready()
                && last_sync.is_some_and(|last: tokio::time::Instant| {
                    last.elapsed() < Duration::from_secs(30)
                })
            {
                continue;
            }
            last_sync = Some(tokio::time::Instant::now());
            if let Err(error) = gateway.replication.catch_up(&live).await {
                tracing::warn!(code=%error.code, "replica synchronization failed");
            }
        }
    }
}

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ed25519_compact::{KeyPair, Seed};
    use hickory_resolver::{
        config::{ConnectionConfig, NameServerConfig, ResolverConfig},
        net::runtime::TokioRuntimeProvider,
        proto::{
            op::{Message, MessageType, OpCode, ResponseCode},
            rr::{rdata::SOA, Name, RData, Record as DnsRecord},
        },
        TokioResolver,
    };
    use std::collections::HashMap;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn authoritative_empty_peer_dns_is_empty_but_discovery_failures_are_errors() {
        for (response_code, soa, expected_empty) in [
            (Some(ResponseCode::NXDomain), true, true),
            (Some(ResponseCode::NoError), true, true),
            (Some(ResponseCode::NXDomain), false, true),
            (Some(ResponseCode::NoError), false, true),
            (Some(ResponseCode::Refused), true, false),
            (Some(ResponseCode::ServFail), true, false),
            (None, false, false),
        ] {
            let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let mut connection = ConnectionConfig::udp();
            connection.port = socket.local_addr().unwrap().port();
            let config = ResolverConfig::from_name_servers(vec![NameServerConfig::new(
                "127.0.0.1".parse().unwrap(),
                true,
                vec![connection],
            )]);
            let mut builder =
                TokioResolver::builder_with_config(config, TokioRuntimeProvider::default());
            builder.options_mut().attempts = 1;
            builder.options_mut().timeout = Duration::from_millis(100);
            builder.options_mut().cache_size = 0;
            let server = tokio::spawn(async move {
                let mut buffer = [0; 4096];
                loop {
                    let (len, peer) = socket.recv_from(&mut buffer).await.unwrap();
                    let request = Message::from_vec(&buffer[..len]).unwrap();
                    let Some(code) = response_code else {
                        continue;
                    };
                    let mut response =
                        Message::new(request.id, MessageType::Response, OpCode::Query);
                    response.metadata.recursion_desired = true;
                    response.metadata.recursion_available = true;
                    response.metadata.authoritative = true;
                    response.metadata.response_code = code;
                    response.queries = request.queries;
                    if soa {
                        response.authorities.push(DnsRecord::from_rdata(
                            Name::from_ascii("test.").unwrap(),
                            0,
                            RData::SOA(SOA::new(
                                Name::from_ascii("ns.test.").unwrap(),
                                Name::from_ascii("hostmaster.test.").unwrap(),
                                1,
                                1,
                                1,
                                1,
                                0,
                            )),
                        ));
                    }
                    socket
                        .send_to(&response.to_vec().unwrap(), peer)
                        .await
                        .unwrap();
                }
            });
            let replication = Replication::new(
                Some(crate::config::Peers::Dns(crate::config::DnsPeers {
                    dns: "empty.test".into(),
                    port: 3129,
                })),
                vec![[7; 32]],
                3129,
                builder.build().unwrap(),
            )
            .unwrap();
            let result = replication.peers().await;
            server.abort();
            assert_eq!(
                result.as_ref().is_ok_and(Vec::is_empty),
                expected_empty,
                "response={response_code:?}, soa={soa}, result={result:?}"
            );
            if !expected_empty {
                assert!(result.is_err());
            }
        }
    }

    #[tokio::test]
    async fn chunked_snapshot_decodes_one_signed_record_at_a_time() {
        let pair = KeyPair::from_seed(Seed::new([9; 32]));
        let bindings = Arc::new(
            alien_bindings::Bindings::from_env_map(HashMap::from([(
                "ALIEN_DEPLOYMENT_TYPE".into(),
                "local".into(),
            )]))
            .unwrap(),
        );
        let config = serde_json::json!({"listen":"127.0.0.1:3128","audiences":["p/r"],"keys":[{"kid":"test","publicKey":STANDARD.encode(pair.pk.as_ref())}],"deniedRanges":[],"policyMemoryBytes":1048576});
        let gateway = Arc::new(
            crate::runtime::build(
                &config.to_string(),
                bindings,
                Arc::new(std::sync::atomic::AtomicU64::new(0)),
                Arc::new(std::sync::atomic::AtomicU64::new(0)),
            )
            .await
            .unwrap(),
        );
        let mut signed = Vec::new();
        for index in 0..3 {
            let claims = serde_json::json!({"v":1,"aud":"p/r","sub":format!("box-{index}"),"ver":[1,"0".repeat(32)],"policy":{"allow":(0..1000).map(|i|format!("{i}.example.com")).collect::<Vec<_>>()}});
            let input = format!(
                "{}.{}",
                URL_SAFE_NO_PAD
                    .encode(br#"{"alg":"EdDSA","kid":"test","typ":"alien-egress-record"}"#),
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
            );
            signed.push(format!(
                "{input}.{}",
                URL_SAFE_NO_PAD.encode(pair.sk.sign(input.as_bytes(), None).as_ref())
            ));
        }
        let bytes = serde_json::to_vec(&signed).unwrap();
        let (reader, mut writer) = tokio::io::duplex(113);
        let bridge = tokio_util::io::SyncIoBridge::new(reader);
        let output = gateway.clone();
        let decoder = tokio::task::spawn_blocking(move || {
            let mut decoder =
                serde_json::Deserializer::from_reader(BufReader::with_capacity(4096, bridge));
            let count = decoder.deserialize_seq(RecordVisitor(output)).unwrap();
            decoder.end().unwrap();
            count
        });
        for chunk in bytes.chunks(67) {
            writer.write_all(chunk).await.unwrap();
        }
        drop(writer);
        assert_eq!(decoder.await.unwrap().0, 3);
        for index in 0..3 {
            assert!(gateway
                .policies
                .resolve("p/r", &format!("box-{index}"))
                .await
                .is_ok());
        }
    }
}
