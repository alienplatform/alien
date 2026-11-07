//! Signing from first release to recovery, with real processes: `alien serve`
//! on SQLite as the manager, the `alien` binary in a terminal, and device keys
//! in a TPM (the swtpm simulator in CI).
//!
//! Alice's first release turns signing on. A second "first release" from
//! another clone is refused with who turned it on. Bob asks to be trusted; a
//! wrong pairing code is refused and the right one approved. CI is added and
//! signs a release. Bob is removed and can't sign. Alice's laptop is lost and
//! Carol restores signing with the recovery code.
//!
//! Needs `swtpm`, `tpm2-tools` and `script`. Run with
//! `cargo test -p alien-cli --test signing_flow -- --ignored`.

#![cfg(target_os = "linux")]

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alien_signing::{
    InstallId, Ledger, ReleaseBody, SecretKey, SignedStatement, SignerChain, SigningSettings, Trust,
};
use base64::Engine as _;
use sha2::Digest as _;
use tempfile::TempDir;

const TOKEN: &str = "ax_admin_signingflowtesttoken0000000000000000";
const PIN: &str = "4321";
const PROJECT: &str = "default";

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A TPM simulator, unless the environment already names one.
struct Tpm {
    tcti: String,
    _process: Option<Process>,
    _state: TempDir,
}

