//! The signing commands against a real manager (`alien serve` on SQLite):
//! an admin sets up the signer list, a second engineer's device is approved
//! and later removed, CI signs a release, and a manager that forges a signer
//! list is caught by the CLI.

#![cfg(unix)]

use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

use alien_release_trust::{
    Anchor, Envelope, Kind, ReleaseBody, Role, SignedStatement, Signer, SignerKind, SignerListBody,
    SoftwareKey, Trust,
};
use tempfile::TempDir;

const TOKEN: &str = "ax_admin_signingflowtesttoken0000000000000000";

struct Manager {
    child: Child,
    url: String,
    _state: TempDir,
}

impl Drop for Manager {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn start_manager() -> Manager {
    let state = TempDir::new().unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = state.path().join("alien-manager.toml");
    std::fs::write(
        &config,
        format!(
            "[server]\nport = {port}\nhost = \"127.0.0.1\"\n\n[database]\nstate-dir = \"{}\"\n",
            state.path().join("state").display()
        ),
    )
    .unwrap();
    let log = std::fs::File::create(state.path().join("manager.log")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_alien"))
        .args(["serve", "--config"])
        .arg(&config)
        .env("ALIEN_ADMIN_TOKEN", TOKEN)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let url = format!("http://127.0.0.1:{port}");
    let started = Instant::now();
    let client = reqwest::Client::new();
    loop {
        let up = client
            .get(format!("{url}/health"))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
        if up {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "the manager did not start: {}",
            std::fs::read_to_string(state.path().join("manager.log")).unwrap_or_default()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Manager {
        child,
        url,
        _state: state,
    }
}

/// Run `alien` against `manager` with signing state in `home`.
fn alien(manager: &Manager, home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_alien"))
        .args(args)
        .current_dir(cwd)
        .env("ALIEN_MANAGER_URL", &manager.url)
        .env("ALIEN_API_KEY", TOKEN)
        .env("ALIEN_SIGNING_HOME", home)
        .env_remove("ALIEN_SIGNING_KEY")
        .env_remove("ALIEN_SIGNING_FINGERPRINT")
        .output()
        .unwrap()
}

fn ok(output: Output) -> String {
    assert!(
        output.status.success(),
        "alien failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn failed(output: Output) -> String {
    assert!(
        !output.status.success(),
        "alien succeeded but should have failed:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|error| panic!("{error}: {text}"))
}

fn signer_ids(list: &serde_json::Value) -> Vec<String> {
    let mut ids: Vec<String> = list["signers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|signer| signer["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

async fn stored_lists(manager: &Manager) -> Vec<SignedStatement> {
    let body: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/v1/projects/default/signer-lists", manager.url))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    serde_json::from_value(body["items"].clone()).unwrap()
}

#[tokio::test]
async fn signing_setup_approval_release_and_a_forged_list() {
    let manager = start_manager().await;
    let work = TempDir::new().unwrap();
    let admin_home = work.path().join("admin");
    let bob_home = work.path().join("bob");
    let ci_key = work.path().join("ci.pem");
    let recovery_key = work.path().join("recovery.pem");

    // The admin sets up signing.
    let init = json(&ok(alien(
        &manager,
        &admin_home,
        work.path(),
        &[
            "signing",
            "init",
            "--backend",
            "software",
            "--ci-key-out",
            ci_key.to_str().unwrap(),
            "--recovery-key-out",
            recovery_key.to_str().unwrap(),
            "--json",
        ],
    )));
    let fingerprint = init["fingerprint"].as_str().unwrap().to_string();
    assert!(fingerprint.starts_with("SHA256:"), "{init}");
    let stored = stored_lists(&manager).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(
        alien_release_trust::fingerprint(&stored[0].open().unwrap().digest),
        fingerprint,
        "the manager returns version 1 byte for byte"
    );
    let list = json(&ok(alien(
        &manager,
        &admin_home,
        work.path(),
        &["signers", "list", "--json"],
    )));
    assert_eq!(list["version"], 1);
    let device = init["device"].as_str().unwrap().to_string();
    let mut expected = vec!["ci".to_string(), device.clone(), "recovery".to_string()];
    expected.sort();
    assert_eq!(signer_ids(&list), expected);
    let kinds: Vec<&str> = list["signers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"software"),
        "a software device key is labeled software: {list}"
    );

    // A second init is refused.
    failed(alien(
        &manager,
        &admin_home,
        work.path(),
        &[
            "signing",
            "init",
            "--ci-key-out",
            "/nonexistent/x",
            "--recovery-key-out",
            "/nonexistent/y",
        ],
    ));

    // Bob creates a device key and asks for approval.
    let request = json(&ok(alien(
        &manager,
        &bob_home,
        work.path(),
        &[
            "signing",
            "device",
            "--id",
            "bob-laptop",
            "--backend",
            "software",
            "--json",
        ],
    )));
    let code = request["code"].as_str().unwrap().to_string();
    let wrong = if code == "0000" { "1111" } else { "0000" };
    let refused = failed(alien(
        &manager,
        &admin_home,
        work.path(),
        &["signers", "approve", "bob-laptop", "--code", wrong, "--yes"],
    ));
    assert!(refused.contains("not the device you expect"), "{refused}");
    ok(alien(
        &manager,
        &admin_home,
        work.path(),
        &[
            "signers",
            "approve",
            "bob-laptop",
            "--code",
            &code,
            "--roles",
            "operate,release",
            "--yes",
        ],
    ));

    // Bob pins the fingerprint and sees himself on version 2.
    ok(alien(
        &manager,
        &bob_home,
        work.path(),
        &["signers", "trust", &fingerprint],
    ));
    let list = json(&ok(alien(
        &manager,
        &bob_home,
        work.path(),
        &["signers", "list", "--json"],
    )));
    assert_eq!(list["version"], 2);
    assert!(signer_ids(&list).contains(&"bob-laptop".to_string()));

    // Bob is not an admin, so he can't remove the CI key.
    let refused = failed(alien(
        &manager,
        &bob_home,
        work.path(),
        &["signers", "remove", "ci", "--yes"],
    ));
    assert!(
        refused.contains("does not hold the 'admin' role"),
        "{refused}"
    );

    // CI signs a release with its key.
    let stack = alien_core::Stack::new("app".to_string())
        .add(
            alien_core::Container::new("web".to_string())
                .code(alien_core::ContainerCode::Image {
                    image: format!("registry.example.com/app/web@sha256:{}", "a".repeat(64)),
                })
                .cpu(alien_core::ResourceSpec {
                    min: "0.5".into(),
                    desired: "1".into(),
                })
                .memory(alien_core::ResourceSpec {
                    min: "512Mi".into(),
                    desired: "1Gi".into(),
                })
                .port(8080)
                .permissions("execution".to_string())
                .build(),
            alien_core::ResourceLifecycle::Live,
        )
        .build();
    let project_dir = work.path().join("app");
    std::fs::create_dir_all(project_dir.join(".alien/build/test")).unwrap();
    std::fs::write(
        project_dir.join(".alien/build/test/stack.json"),
        serde_json::to_vec(&stack).unwrap(),
    )
    .unwrap();
    let ci_home = work.path().join("ci");
    let release = json(&ok(alien(
        &manager,
        &ci_home,
        &project_dir,
        &[
            "release",
            "--prebuilt",
            "--platforms",
            "test",
            "--no-git",
            "--json",
            "--sign",
            "--signing-key-file",
            ci_key.to_str().unwrap(),
            "--signing-fingerprint",
            &fingerprint,
        ],
    )));
    let release_id = release["releaseId"]
        .as_str()
        .or(release["id"].as_str())
        .unwrap()
        .to_string();
    let stored: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/v1/releases/{release_id}", manager.url))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let statement: SignedStatement =
        serde_json::from_value(stored["signedStacks"]["test"].clone()).unwrap();
    let anchor = Anchor {
        project: "default".to_string(),
        deployment: "dep_example".to_string(),
        protected: true,
        signing: fingerprint.clone(),
        components: SoftwareKey::generate()
            .unwrap()
            .public_key()
            .as_str()
            .to_string(),
        session_approval: Default::default(),
        inputs: None,
    };
    let verified = Trust::new(anchor.clone(), &stored_lists(&manager).await)
        .unwrap()
        .verify::<ReleaseBody>(&statement, Kind::Release, chrono::Utc::now())
        .expect("the stored release verifies from the fingerprint");
    assert_eq!(verified.signers.iter().collect::<Vec<_>>(), ["ci"]);
    assert_eq!(
        alien_core::release_statement::signed_stack(&verified.body, alien_core::Platform::Test)
            .unwrap(),
        stack
    );

    // Bob's device is removed; a release he signs afterwards no longer verifies.
    ok(alien(
        &manager,
        &admin_home,
        work.path(),
        &["signers", "remove", "bob-laptop", "--yes"],
    ));
    let lists = stored_lists(&manager).await;
    assert_eq!(lists.len(), 3);
    let refused = failed(alien(
        &manager,
        &bob_home,
        &project_dir,
        &[
            "release",
            "--prebuilt",
            "--platforms",
            "test",
            "--no-git",
            "--sign",
        ],
    ));
    assert!(
        refused.contains("not on signer list version 3"),
        "{refused}"
    );

    // A compromised manager stores a version 4 that adds its own key. It
    // doesn't verify, so every CLI refuses the list instead of trusting it.
    let attacker = SoftwareKey::generate().unwrap();
    let current = Trust::new(anchor, &lists).unwrap();
    let mut body: SignerListBody = current.chain().current().body.clone();
    body.previous = Some(current.chain().current().digest.clone());
    body.signers.push(Signer {
        id: "attacker".to_string(),
        key: attacker.public_key().as_str().to_string(),
        kind: SignerKind::Ci,
        roles: vec![Role::Release, Role::Admin],
        label: None,
    });
    let forged = SignedStatement::new(
        &Envelope {
            kind: Kind::SignerList.as_str().to_string(),
            project: Some("default".to_string()),
            deployment: None,
            sequence: 4,
            not_after: None,
            signer_list_version: 3,
            body,
        }
        .to_payload()
        .unwrap(),
    )
    .sign_with(&device, &attacker)
    .unwrap();
    let response = reqwest::Client::new()
        .post(format!("{}/v1/projects/default/signer-lists", manager.url))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "statement": forged }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        201,
        "the manager stores what it is given"
    );
    let refused = failed(alien(
        &manager,
        &admin_home,
        work.path(),
        &["signers", "list"],
    ));
    assert!(
        refused.contains("do not verify from the pinned fingerprint"),
        "{refused}"
    );

    // The manager keeps versions in order and never replaces one.
    let response = reqwest::Client::new()
        .post(format!("{}/v1/projects/default/signer-lists", manager.url))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "statement": lists[0].clone().sign_with("x", &attacker).unwrap() }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
}
