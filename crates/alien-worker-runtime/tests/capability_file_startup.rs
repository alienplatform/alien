#![cfg(unix)]

use std::{os::unix::fs::PermissionsExt, process::Stdio, time::Duration};

use alien_client_core::runtime_credentials::RuntimeCapability;
use chrono::{TimeDelta, Utc};
use tokio::process::Command;

/// Lambda, Cloud Run and Kubernetes workers must start while the manager is
/// unreachable or their telemetry credential is unusable. Log export then
/// waits for a credential; the application does not.
#[tokio::test]
async fn application_starts_without_a_usable_telemetry_credential() {
    for (provider, telemetry, valid) in [
        // The cloud proof is not ready yet and the manager is down.
        ("session", false, true),
        ("session", true, true),
        ("capabilityFile", false, true),
        ("capabilityFile", true, true),
        // The operator wrote a bad capability file.
        ("capabilityFile", false, false),
        ("capabilityFile", true, false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let capability = directory.path().join("telemetry.json");
        std::fs::write(
            &capability,
            serde_json::to_vec(&RuntimeCapability {
                token: if valid {
                    "ax_cap_v1_startup-fixture"
                } else {
                    "legacy-deployment-token"
                }
                .into(),
                expires_at: Utc::now() + TimeDelta::minutes(5),
            })
            .unwrap(),
        )
        .unwrap();
        std::fs::set_permissions(&capability, std::fs::Permissions::from_mode(0o600)).unwrap();
        let marker = directory.path().join("application-started");
        let mut command = Command::new(env!("CARGO_BIN_EXE_alien-worker-runtime"));
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("ALIEN_DEPLOYMENT_TYPE", "local")
            .env("ALIEN_IDENTITY_PROVIDER", provider)
            .env("ALIEN_MANAGER_URL", "http://127.0.0.1:1")
            .env("ALIEN_MANAGER_AUDIENCE", "startup-test")
            .env("ALIEN_DEPLOYMENT_ID", "deployment-one")
            .env("ALIEN_TELEMETRY_CAPABILITY_FILE", &capability)
            .env(
                "ALIEN_SESSION_FILE",
                directory.path().join("not-yet-written"),
            )
            .env("STARTUP_MARKER", &marker)
            .args([
                "--worker-grpc-address",
                "127.0.0.1:0",
                "--transport",
                "local",
                "--",
                "/bin/sh",
                "-c",
                "printf started > \"$STARTUP_MARKER\"",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if telemetry {
            command.env(
                "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
                "http://127.0.0.1:1/v1/logs",
            );
        }
        let mut child = command.spawn().unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "runtime exited before application startup \
                     (provider={provider}, telemetry={telemetry}, valid={valid})"
                );
                if std::fs::read_to_string(&marker).is_ok_and(|text| text == "started") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("application should start without the manager");
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
}