fn start_tpm() -> Tpm {
    let state = TempDir::new().unwrap();
    if let Ok(tcti) = std::env::var("TPM2TOOLS_TCTI") {
        return Tpm {
            tcti,
            _process: None,
            _state: state,
        };
    }
    let port = free_port();
    let child = Command::new("swtpm")
        .args(["socket", "--tpm2", "--tpmstate"])
        .arg(format!("dir={}", state.path().display()))
        .args(["--server", &format!("type=tcp,port={port}")])
        // The swtpm TCTI reaches the control channel on the next port.
        .args(["--ctrl", &format!("type=tcp,port={}", port + 1)])
        .args(["--flags", "not-need-init,startup-clear"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("swtpm is installed");
    let tcti = format!("swtpm:port={port}");
    let started = Instant::now();
    while !Command::new("tpm2_getrandom")
        .args(["4", "--hex"])
        .env("TPM2TOOLS_TCTI", &tcti)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
    {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "swtpm did not start"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    Tpm {
        tcti,
        _process: Some(Process(child)),
        _state: state,
    }
}

struct Manager {
    url: String,
    _process: Process,
    _state: TempDir,
}

async fn start_manager() -> Manager {
    let state = TempDir::new().unwrap();
    let port = free_port();
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
    let process = Process(child);
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
        url,
        _process: process,
        _state: state,
    }
}

/// One engineer's machine: their own signing home, user name and git email.
struct Person {
    name: &'static str,
    home: PathBuf,
}

struct World {
    manager: Manager,
    tpm: Tpm,
    work: TempDir,
    /// A `gh` that records what it is asked to store instead of calling GitHub.
    gh_dir: PathBuf,
}

impl World {
    fn person(&self, name: &'static str) -> Person {
        let home = self.work.path().join(name);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("gitconfig"),
            format!("[user]\n\temail = {name}@example.com\n"),
        )
        .unwrap();
        Person { name, home }
    }

    /// A checkout of the app: a prebuilt stack and, optionally, the pin.
    fn checkout(&self, name: &str) -> PathBuf {
        let dir = self.work.path().join(name);
        std::fs::create_dir_all(dir.join(".alien/build/test")).unwrap();
        std::fs::write(dir.join(".gitignore"), ".alien\n").unwrap();
        std::fs::write(
            dir.join(".alien/build/test/stack.json"),
            serde_json::to_vec(&stack()).unwrap(),
        )
        .unwrap();
        dir
    }

    fn command(&self, person: &Person, cwd: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
        command
            .args(args)
            .current_dir(cwd)
            .env("ALIEN_MANAGER_URL", &self.manager.url)
            .env("ALIEN_API_KEY", TOKEN)
            .env("ALIEN_SIGNING_HOME", person.home.join("signing"))
            .env("ALIEN_DEVICE_KEY_PIN", PIN)
            .env("TPM2TOOLS_TCTI", &self.tpm.tcti)
            .env("USER", person.name)
            .env("GIT_CONFIG_GLOBAL", person.home.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.gh_dir.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env_remove("ALIEN_SIGNING_KEY")
            .env("NO_COLOR", "1");
        command
    }

    /// Run `alien` without a terminal, as CI does.
    fn run(&self, person: &Person, cwd: &Path, args: &[&str]) -> Output {
        self.command(person, cwd, args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// Run `alien` with the CI key in its environment.
    fn run_ci(&self, cwd: &Path, key: &str, args: &[&str]) -> Output {
        let ci = self.person("ci");
        self.command(&ci, cwd, args)
            .env("ALIEN_SIGNING_KEY", key)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// Run `alien` in a terminal (`script` gives it one). For each
    /// `(prompt, answer)`, wait for the prompt, then type the answer.
    fn run_in_terminal(
        &self,
        person: &Person,
        cwd: &Path,
        args: &[&str],
        answers: &[(&str, &str)],
    ) -> Output {
        let command = self.command(person, cwd, args);
        let line = std::iter::once(command.get_program().to_string_lossy().to_string())
            .chain(
                command
                    .get_args()
                    .map(|arg| arg.to_string_lossy().to_string()),
            )
            .map(|part| shell_quote(&part))
            .collect::<Vec<_>>()
            .join(" ");
        let mut script = Command::new("script");
        script.args(["-q", "-e", "-c", &line, "/dev/null"]);
        script.current_dir(cwd);
        for (key, value) in command.get_envs() {
            match value {
                Some(value) => script.env(key, value),
                None => script.env_remove(key),
            };
        }
        let mut child = script
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut stdout = child.stdout.take().unwrap();
        let reader = {
            let seen = seen.clone();
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                loop {
                    match stdout.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => seen.lock().unwrap().extend_from_slice(&buffer[..read]),
                    }
                }
            })
        };
        let mut stdin = child.stdin.take().unwrap();
        let mut from = 0;
        for (prompt, answer) in answers {
            let started = Instant::now();
            loop {
                let found = {
                    let seen = seen.lock().unwrap();
                    String::from_utf8_lossy(&seen[from..])
                        .find(prompt)
                        .map(|at| from + at + prompt.len())
                };
                if let Some(end) = found {
                    from = end;
                    break;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(120),
                    "no prompt '{prompt}' in:\n{}",
                    String::from_utf8_lossy(&seen.lock().unwrap())
                );
                std::thread::sleep(Duration::from_millis(50));
            }
            // The CLI prints a prompt just before it turns echo off for a
            // hidden one, and turning echo off drops what was typed ahead.
            std::thread::sleep(Duration::from_millis(500));
            stdin.write_all(answer.as_bytes()).unwrap();
            stdin.flush().unwrap();
        }
        let status = child.wait().unwrap();
        drop(stdin);
        reader.join().unwrap();
        let mut stderr = Vec::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_end(&mut stderr)
            .unwrap();
        let stdout = seen.lock().unwrap().clone();
        Output {
            status,
            stdout,
            stderr,
        }
    }
}

/// Whether `text` is still on the screen after `output` is drawn: plain
/// lines, colors ignored, and the cursor moves and line erases that live
/// progress uses.
fn on_screen(output: &str, text: &str) -> bool {
    let mut lines = vec![String::new()];
    let mut row = 0;
    let mut chars = output.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => {
                row += 1;
                if row == lines.len() {
                    lines.push(String::new());
                }
            }
            '\u{1b}' if chars.peek() == Some(&'[') => {
                chars.next();
                let mut code = String::new();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        code.push(next);
                        break;
                    }
                    code.push(next);
                }
                match code.as_str() {
                    "1A" => row = row.saturating_sub(1),
                    "1B" => {
                        row += 1;
                        if row == lines.len() {
                            lines.push(String::new());
                        }
                    }
                    "2K" => lines[row].clear(),
                    _ => {}
                }
            }
            c => lines[row].push(c),
        }
    }
    lines.iter().any(|line| line.contains(text))
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .replace('\r', "")
}

fn ok(output: Output) -> String {
    assert!(output.status.success(), "alien failed:\n{}", text(&output));
    text(&output)
}

fn failed(output: Output) -> String {
    assert!(
        !output.status.success(),
        "alien succeeded but should have failed:\n{}",
        text(&output)
    );
    text(&output)
}

/// The line after the one containing `marker`, trimmed.
fn line_after(output: &str, marker: &str) -> String {
    let mut lines = output.lines();
    lines
        .by_ref()
        .find(|line| line.contains(marker))
        .unwrap_or_else(|| panic!("no '{marker}' in:\n{output}"));
    lines.next().unwrap().trim().to_string()
}

