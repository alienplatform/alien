//! The agent retains one signed record in supervisor memory, outside command environments.
use super::{failed, invalid, read_header, ErrorData, GatewayEgress, Result, TIMEOUT};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
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
    pub(super) async fn policy_request(&self, record: Option<String>) -> Result<u16> {
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
        match self.policy_check().await {
            PolicyCheck::Accepted => Ok(()),
            PolicyCheck::Rejected(error) | PolicyCheck::Unanswered(error) => Err(error),
        }
    }
    /// Start of the current run of checks that no gateway answered, shared by the monitor and
    /// lifecycle checks. Callers hold `update_lock`.
    fn unanswered_since(&self) -> std::sync::MutexGuard<'_, Option<Instant>> {
        self.unanswered_since
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    async fn policy_check(&self) -> PolicyCheck {
        let check = self.policy_check_once().await;
        if !matches!(check, PolicyCheck::Unanswered(_)) {
            *self.unanswered_since() = None;
        }
        check
    }
    async fn policy_check_once(&self) -> PolicyCheck {
        match self.policy_request(None).await {
            Ok(200) => PolicyCheck::Accepted,
            Ok(403) => {
                self.awaiting_delivery.store(true, Ordering::Release);
                PolicyCheck::answer(403)
            }
            Ok(404) => self.reregister().await,
            Ok(status) => PolicyCheck::answer(status),
            Err(error) => PolicyCheck::Unanswered(error),
        }
    }
    /// The gateway lost this sandbox's state; restore it from the retained signed record.
    async fn reregister(&self) -> PolicyCheck {
        let record = match self
            .policy
            .read()
            .map_err(|_| invalid("policy lock poisoned"))
            .and_then(|policy| {
                policy
                    .clone()
                    .ok_or_else(|| invalid("policy delivery pending"))
            }) {
            Ok(record) => record,
            Err(error) => return PolicyCheck::Rejected(error),
        };
        match self.policy_request(Some(record)).await {
            Ok(204 | 409) => {}
            Ok(status) => return PolicyCheck::answer(status),
            Err(error) => return PolicyCheck::Unanswered(error),
        }
        match self.policy_request(None).await {
            Ok(200) => PolicyCheck::Accepted,
            Ok(status) => PolicyCheck::answer(status),
            Err(error) => PolicyCheck::Unanswered(error),
        }
    }
    pub(super) async fn monitor_policy(self: Arc<Self>) {
        self.monitor_policy_with(MonitorTiming::PRODUCTION).await
    }
    async fn monitor_policy_with(self: Arc<Self>, timing: MonitorTiming) {
        loop {
            let mut retry = false;
            {
                let _update = self.update_lock.lock().await;
                let started = Instant::now();
                if self.awaiting_delivery.load(Ordering::Acquire)
                    || !self.path_verified.load(Ordering::Acquire)
                    || !self.current_token().is_ok_and(|token| !token.is_empty())
                {
                    *self.unanswered_since() = None;
                } else {
                    match self.policy_check().await {
                        PolicyCheck::Accepted => {
                            self.ready.store(true, Ordering::Release);
                        }
                        PolicyCheck::Rejected(error) => {
                            self.ready.store(false, Ordering::Release);
                            tracing::warn!(code=%error.code, "gateway policy check failed");
                        }
                        PolicyCheck::Unanswered(error) => {
                            let since = *self.unanswered_since().get_or_insert(started);
                            if since.elapsed() < timing.grace {
                                retry = true;
                                tracing::warn!(code=%error.code, "gateway policy check unanswered; retrying on a new connection");
                            } else if self.ready.swap(false, Ordering::AcqRel) {
                                tracing::warn!(code=%error.code, "gateway policy check failed");
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(if retry { timing.retry } else { timing.period }).await;
        }
    }
}

/// Result of one gateway policy check. Only a status a gateway sent is an answer.
enum PolicyCheck {
    Accepted,
    /// A gateway refused this sandbox: token rejected, tombstoned, or state not restorable.
    Rejected(AlienError<ErrorData>),
    /// No gateway answered: the connection failed, timed out or closed before a status, or the
    /// replica reported itself unavailable (5xx).
    Unanswered(AlienError<ErrorData>),
}

impl PolicyCheck {
    fn answer(status: u16) -> Self {
        let error = AlienError::new(ErrorData::EgressPathUnexpected);
        if (500..600).contains(&status) {
            Self::Unanswered(error)
        } else {
            Self::Rejected(error)
        }
    }
}

struct MonitorTiming {
    period: Duration,
    retry: Duration,
    grace: Duration,
}

impl MonitorTiming {
    /// Gateway replicas sit behind a load balancer that keeps sending some new connections to a
    /// replica that died until its health check removes it, which takes tens of seconds. A
    /// check that no gateway answers is therefore retried promptly on a new connection, which
    /// the load balancer can send to a live replica. Egress closes only when checks stay
    /// unanswered for `grace`; an attempt against a dead replica typically costs `TIMEOUT`, so
    /// that allows about ten independent attempts. A gateway answer refusing the sandbox closes
    /// egress immediately, and keeping readiness during the grace grants nothing: every
    /// forwarded connection is still authorized by a gateway.
    const PRODUCTION: Self = Self {
        period: Duration::from_secs(5),
        retry: Duration::from_millis(500),
        grace: Duration::from_secs(60),
    };
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
            unanswered_since: std::sync::Mutex::new(None),
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
        assert_eq!(
            claims(agent.policy.read().unwrap().as_ref().unwrap())
                .unwrap()
                .ver
                .0,
            3
        );
        agent.ready.store(true, Ordering::Release);
        assert!(agent.ensure_ready().is_ok());
        agent.awaiting_delivery.store(true, Ordering::Release);
        assert!(agent.ensure_ready().is_err());
        assert_eq!(agent.status().unwrap(), "awaiting-egress-policy");
        agent.path_verified.store(false, Ordering::Release);
        assert!(agent.status().is_err());
    }

    #[tokio::test]
    async fn record_only_delivery_preserves_token_and_cannot_release_resume() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let agent = agent(listener.local_addr().unwrap());
        let token = format!(
            "e30.{}.eA",
            URL_SAFE_NO_PAD.encode(br#"{"aud":"p/r","sub":"box"}"#)
        );
        *agent.token.write().unwrap() = token.clone();
        let expected_token = token.clone();
        let server = tokio::spawn(async move {
            for status in [200, 200, 403] {
                let (socket, _) = listener.accept().await.unwrap();
                let mut socket = BufReader::new(socket);
                let header = String::from_utf8(read_header(&mut socket).await.unwrap()).unwrap();
                assert!(header.starts_with("GET /health "));
                assert!(header.contains(&format!("Proxy-Authorization: Bearer {expected_token}")));
                socket
                    .write_all(
                        format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\n\r\n").as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        agent.ready.store(true, Ordering::Release);
        agent.update(None, record(3)).await.unwrap();
        assert!(agent.ensure_ready().is_ok());
        assert_eq!(agent.current_token().unwrap(), token);
        agent.awaiting_delivery.store(true, Ordering::Release);
        agent.update(None, record(4)).await.unwrap();
        assert!(agent.ensure_ready().is_err());
        assert_eq!(agent.current_token().unwrap(), token);
        assert!(agent.update(None, record(5)).await.is_err());
        assert_eq!(*agent.policy.read().unwrap(), Some(record(4)));
        server.await.unwrap();
        agent.token.write().unwrap().clear();
        assert!(agent.update(None, record(6)).await.is_err());
    }

    /// Answer one gateway request, reading its body when present.
    async fn answer(socket: tokio::net::TcpStream, status: u16) -> String {
        let mut socket = BufReader::new(socket);
        let header = String::from_utf8(read_header(&mut socket).await.unwrap()).unwrap();
        if let Some(length) = header
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
        {
            let mut body = vec![0; length.parse().unwrap()];
            socket.read_exact(&mut body).await.unwrap();
        }
        socket
            .write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\n\r\n").as_bytes())
            .await
            .unwrap();
        header
    }

    async fn wait_closed(agent: &GatewayEgress) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while agent.ready.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("egress stayed open after the gateway refused the sandbox");
        assert!(agent.ensure_ready().is_err());
    }

    #[tokio::test]
    async fn unanswered_check_retries_on_a_new_connection_without_closing_egress() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let agent = Arc::new(agent(listener.local_addr().unwrap()));
        agent.ready.store(true, Ordering::Release);
        let started = Instant::now();
        let monitor = tokio::spawn(agent.clone().monitor_policy());
        // A replica that died behind the load balancer: the connection closes with no status.
        drop(listener.accept().await.unwrap());
        let (socket, _) = listener.accept().await.unwrap();
        assert!(
            agent.ensure_ready().is_ok(),
            "one unanswered check closed egress"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the retry waited for the regular check period"
        );
        assert!(answer(socket, 200).await.starts_with("GET /health "));
        // An answered check returns to the regular period and egress stays open.
        assert!(
            tokio::time::timeout(Duration::from_secs(2), listener.accept())
                .await
                .is_err()
        );
        assert!(agent.ensure_ready().is_ok());
        monitor.abort();
    }

    #[tokio::test]
    async fn gateway_refusal_closes_egress_immediately_even_while_retrying() {
        // (unanswered connections first, gateway answers, token rejected)
        for (unanswered, statuses, rejected) in [
            (0, vec![403], true),
            (1, vec![403], true),
            (1, vec![404, 403], false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let agent = Arc::new(agent(listener.local_addr().unwrap()));
            agent.ready.store(true, Ordering::Release);
            let monitor = tokio::spawn(agent.clone().monitor_policy());
            for _ in 0..unanswered {
                drop(listener.accept().await.unwrap());
            }
            for status in statuses {
                let (socket, _) = listener.accept().await.unwrap();
                answer(socket, status).await;
            }
            wait_closed(&agent).await;
            assert_eq!(agent.awaiting_delivery.load(Ordering::Acquire), rejected);
            monitor.abort();
        }
    }

    #[tokio::test]
    async fn answered_lifecycle_check_restarts_the_grace() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let agent = Arc::new(agent(listener.local_addr().unwrap()));
        agent.ready.store(true, Ordering::Release);
        let answer_next = Arc::new(AtomicBool::new(false));
        let server = tokio::spawn({
            let answer_next = answer_next.clone();
            async move {
                loop {
                    let (socket, _) = listener.accept().await.unwrap();
                    if answer_next.swap(false, Ordering::AcqRel) {
                        answer(socket, 200).await;
                    }
                }
            }
        });
        let grace = Duration::from_millis(400);
        let started = Instant::now();
        let monitor = tokio::spawn(agent.clone().monitor_policy_with(MonitorTiming {
            period: Duration::from_millis(100),
            retry: Duration::from_millis(10),
            grace,
        }));
        tokio::time::sleep(Duration::from_millis(250)).await;
        // A lifecycle check (token delivery or Resume) reaches a live gateway mid-streak.
        let answered = {
            let _update = agent.update_lock.lock().await;
            answer_next.store(true, Ordering::Release);
            agent.check_policy().await.unwrap();
            Instant::now()
        };
        while agent.ready.load(Ordering::Acquire) {
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "egress never closed"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(
            answered.elapsed() >= grace,
            "closed {:?} after an answered check, before a new grace",
            answered.elapsed()
        );
        monitor.abort();
        server.abort();
    }

    #[tokio::test]
    async fn persistent_unreachability_closes_egress_after_the_grace_and_reopens_on_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let agent = Arc::new(agent(listener.local_addr().unwrap()));
        agent.ready.store(true, Ordering::Release);
        let reachable = Arc::new(AtomicBool::new(false));
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let server = tokio::spawn({
            let (reachable, attempts) = (reachable.clone(), attempts.clone());
            async move {
                loop {
                    let (socket, _) = listener.accept().await.unwrap();
                    attempts.fetch_add(1, Ordering::AcqRel);
                    if reachable.load(Ordering::Acquire) {
                        answer(socket, 200).await;
                    }
                }
            }
        });
        let grace = Duration::from_millis(300);
        let started = Instant::now();
        let monitor = tokio::spawn(agent.clone().monitor_policy_with(MonitorTiming {
            period: Duration::from_millis(100),
            retry: Duration::from_millis(10),
            grace,
        }));
        while agent.ready.load(Ordering::Acquire) {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "egress never closed"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let closed = started.elapsed();
        assert!(closed >= grace, "closed after {closed:?}, before the grace");
        assert!(
            closed < grace * 2,
            "closed after {closed:?}, well past the grace"
        );
        assert!(
            attempts.load(Ordering::Acquire) >= 5,
            "unanswered checks were not retried"
        );
        assert!(agent.ensure_ready().is_err());
        assert!(!agent.awaiting_delivery.load(Ordering::Acquire));
        reachable.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(1), async {
            while !agent.ready.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("egress did not reopen once a gateway answered");
        assert!(agent.ensure_ready().is_ok());
        monitor.abort();
        server.abort();
    }
}
