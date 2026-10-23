//! Real execution coverage for the AWS bundle's setup, environment and privilege boundary.
//!
//! Build a native `alien-sandbox-agent`, then run with Docker available:
//! `ALIEN_TEST_SANDBOX_AGENT_BINARY=/absolute/path/to/alien-sandbox-agent cargo test -p alien-build
//! --test sandbox_bundle_docker -- --ignored --nocapture`

use std::{path::Path, process::Command, time::Duration};

use alien_build::{
    sandbox_bundle::{write_bundle, AgentSource},
    sandbox_identity::BaseIdentity,
};

fn docker(args: &[&str]) -> String {
    let output = Command::new("docker")
        .env("DOCKER_BUILDKIT", "1")
        .args(args)
        .output()
        .expect("Docker CLI");
    assert!(
        output.status.success(),
        "docker {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// Test resources are removed even when a build or assertion panics.
#[derive(Default)]
struct Resources {
    images: Vec<String>,
    containers: Vec<String>,
}

impl Drop for Resources {
    fn drop(&mut self) {
        for name in &self.containers {
            let _ = Command::new("docker")
                .args(["stop", "-t", "1", name])
                .output();
            let _ = Command::new("docker").args(["rm", "-v", name]).output();
        }
        for image in self.images.iter().rev() {
            let _ = Command::new("docker").args(["rmi", image]).output();
        }
    }
}

#[tokio::test]
#[ignore = "requires Docker and ALIEN_TEST_SANDBOX_AGENT_BINARY pointing to a native agent"]
async fn bundles_run_as_the_image_user_and_keep_the_agent_protected() {
    let agent = std::env::var("ALIEN_TEST_SANDBOX_AGENT_BINARY").expect("native agent path");
    assert!(Path::new(&agent).is_file());
    let prefix = format!("alien1610-bundle-{:x}", rand::random::<u64>());
    let mut resources = Resources::default();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();

    for (variant, user, uid, gid) in [
        ("app", "app", 1001, 2001),
        ("root", "root", 60000, 60000),
        ("absent", "", 60000, 60000),
    ] {
        let base = format!("{prefix}-base:{variant}");
        let image = format!("{prefix}:{variant}");
        resources.images.extend([base.clone(), image.clone()]);
        let context = tempfile::tempdir().unwrap();
        // The final USER is the only variation: the image keeps the same /data ownership.
        let user_directive = if user.is_empty() {
            String::new()
        } else {
            format!("USER {user}\n")
        };
        std::fs::write(context.path().join("Dockerfile"), format!(
            "FROM python:3.13-slim\nRUN groupadd -g 2001 app && useradd -u 1001 -g 2001 app && mkdir /data && chown 1001:2001 /data\n{user_directive}"
        )).unwrap();
        docker(&["build", "-q", "-t", &base, context.path().to_str().unwrap()]);
        // Read the actual built fixture; only trusted test code is executed here.
        let identity = BaseIdentity {
            image_command: None,
            user: docker(&["image", "inspect", "--format", "{{.Config.User}}", &base]),
            passwd: Some(docker(&[
                "run",
                "--rm",
                "--entrypoint",
                "cat",
                &base,
                "/etc/passwd",
            ])),
            group: Some(docker(&[
                "run",
                "--rm",
                "--entrypoint",
                "cat",
                &base,
                "/etc/group",
            ])),
        };
        let bundle = context.path().join("bundle.zip");
        write_bundle(
            &bundle,
            &base,
            &AgentSource::Binary(agent.clone().into()),
            &identity,
        )
        .unwrap();
        zip::ZipArchive::new(std::fs::File::open(bundle).unwrap())
            .unwrap()
            .extract(context.path())
            .unwrap();
        docker(&[
            "build",
            "-q",
            "-t",
            &image,
            context.path().to_str().unwrap(),
        ]);
        let container = format!("{prefix}-{variant}");
        resources.containers.push(container.clone());
        docker(&[
            "run",
            "-d",
            "--name",
            &container,
            "-p",
            "127.0.0.1::8971",
            &image,
        ]);
        let address = docker(&["port", &container, "8971/tcp"]);
        let endpoint = format!("http://{address}");
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if client
                    .get(format!("{endpoint}/health"))
                    .send()
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("agent starts with the baked environment");
        assert_eq!(
            docker(&["exec", &container, "id", "-u"]),
            "0",
            "the supervisor starts as root"
        );
        let mut script = format!(
            "set -e; id; test $(id -u) = {uid}; test $(id -g) = {gid}; \
             test $ALIEN_SANDBOX_EXEC_UID = {uid}; test $ALIEN_SANDBOX_EXEC_GID = {gid}; \
             test $ALIEN_SANDBOX_ROOT = /sandbox; echo writable > /sandbox/probe; \
             test ! -w /usr/local/bin/alien-sandbox-agent; test ! -w /usr/local/bin; \
             test ! -w /etc/passwd; test ! -w /etc/group; \
             test $(stat -c %u /usr/local/bin/alien-sandbox-agent) = 0; \
             test $(stat -c %u /sandbox/probe) = {uid}; test $(stat -c %g /sandbox/probe) = {gid};"
        );
        if variant == "app" {
            script.push_str("echo success > /data/x; echo lock > /data/lock;");
        }
        let response = client
            .post(format!("{endpoint}/v1/exec"))
            .json(&serde_json::json!({"command":["/bin/sh", "-c", script],"timeoutMs":10000}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        let frames: Vec<serde_json::Value> = response
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let exit = frames.last().expect("terminal frame");
        assert_eq!(exit["t"], "exit", "{variant}: {response}");
        assert_eq!(exit["code"], 0, "{variant}: {response}");
        println!(
            "{variant}: uid={uid} gid={gid}, writable roots, protected agent/accounts: passed"
        );
    }
}