fn stack() -> alien_core::Stack {
    alien_core::Stack::new("app".to_string())
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
        .build()
}

/// This machine's short host name, as the CLI names devices `<user>-<host>`.
fn host() -> String {
    let name = std::fs::read_to_string("/etc/hostname")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap();
    name.trim()
        .split('.')
        .next()
        .unwrap()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn device(user: &str) -> String {
    format!("{user}-{}", host())
}

const RELEASE: &[&str] = &["release", "--prebuilt", "--platforms", "test", "--no-git"];

async fn get(manager: &Manager, path: &str) -> serde_json::Value {
    reqwest::Client::new()
        .get(format!("{}{path}", manager.url))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn lists(manager: &Manager) -> Vec<SignedStatement> {
    serde_json::from_value(
        get(manager, &format!("/v1/projects/{PROJECT}/signer-lists")).await["items"].clone(),
    )
    .unwrap()
}

async fn chain(manager: &Manager, fingerprint: &str) -> SignerChain {
    SignerChain::from_statements(fingerprint, &lists(manager).await, chrono::Utc::now())
        .expect("the stored lists verify from the fingerprint")
}

/// The newest release's statement, checked as a protected install on the
/// `production` channel checks it. `None` when the release is unsigned.
async fn newest_release(manager: &Manager, fingerprint: &str) -> Option<String> {
    let releases = get(manager, "/v1/releases").await;
    let newest = releases["items"]
        .as_array()
        .unwrap()
        .iter()
        .max_by_key(|release| release["createdAt"].as_str().unwrap().to_string())
        .unwrap()
        .clone();
    let statement = newest["signedStacks"]["test"].clone();
    if statement.is_null() {
        return None;
    }
    let statement: SignedStatement = serde_json::from_value(statement).unwrap();
    let prefix = "acme-prod";
    let trust = Trust::new(
        SigningSettings {
            fingerprint: fingerprint.to_string(),
            prefix: prefix.to_string(),
            channel: "production".to_string(),
        },
        InstallId::parse(&format!("aws:111122223333:us-east-1:{prefix}")).unwrap(),
        &lists(manager).await,
        &Ledger::new(prefix),
        chrono::Utc::now(),
    )
    .unwrap();
    let verified: alien_signing::Verified<ReleaseBody> = trust
        .verify_release(&statement, chrono::Utc::now())
        .expect("the release verifies as a protected install checks it");
    assert_eq!(
        alien_core::release_statement::signed_stack(&verified.body, alien_core::Platform::Test)
            .unwrap(),
        stack()
    );
    Some(verified.signers.into_iter().collect::<Vec<_>>().join(","))
}

/// A `gh` standing in for GitHub: a tag ruleset freezes `v*` tags, and
/// environments and branch policies answer as
/// a correctly set up `release` environment does, and `secret set` writes
/// the secret to a file.
fn fake_gh(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let secret = dir.join("secret.pem");
    let script = format!(
        r#"#!/bin/sh
case "$*" in
  "secret set"*) cat > '{secret}' ;;
  *"/rulesets/1") echo '{{"id":1,"name":"Release tags","target":"tag","enforcement":"active","conditions":{{"ref_name":{{"include":["refs/tags/v*"],"exclude":[]}}}},"rules":[{{"type":"creation"}},{{"type":"update"}},{{"type":"deletion"}}],"bypass_actors":[]}}' ;;
  *"/rulesets") echo '[{{"id":1,"target":"tag"}}]' ;;
  *"deployment-branch-policies"*"-X POST"*|*"-X POST"*"deployment-branch-policies"*) echo '{{}}' ;;
  *"deployment-branch-policies"*) echo '{{"branch_policies":[{{"name":"main","type":"branch"}},{{"name":"v*","type":"tag"}}]}}' ;;
  *"-X PUT"*) cat > /dev/null; echo '{{}}' ;;
  *"environments/release"*) echo '{{"deployment_branch_policy":{{"protected_branches":false,"custom_branch_policies":true}}}}' ;;
  *) echo "unexpected gh $*" >&2; exit 1 ;;
esac
"#,
        secret = secret.display()
    );
    let path = bin.join("gh");
    std::fs::write(&path, script).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    bin
}

