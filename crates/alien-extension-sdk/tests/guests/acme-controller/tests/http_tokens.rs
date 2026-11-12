//! The controller binary's HTTP server against requests it must refuse: every token failure
//! gets 401, and no reply carries a secret from the request or the token.

use alien_infra::step_protocol::token::{self, Claims};
use base64::Engine as _;
use ed25519_compact::{KeyPair, Seed};
use serde_json::{json, Value};
use std::process::{Child, Command};
use std::time::Duration;

const BINARY: &str = env!("CARGO_BIN_EXE_acme-controller");

/// Stops the server when the test ends, failed assertion or not.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn step(step_id: &str, platform: &str, credentials: Value) -> Value {
    json!({
        "protocol": 1, "stepId": step_id, "deploymentId": "dep_acme",
        "resourceId": "zone", "resourceType": "acme/dns-zone", "platform": platform,
        "resourcePrefix": "q6aa0b26", "initialSetupAuthority": "importedHandoff",
        "lifecycle": "frozen",
        "resource": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
        "previousResource": null, "dependencies": {},
        "deploymentConfig": {
            "stackSettings": {},
            "environmentVariables": { "variables": [], "hash": "", "createdAt": "" },
            "externalBindings": {},
            "allowFrozenChanges": false
        },
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": credentials, "deadline": "2099-01-01T00:00:00Z"
    })
}

