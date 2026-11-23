//! Drives real gateway replicas on separate hosts, the way an app owning sandboxes does: it
//! signs the replicas' tokens, sets a sandbox's credential rules on every replica, and then acts
//! as the sandbox, sending requests through each replica's proxy with a placeholder value.
//!
//! Run in stages by the proof script (`ALIEN_PROOF_STAGE`):
//! `keygen` writes the signing key; `rules` covers inject, replace, empty and other-sandbox;
//! `restarted` checks a restarted replica serves the current rules; `away` expects a write to
//! fail while a replica is stopped; `back` expects it to succeed again.
//!
//! Environment: `ALIEN_PROOF_REPLICAS` (comma-separated replica IPs), `ALIEN_PROOF_KEY_FILE`,
//! `ALIEN_PROOF_LOG`, `ALIEN_PROOF_LINK_URL` (a linked echo service that answers with its request
//! headers), `ALIEN_PROOF_VALUES` (file the credential values are written to, for log greps).

use std::io::Write;
use std::sync::Arc;

use alien_bindings::sandbox_egress::SandboxEgressManager;
use alien_bindings::{Binding, SecretPresence, Vault};
use alien_core::bindings::{SandboxEgressGateway, SandboxEgressReplicas, SandboxEgressSigningKey};
use alien_core::{EgressPolicy, SandboxCredentialRule};

const AUDIENCE: &str = "proof/credentials";
const KEY: &str = "sandbox-egress-signing-key";
const PROXY_PORT: u16 = 3128;
const ADMIN_PORT: u16 = 3129;
const INTERNET: &str = "https://httpbin.org/headers";
const VALUES: [&str; 4] = [
    "first-turn-aws-6e0c41",
    "second-turn-aws-a93d27",
    "third-turn-aws-52f8b1",
    "fourth-turn-aws-0d7e93",
];

#[derive(Debug)]
struct Key(String);
impl Binding for Key {}
#[async_trait::async_trait]
impl Vault for Key {
    async fn get_secret(&self, name: &str) -> alien_bindings::error::Result<String> {
        assert_eq!(name, KEY);
        Ok(self.0.clone())
    }
    async fn secret_presence(&self, _: &str) -> alien_bindings::error::Result<SecretPresence> {
        Ok(SecretPresence::Present { version: None })
    }
    async fn set_secret(&self, _: &str, _: &str) -> alien_bindings::error::Result<()> {
        unreachable!()
    }
    async fn delete_secret(&self, _: &str) -> alien_bindings::error::Result<()> {
        unreachable!()
    }
    async fn list_secrets(&self) -> alien_bindings::error::Result<Vec<String>> {
        Ok(vec![KEY.to_string()])
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

struct Proof {
    log: std::fs::File,
    manager: SandboxEgressManager,
    replicas: Vec<String>,
    link: String,
}

impl Proof {
    fn note(&mut self, line: impl AsRef<str>) {
        eprintln!("{}", line.as_ref());
        writeln!(self.log, "{}", line.as_ref()).unwrap();
    }

    /// One request as the sandbox: through replica `index`'s proxy with the session's token,
    /// sending `header` with a placeholder (or nothing). Returns the echo's headers, lowercased.
    async fn get(&self, index: usize, session: &str, url: &str, header: Option<&str>) -> String {
        let token = self
            .manager
            .mint_identity(session, chrono::Utc::now().timestamp() + 600)
            .await
            .expect("token");
        let ca = self
            .manager
            .sandbox_ca(session)
            .await
            .expect("ca")
            .expect("a gateway that serves credentials issues a CA");
        let client = reqwest::Client::builder()
            .proxy(
                reqwest::Proxy::all(format!(
                    "http://alien:{token}@{}:{PROXY_PORT}",
                    self.replicas[index]
                ))
                .unwrap(),
            )
            .add_root_certificate(reqwest::Certificate::from_pem(ca.as_bytes()).unwrap())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        let mut request = client.get(url);
        if let Some(header) = header {
            request = request.header(header, "placeholder");
        }
        let response = request.send().await.expect("request through the proxy");
        assert!(
            response.status().is_success(),
            "{url}: {}",
            response.status()
        );
        response.text().await.unwrap().to_ascii_lowercase()
    }

    async fn both(&mut self, label: &str, session: &str, expect: Expect<'_>) {
        for index in 0..self.replicas.len() {
            let link = self
                .get(index, session, &self.link.clone(), Some("Authorization"))
                .await;
            let link_none = self.get(index, session, &self.link.clone(), None).await;
            let internet = self.get(index, session, INTERNET, Some("X-Api-Key")).await;
            self.note(format!(
                "{label} | replica {index} ({}) | {session}",
                self.replicas[index]
            ));
            match expect {
                Expect::Value(value) => {
                    self.note(format!(
                        "  link -> authorization present: {}",
                        link.contains(&format!("\"authorization\": \"bearer {value}\""))
                    ));
                    self.note(format!(
                        "  internet (HTTPS) -> x-api-key present: {}",
                        internet.contains(&format!("\"x-api-key\": \"{value}\""))
                    ));
                    assert!(
                        link.contains(&format!("\"authorization\": \"bearer {value}\"")),
                        "{link}"
                    );
                    assert!(
                        internet.contains(&format!("\"x-api-key\": \"{value}\"")),
                        "{internet}"
                    );
                    assert!(
                        link_none.contains(&format!("\"authorization\": \"bearer {value}\"")),
                        "{link_none}"
                    );
                    for other in VALUES.iter().filter(|other| **other != value) {
                        assert!(
                            !link.contains(other) && !internet.contains(other),
                            "stale value {other}"
                        );
                    }
                }
                Expect::Placeholder => {
                    self.note(
                        "  link -> authorization is the placeholder the sandbox sent".to_string(),
                    );
                    assert!(
                        link.contains("\"authorization\": \"placeholder\""),
                        "{link}"
                    );
                    assert!(
                        internet.contains("\"x-api-key\": \"placeholder\""),
                        "{internet}"
                    );
                    assert!(!link_none.contains("\"authorization\""), "{link_none}");
                    self.note("  link, no header sent -> no authorization header".to_string());
                    for value in VALUES {
                        assert!(!link.contains(value) && !internet.contains(value));
                    }
                }
            }
        }
    }

    async fn set(&mut self, session: &str, value: Option<&str>) -> Result<(), String> {
        let rules: Vec<SandboxCredentialRule> = match value {
            None => Vec::new(),
            Some(value) => serde_json::from_value(serde_json::json!([
                {"link":"connector","header":"Authorization","valuePrefix":"Bearer ","value":value},
                {"host":"httpbin.org","header":"X-Api-Key","value":value}
            ]))
            .unwrap(),
        };
        self.manager
            .update_credentials(session, &rules)
            .await
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Copy)]
enum Expect<'a> {
    Value(&'a str),
    Placeholder,
}

#[tokio::test]
#[ignore = "driven by the AWS proof script against real gateway replicas"]
async fn gateway_replicas_proof() {
    let stage = env("ALIEN_PROOF_STAGE");
    let key_file = env("ALIEN_PROOF_KEY_FILE");
    if stage == "keygen" {
        let key = alien_bindings::sandbox_egress_token::generate_signing_key();
        std::fs::write(
            &key_file,
            serde_json::json!({"document": key.document, "kid": key.kid, "public_key": key.public_key})
                .to_string(),
        )
        .unwrap();
        println!("KID={} PUBLIC_KEY={}", key.kid, key.public_key);
        return;
    }
    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&key_file).unwrap()).unwrap();
    let replicas: Vec<String> = env("ALIEN_PROOF_REPLICAS")
        .split(',')
        .map(String::from)
        .collect();
    let manager = SandboxEgressManager::new(
        SandboxEgressGateway {
            container: "proof-gateway".to_string(),
            audience: AUDIENCE.to_string(),
            signing_key: SandboxEgressSigningKey {
                vault: "proof".to_string(),
                key: KEY.to_string(),
            },
            kid: stored["kid"].as_str().unwrap().to_string(),
            admin_ca_pem: None,
            replicas: Some(SandboxEgressReplicas {
                dns: String::new(),
                addresses: replicas.clone(),
                port: ADMIN_PORT,
                count: replicas.len() as u32,
                min_protocol: 2,
                proxy_port: PROXY_PORT,
            }),
            links: Default::default(),
        },
        Arc::new(Key(stored["document"].as_str().unwrap().to_string())),
        &format!("http://{}:{PROXY_PORT}", replicas[0]),
        &format!("http://{}:{ADMIN_PORT}", replicas[0]),
    )
    .expect("manager");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(env("ALIEN_PROOF_LOG"))
        .unwrap();
    let mut proof = Proof {
        log,
        manager,
        replicas,
        link: env("ALIEN_PROOF_LINK_URL"),
    };
    std::fs::write(env("ALIEN_PROOF_VALUES"), VALUES.join("\n")).unwrap();
    proof.note(format!("=== stage {stage} ==="));