#[tokio::test]
#[ignore = "needs swtpm, tpm2-tools and script"]
async fn signing_from_first_release_to_recovery() {
    let work = TempDir::new().unwrap();
    let gh_dir = fake_gh(work.path());
    let world = World {
        manager: start_manager().await,
        tpm: start_tpm(),
        gh_dir,
        work,
    };
    let alice = world.person("alice");
    let app = world.checkout("app");

    // CI with no .alien/signing.json releases unsigned and never creates a list.
    let output = ok(world.run(&alice, &app, RELEASE));
    assert!(
        output.contains("not signed: this repo has no .alien/signing.json"),
        "{output}"
    );
    assert!(lists(&world.manager).await.is_empty());

    // Alice closes the terminal input at the recovery code: nothing is
    // published or written, since the code was never confirmed saved.
    let output = failed(world.run_in_terminal(
        &alice,
        &app,
        RELEASE,
        &[("Press Enter once saved", "\u{4}")],
    ));
    assert!(
        output.contains("The recovery code wasn't confirmed"),
        "{output}"
    );
    assert!(lists(&world.manager).await.is_empty());
    assert!(!app.join(".alien/signing.json").exists());

    // Alice's first release in a terminal turns signing on.
    let output =
        ok(world.run_in_terminal(&alice, &app, RELEASE, &[("Press Enter once saved", "\n")]));
    assert!(
        output.contains("Signing is now on for default. This machine is its first trusted device."),
        "{output}"
    );
    let recovery_code = line_after(&output, "Recovery code (shown once");
    // Progress redraws by erasing its last lines: the code must still be on
    // the screen when the release is done.
    assert!(on_screen(&output, &recovery_code), "{output:?}");
    assert!(
        output.contains("Wrote .alien/signing.json — commit it."),
        "{output}"
    );
    assert!(
        output.contains("Setup as the control plane hosts it · registers through stack outputs · managed by any managing role"),
        "{output}"
    );
    assert!(
        output.contains("Releasing to production · 0 protected deployments will run it"),
        "{output}"
    );
    assert!(output.contains("TPM PIN to sign ▸"), "{output}");
    let pin: serde_json::Value =
        serde_json::from_slice(&std::fs::read(app.join(".alien/signing.json")).unwrap()).unwrap();
    let fingerprint = pin["fingerprint"].as_str().unwrap().to_string();
    assert_eq!(pin["project"], PROJECT);
    assert_eq!(
        std::fs::read_to_string(app.join(".gitignore")).unwrap(),
        ".alien/*\n!.alien/signing.json\n"
    );
    let v1 = chain(&world.manager, &fingerprint).await;
    assert_eq!(v1.current().version, 1);
    let alice_id = device("alice");
    let alice_id = alice_id.as_str();
    let bob_id = device("bob");
    let bob_id = bob_id.as_str();
    let devices = ok(world.run(&alice, &app, &["devices", "list"]));
    assert!(
        devices.contains(&format!(
            "This device  {alice_id} · TPM simulator (test build)"
        )),
        "{devices}"
    );
    assert!(
        output.contains(&format!("· signed by {alice_id} · production")),
        "{output}"
    );
    assert_eq!(
        newest_release(&world.manager, &fingerprint)
            .await
            .as_deref(),
        Some(alice_id)
    );

    // A second first release, from a clone without the pin, loses the race.
    let dave = world.person("dave");
    let other = world.checkout("other-clone");
    let output = ok(world.run_in_terminal(&dave, &other, RELEASE, &[]));
    assert!(
        output.contains("signing was already turned on for default by alice@example.com — pull .alien/signing.json"),
        "{output}"
    );
    assert!(output.contains("pull .alien/signing.json"), "{output}");
    assert!(!output.contains("Recovery code"), "{output}");
    assert!(!other.join(".alien/signing.json").exists());
    assert_eq!(lists(&world.manager).await.len(), 1);
    assert_eq!(newest_release(&world.manager, &fingerprint).await, None);

    // Bob, a teammate, asks to be trusted; his release goes out unsigned.
    let bob = world.person("bob");
    let output = ok(world.run_in_terminal(&bob, &app, RELEASE, &[]));
    assert!(
        output.contains("! Not signed: this device isn't trusted for default yet."),
        "{output}"
    );
    assert!(
        output.contains(&format!(
            "Asked admins (alice@example.com) to trust {bob_id}."
        )),
        "{output}"
    );
    let pairing_code = line_after(&output, "Send them this code over Slack or in person:");
    assert_eq!(pairing_code.len(), 19, "{pairing_code}");
    assert!(on_screen(&output, &pairing_code), "{output:?}");
    assert_eq!(newest_release(&world.manager, &fingerprint).await, None);

    // Alice types the wrong code: refused, nothing signed.
    let output = failed(world.run_in_terminal(
        &alice,
        &app,
        &["approve"],
        &[("Code from Bob ▸", "0000-0000-0000-0000\n")],
    ));
    assert!(
        output.contains(&format!("{bob_id} (bob@example.com) asks to: release")),
        "{output}"
    );
    assert!(output.contains("That isn't the pairing code"), "{output}");
    assert_eq!(lists(&world.manager).await.len(), 1);

    // The right code: version 2 trusts Bob, and the request is resolved.
    let output = ok(world.run_in_terminal(
        &alice,
        &app,
        &["approve"],
        &[("Code from Bob ▸", &format!("{pairing_code}\n"))],
    ));
    assert!(output.contains("TPM PIN to approve ▸"), "{output}");
    assert!(
        output.contains(&format!(
            "✓ {bob_id} can release · delivered to protected deployments as the next device list"
        )),
        "{output}"
    );
    assert_eq!(
        chain(&world.manager, &fingerprint).await.current().version,
        2
    );
    let pending = get(
        &world.manager,
        &format!("/v1/projects/{PROJECT}/pending-changes?status=pending"),
    )
    .await;
    assert!(pending["items"].as_array().unwrap().is_empty(), "{pending}");

    // Erin asks to be trusted. The control plane drops her request and files
    // its own for her key with the admin role: her CLI refuses to show a
    // pairing code for it, so no admin is asked to type one.
    let erin = world.person("erin");
    let output = ok(world.run_in_terminal(&erin, &app, RELEASE, &[]));
    assert!(output.contains("Send them this code"), "{output}");
    let pending = get(
        &world.manager,
        &format!("/v1/projects/{PROJECT}/pending-changes?status=pending"),
    )
    .await;
    let honest = pending["items"][0].clone();
    assert_eq!(pending["items"].as_array().unwrap().len(), 1, "{pending}");
    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "{}/v1/pending-changes/{}/resolution",
            world.manager.url,
            honest["id"].as_str().unwrap()
        ))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "status": "rejected" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut crafted = honest["body"].clone();
    let signers = crafted["signers"].as_array_mut().unwrap();
    signers.last_mut().unwrap()["roles"] = serde_json::json!(["admin", "release"]);
    let response = client
        .post(format!(
            "{}/v1/projects/{PROJECT}/pending-changes",
            world.manager.url
        ))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({
            "summary": "erin asks to: release",
            "basedOnVersion": honest["basedOnVersion"],
            "body": crafted,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        201,
        "the manager stores what it is given"
    );
    let crafted: serde_json::Value = response.json().await.unwrap();
    let output = failed(world.run_in_terminal(&erin, &app, RELEASE, &[]));
    assert!(
        output.contains(&format!(
            "Waiting change {} adds this device but isn't the request this device makes",
            crafted["id"].as_str().unwrap()
        )),
        "{output}"
    );
    assert!(!output.contains("Send them this code"), "{output}");
    let response = client
        .post(format!(
            "{}/v1/pending-changes/{}/resolution",
            world.manager.url,
            crafted["id"].as_str().unwrap()
        ))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "status": "rejected" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // Bob's next release is signed by him.
    ok(world.run_in_terminal(&bob, &app, RELEASE, &[]));
    assert_eq!(
        newest_release(&world.manager, &fingerprint)
            .await
            .as_deref(),
        Some(bob_id)
    );

    // Alice connects CI; the key lands in the `release` environment only.
    let output = ok(world.run_in_terminal(&alice, &app, &["devices", "add-ci", "acme/app"], &[]));
    assert!(
        output.contains("GitHub Actions (acme/app) can release · its key is in the `release` environment, which only main and v* tags can use"),
        "{output}"
    );
    let ci_key = std::fs::read_to_string(world.work.path().join("secret.pem")).unwrap();
    SecretKey::from_pkcs8_pem(&ci_key).expect("the stored secret is the CI key");
    let output = ok(world.run_ci(&app, &ci_key, RELEASE));
    assert!(!output.contains("not signed"), "{output}");
    assert_eq!(
        newest_release(&world.manager, &fingerprint)
            .await
            .as_deref(),
        Some("github-acme-app")
    );

    // CI is not an admin: it can't approve or change the devices.
    let output = failed(world.run_ci(&app, &ci_key, &["devices", "remove", bob_id]));
    assert!(output.contains("This device"), "{output}");

    // Bob is removed; his device can no longer sign.
    let output = ok(world.run_in_terminal(&alice, &app, &["devices", "remove", bob_id], &[]));
    assert!(
        output.contains(&format!("{bob_id} can no longer sign")),
        "{output}"
    );
    let output = ok(world.run(&bob, &app, RELEASE));
    assert!(
        output.contains("not signed: this device isn't trusted for default yet"),
        "{output}"
    );
    assert_eq!(newest_release(&world.manager, &fingerprint).await, None);

    // Alice's laptop is lost. Carol restores signing with the recovery code.
    let carol = world.person("carol");
    let output = ok(world.run_in_terminal(
        &carol,
        &app,
        &["devices", "recover"],
        &[("Recovery code for default:", &format!("{recovery_code}\n"))],
    ));
    assert!(output.contains("Signing restored for default"), "{output}");
    assert!(
        output.contains(&format!("No longer trusted: {alice_id}")),
        "{output}"
    );
    let restored = chain(&world.manager, &fingerprint).await;
    assert!(restored.current().body.recovered);
    assert!(restored.current().signer(alice_id).is_none());
    let output = ok(world.run(&carol, &app, &["devices", "list"]));
    assert!(
        output.contains("restored with the recovery code"),
        "{output}"
    );
    ok(world.run_in_terminal(&carol, &app, RELEASE, &[]));
    assert_eq!(
        newest_release(&world.manager, &fingerprint)
            .await
            .as_deref(),
        Some(device("carol").as_str())
    );
    let output = failed(world.run_in_terminal(&alice, &app, &["approve"], &[]));
    assert!(output.contains("isn't an admin"), "{output}");

    // A manager that serves a list of its own making is refused by every CLI.
    let attacker = SecretKey::generate().unwrap();
    let mut body = restored.current().body.clone();
    body.previous = Some(restored.current().digest.clone());
    body.recovered = false;
    body.signers.push(alien_signing::Signer {
        id: "attacker".to_string(),
        key: attacker.public_key().as_str().to_string(),
        kind: alien_signing::SignerKind::Ci,
        roles: vec![alien_signing::Role::Release, alien_signing::Role::Admin],
        label: None,
    });
    let forged = SignedStatement::new(
        &alien_signing::Envelope {
            kind: alien_signing::Kind::SignerList.as_str().to_string(),
            project: PROJECT.to_string(),
            install: None,
            sequence: restored.current().version + 1,
            not_after: Some(chrono::Utc::now() + chrono::Duration::days(30)),
            signer_list_version: restored.current().version,
            body,
        }
        .to_payload()
        .unwrap(),
    )
    .sign_with("attacker", &attacker)
    .unwrap();
    let response = reqwest::Client::new()
        .post(format!(
            "{}/v1/projects/{PROJECT}/signer-lists",
            world.manager.url
        ))
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
    let output = ok(world.run(&carol, &app, RELEASE));
    assert!(output.contains("do not verify from"), "{output}");
    assert_eq!(newest_release(&world.manager, &fingerprint).await, None);
}