fn claims(aud: &str, body: &Value, exp_offset: i64) -> Claims {
    let now = chrono::Utc::now().timestamp();
    Claims {
        iss: "https://manager.example.com".into(),
        aud: aud.into(),
        project: "prj_acme".into(),
        deployment_id: body["deploymentId"].as_str().unwrap().into(),
        extension: "acme".into(),
        resource_id: body["resourceId"].as_str().unwrap().into(),
        step_id: body["stepId"].as_str().unwrap().into(),
        // reqwest's `.json` sends these bytes.
        body_sha256: token::body_sha256(&serde_json::to_vec(body).unwrap()),
        iat: now,
        exp: now + exp_offset,
        jti: "j1".into(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_token_failure_gets_401_and_no_reply_holds_a_secret() {
    let manager = KeyPair::from_seed(Seed::new([7; 32]));
    let stranger = KeyPair::from_seed(Seed::new([8; 32]));
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}");
    let _server = KillOnDrop(
        Command::new(BINARY)
            .args(["--http", &format!("127.0.0.1:{port}"), "--audience", &url])
            .args(["--issuer", "https://manager.example.com"])
            .args(["--project", "prj_acme"])
            .args([
                "--trusted-key",
                &base64::engine::general_purpose::STANDARD.encode(manager.pk.as_ref()),
            ])
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::new();
    let endpoint = format!("{url}/v1/step");
    // The first start of a freshly linked binary can stall most of a minute while macOS scans it.
    for _ in 0..1200 {
        if client.get(&endpoint).send().await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let secret_creds = json!({
        "platform": "aws", "accountId": "123456789012", "region": "us-east-1",
        "accessKeyId": "ASIASECRETKEYID", "secretAccessKey": "SUPERSECRETVALUE",
        "sessionToken": "SESSIONTOKENVALUE", "expiresAt": null
    });

    let send = |token: Option<String>, body: Value| {
        let mut request = client.post(&endpoint).json(&body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        request.send()
    };

    let mut results = Vec::new();
    // 1. no token
    let body = step("s-none", "aws", secret_creds.clone());
    let r = send(None, body).await.unwrap();
    results.push(("no token", r.status().as_u16(), r.text().await.unwrap()));
    // 2. another audience
    let body = step("s-aud", "aws", secret_creds.clone());
    let t = token::mint(&claims("https://other.example", &body, 300), &manager.sk).unwrap();
    let r = send(Some(t.clone()), body).await.unwrap();
    let code = r.status().as_u16();
    let text = r.text().await.unwrap();
    assert!(!text.contains(&t), "the 401 body echoes the token");
    results.push(("other audience", code, text));
    // 3. expired
    let body = step("s-exp", "aws", secret_creds.clone());
    let t = token::mint(&claims(&url, &body, -5), &manager.sk).unwrap();
    let r = send(Some(t), body).await.unwrap();
    results.push(("expired", r.status().as_u16(), r.text().await.unwrap()));
    // 4. untrusted key
    let body = step("s-key", "aws", secret_creds.clone());
    let t = token::mint(&claims(&url, &body, 300), &stranger.sk).unwrap();
    let r = send(Some(t), body).await.unwrap();
    results.push((
        "untrusted key",
        r.status().as_u16(),
        r.text().await.unwrap(),
    ));
    // 5. token for another step id
    let body = step("s-mismatch", "aws", secret_creds.clone());
    let mut other = body.clone();
    other["stepId"] = json!("s-other");
    let t = token::mint(&claims(&url, &other, 300), &manager.sk).unwrap();
    let r = send(Some(t), body).await.unwrap();
    results.push(("other step", r.status().as_u16(), r.text().await.unwrap()));
    // 6. a valid token replayed for another deployment's body
    let body = step("s-dep", "aws", secret_creds.clone());
    let t = token::mint(&claims(&url, &body, 300), &manager.sk).unwrap();
    let mut other_dep = body.clone();
    other_dep["deploymentId"] = json!("dep_other");
    let r = send(Some(t), other_dep).await.unwrap();
    results.push((
        "other deployment",
        r.status().as_u16(),
        r.text().await.unwrap(),
    ));
    // 6b. a valid signature from another manager's issuer
    let body = step("s-iss", "aws", secret_creds.clone());
    let mut c = claims(&url, &body, 300);
    c.iss = "https://another-manager.example.com".into();
    let t = token::mint(&c, &manager.sk).unwrap();
    let r = send(Some(t), body).await.unwrap();
    results.push(("other issuer", r.status().as_u16(), r.text().await.unwrap()));
    // 6c. a token the manager signed for a deployment of another project
    let body = step("s-project", "aws", secret_creds.clone());
    let mut c = claims(&url, &body, 300);
    c.project = "prj_other".into();
    let t = token::mint(&c, &manager.sk).unwrap();
    let r = send(Some(t), body).await.unwrap();
    results.push((
        "other project",
        r.status().as_u16(),
        r.text().await.unwrap(),
    ));
    // 7. wrong protocol, no token
    let mut body = step("s-proto", "aws", secret_creds.clone());
    body["protocol"] = json!(2);
    let r = send(None, body).await.unwrap();
    results.push((
        "protocol 2, no token",
        r.status().as_u16(),
        r.text().await.unwrap(),
    ));
    // 8. unreadable body, no token
    let r = send(
        None,
        json!({"protocol": 1, "stepId": "x", "credentials": secret_creds}),
    )
    .await
    .unwrap();
    results.push((
        "bad body, no token",
        r.status().as_u16(),
        r.text().await.unwrap(),
    ));
    // 9. a step for a type with no controller, valid token
    let mut body = step("s-type", "aws", secret_creds.clone());
    body["resourceType"] = json!("acme/unknown");
    let t = token::mint(&claims(&url, &body, 300), &manager.sk).unwrap();
    let r = send(Some(t), body).await.unwrap();
    results.push(("unknown type", r.status().as_u16(), r.text().await.unwrap()));
    // 10. aws step with gcp credentials, valid token: message must not carry secrets
    let mut body = step(
        "s-mixed",
        "aws",
        json!({
            "platform": "gcp", "projectId": "p", "region": "r",
            "accessToken": "GCPACCESSTOKENVALUE", "expiresAt": "2099-01-01T00:00:00Z"
        }),
    );
    body["resourceType"] = json!("acme/dns-zone");
    let t = token::mint(&claims(&url, &body, 300), &manager.sk).unwrap();
    let r = send(Some(t), body).await.unwrap();
    results.push((
        "mixed credentials",
        r.status().as_u16(),
        r.text().await.unwrap(),
    ));

    for (name, _, text) in &results {
        for secret in [
            "SUPERSECRETVALUE",
            "SESSIONTOKENVALUE",
            "GCPACCESSTOKENVALUE",
        ] {
            assert!(!text.contains(secret), "{name} leaks {secret}: {text}");
        }
    }
    let status = |name: &str| results.iter().find(|r| r.0 == name).unwrap().1;
    for name in [
        "no token",
        "other audience",
        "expired",
        "untrusted key",
        "other step",
        "other deployment",
        "other issuer",
        "other project",
    ] {
        assert_eq!(status(name), 401, "{name}");
    }
    // A body this side can't read is refused before the token is looked at; a step the host
    // refuses is refused with a valid token.
    for name in [
        "protocol 2, no token",
        "bad body, no token",
        "unknown type",
        "mixed credentials",
    ] {
        assert_eq!(status(name), 400, "{name}");
    }
}