    match stage.as_str() {
        "rules" => {
            for session in ["s1", "s2"] {
                proof
                    .manager
                    .publish_new_grant(session, &EgressPolicy::Internet)
                    .await
                    .expect("grant");
            }
            proof.both("no rules", "s1", Expect::Placeholder).await;
            proof
                .set("s1", Some(VALUES[0]))
                .await
                .expect("ack on every replica");
            proof
                .both(
                    "first rules, right after the ack",
                    "s1",
                    Expect::Value(VALUES[0]),
                )
                .await;
            proof
                .both(
                    "other sandbox of the same resource",
                    "s2",
                    Expect::Placeholder,
                )
                .await;
            proof.set("s1", Some(VALUES[1])).await.expect("ack");
            proof
                .both(
                    "replaced rules, right after the ack",
                    "s1",
                    Expect::Value(VALUES[1]),
                )
                .await;
            proof.set("s1", None).await.expect("ack");
            proof.both("empty list", "s1", Expect::Placeholder).await;
            proof.set("s1", Some(VALUES[2])).await.expect("ack");
            proof
                .both("third rules", "s1", Expect::Value(VALUES[2]))
                .await;
        }
        "restarted" => {
            proof
                .both(
                    "after a replica restarted (no write since)",
                    "s1",
                    Expect::Value(VALUES[2]),
                )
                .await;
        }
        "away" => {
            let error = proof
                .set("s1", Some(VALUES[3]))
                .await
                .expect_err("a write must not be acknowledged while a replica is away");
            proof.note(format!("write with a replica stopped -> refused: {error}"));
            for value in VALUES {
                assert!(!error.contains(value), "the error carries a value");
            }
        }
        "back" => {
            proof
                .set("s1", Some(VALUES[3]))
                .await
                .expect("ack once the replica is back");
            proof
                .both(
                    "write after the replica returned",
                    "s1",
                    Expect::Value(VALUES[3]),
                )
                .await;
        }
        other => panic!("unknown stage {other}"),
    }
    proof.note(format!("=== stage {stage}: ok ==="));
}
