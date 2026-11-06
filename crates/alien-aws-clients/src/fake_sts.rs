//! A local STS stand-in for proof verification tests.
//!
//! It answers GetCallerIdentity only when the request signature matches the
//! one its fixed role credentials produce for the received audience header.
//! That is the check real STS makes, so a verifier that drops or replaces the
//! audience header fails here the same way it fails against AWS.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, SystemTime},
};

use aws_credential_types::Credentials;
use chrono::NaiveDateTime;
use reqwest::Url;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::runtime_proof_signer::presign_caller_identity;

pub const ACCOUNT_ID: &str = "123456789012";
pub const ROLE_ID: &str = "AROAFIXTUREROLE00001";
pub const SESSION: &str = "fixture-session";

/// The role credentials whose signatures this stand-in accepts.
pub fn role_credentials() -> Credentials {
    Credentials::new(
        "ASIAFIXTUREACCESSKEY",
        "fixture-secret-access-key",
        Some("fixture-session-token".into()),
        None,
        "fake-sts",
    )
}

/// How the stand-in answers a correctly signed request.
#[derive(Clone, Copy)]
pub enum Answer {
    Identity,
    Unavailable,
}

pub struct FakeSts {
    pub transport: Url,
    pub region: String,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FakeSts {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeSts {
    pub async fn start(region: &str, answer: Answer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let transport = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        let region = region.to_owned();
        let served_region = region.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                counter.fetch_add(1, Ordering::SeqCst);
                let region = served_region.clone();
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") && head.len() < 16 * 1024 {
                        if stream.read(&mut byte).await.unwrap_or(0) == 0 {
                            return;
                        }
                        head.push(byte[0]);
                    }
                    let (status, body) = respond(&String::from_utf8_lossy(&head), &region, answer);
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        Self {
            transport,
            region,
            requests,
            task,
        }
    }

    /// Requests that reached the stand-in.
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    /// A proof signed by the fixture role for `audience`.
    pub fn proof(&self, audience: &str, signed_at: SystemTime, expires_in: Duration) -> String {
        presign_caller_identity(
            &role_credentials(),
            &self.region,
            audience,
            signed_at,
            expires_in,
        )
        .unwrap()
    }
}

fn respond(head: &str, region: &str, answer: Answer) -> (&'static str, String) {
    let mismatch = || {
        (
            "403 Forbidden",
            "<ErrorResponse><Error><Code>SignatureDoesNotMatch</Code></Error></ErrorResponse>"
                .to_owned(),
        )
    };
    let mut lines = head.split("\r\n");
    let Some(target) = lines.next().and_then(|line| line.split_whitespace().nth(1)) else {
        return mismatch();
    };
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let Ok(url) = Url::parse(&format!("https://sts.{region}.amazonaws.com{target}")) else {
        return mismatch();
    };
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let (Some(audience), Some(date), Some(expires), Some(signature)) = (
        headers.get("x-alien-manager-audience"),
        query.get("X-Amz-Date"),
        query
            .get("X-Amz-Expires")
            .and_then(|value| value.parse::<u64>().ok()),
        query.get("X-Amz-Signature"),
    ) else {
        return mismatch();
    };
    let Ok(signed_at) = NaiveDateTime::parse_from_str(date, "%Y%m%dT%H%M%SZ") else {
        return mismatch();
    };
    let signed_at: SystemTime = signed_at.and_utc().into();
    let expected = presign_caller_identity(
        &role_credentials(),
        region,
        audience,
        signed_at,
        Duration::from_secs(expires),
    )
    .ok()
    .and_then(|expected| Url::parse(&expected).ok())
    .and_then(|expected| {
        expected
            .query_pairs()
            .find(|(key, _)| key == "X-Amz-Signature")
            .map(|(_, value)| value.into_owned())
    });
    if expected.as_deref() != Some(signature.as_str()) {
        return mismatch();
    }
    match answer {
        Answer::Unavailable => ("503 Service Unavailable", String::new()),
        Answer::Identity => (
            "200 OK",
            format!(
                "<GetCallerIdentityResponse><GetCallerIdentityResult><Arn>arn:aws:sts::{ACCOUNT_ID}:assumed-role/fixture/{SESSION}</Arn><UserId>{ROLE_ID}:{SESSION}</UserId><Account>{ACCOUNT_ID}</Account></GetCallerIdentityResult></GetCallerIdentityResponse>"
            ),
        ),
    }
}