/// A storage-only stack: its AWS setup renders without images.
fn storage_stack() -> alien_core::Stack {
    serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {
            "assets": {
                "config": {
                    "corsAllowedOrigins": [], "id": "assets", "lifecycleRules": [],
                    "publicRead": false, "type": "storage", "versioning": false
                },
                "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
            }
        },
        "permissions": { "profiles": { "execution": {} }, "management": "auto" }
    }))
    .unwrap()
}

async fn post_release(manager: &Manager, body: &serde_json::Value) -> (u16, serde_json::Value) {
    let response = reqwest::Client::new()
        .post(format!("{}/v1/releases", manager.url))
        .bearer_auth(TOKEN)
        .json(body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap())
}

/// The setup templates a signed release uploads are stored and served
/// exactly as signed: the manager hands back bytes that hash to the
/// statement's digests, and refuses bytes that don't or that are missing.
/// Source builds carry no verifier or publisher pins, so the release's
/// renderer runs here with test pins instead of through `alien release`.
#[tokio::test]
async fn setup_templates_are_stored_as_signed() {
    let manager = start_manager().await;
    let stack = storage_stack();
    let signing = alien_core::StackSigning {
        fingerprint: format!("SHA256:{}", "9f".repeat(32)),
        channel: "production".to_string(),
    };
    let hosting: alien_cli_common::setup_templates::SetupHostingOptions =
        serde_json::from_value(serde_json::json!({
            "registration": { "mode": "outputsFallback" },
            "managementTrust": {
                "requireExternalId": true,
                "allowedManagingRoleArns": ["arn:aws:iam::999988887777:role/manager"]
            }
        }))
        .unwrap();
    let pins = alien_core::SetupPins {
        aws_publisher: Some("111122223333".to_string()),
        aws_verifier_image: Some(format!("verifier@sha256:{}", "5d".repeat(32))),
        gcp_verifier_image: Some(format!(
            "us-docker.pkg.dev/publisher/verifier/verifier@sha256:{}",
            "5d".repeat(32)
        )),
        azure_verifier_image: Some(format!(
            "publisher.azurecr.io/verifier@sha256:{}",
            "5d".repeat(32)
        )),
        operator_digest: None,
    };
    let rendered = alien_cli::signing::release::setup_templates(
        &stack,
        alien_core::Platform::Aws,
        &signing,
        &hosting,
        pins,
    )
    .await
    .unwrap();
    assert_eq!(
        rendered.keys().map(String::as_str).collect::<Vec<_>>(),
        ["cloudformation", "terraform-aws"]
    );
    let body = alien_cli::signing::release::body(
        alien_core::Platform::Aws,
        &stack,
        "production",
        &Default::default(),
        alien_cli::signing::release::signed_setup(&rendered),
    )
    .unwrap();
    let envelope = alien_signing::Envelope {
        kind: alien_signing::Kind::Release.as_str().to_string(),
        project: PROJECT.to_string(),
        install: None,
        sequence: 1,
        not_after: None,
        signer_list_version: 1,
        body,
    };
    let key = SecretKey::from_secret_bytes(&[7; 32]).unwrap();
    let statement = SignedStatement::new(&envelope.to_payload().unwrap())
        .sign_with("ci", &key)
        .unwrap();
    let upload = alien_cli::signing::release::setup_upload(&rendered);
    let request = |templates: &serde_json::Value| {
        serde_json::json!({
            "projectId": PROJECT,
            "stack": { "aws": stack },
            "signedStacks": { "aws": statement },
            "setupTemplates": { "aws": templates },
        })
    };

    // Other bytes than the signed ones are refused with the reason.
    let mut tampered = rendered["cloudformation"].bytes.clone();
    tampered.extend_from_slice(b"\n# extra\n");
    let mut changed = upload.clone();
    changed.insert(
        "cloudformation".to_string(),
        base64::engine::general_purpose::STANDARD.encode(&tampered),
    );
    let (status, error) = post_release(&manager, &request(&serde_json::json!(changed))).await;
    assert_eq!(status, 400, "{error}");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("The aws setup template 'cloudformation' hashes to sha256:")
            && message.contains("but the release signed sha256:"),
        "{message}"
    );
    // A signed template without its bytes is refused.
    let mut missing = upload.clone();
    missing.remove("terraform-aws");
    let (status, error) = post_release(&manager, &request(&serde_json::json!(missing))).await;
    assert_eq!(status, 400, "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("names the setup template 'terraform-aws', but its bytes weren't sent"),
        "{error}"
    );
    assert!(get(&manager, "/v1/releases").await["items"]
        .as_array()
        .unwrap()
        .is_empty());

    // The signed bytes are stored, and served back hashing to the signed digests.
    let (status, created) = post_release(&manager, &request(&serde_json::json!(upload))).await;
    assert_eq!(status, 201, "{created}");
    let release = get(
        &manager,
        &format!("/v1/releases/{}", created["id"].as_str().unwrap()),
    )
    .await;
    let served: SignedStatement =
        serde_json::from_value(release["signedStacks"]["aws"].clone()).unwrap();
    let signed: ReleaseBody = served.open().unwrap().body().unwrap();
    assert_eq!(signed.setup.len(), 2);
    for (name, template) in &signed.setup {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(release["setupTemplates"]["aws"][name].as_str().unwrap())
            .unwrap();
        assert_eq!(
            format!("sha256:{}", hex::encode(sha2::Sha256::digest(&bytes))),
            template.sha256,
            "{name}"
        );
        assert_eq!(bytes, rendered[name].bytes, "{name}");
    }
}
