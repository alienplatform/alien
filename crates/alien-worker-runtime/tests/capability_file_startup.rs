#![cfg(unix)]

use std::{os::unix::fs::PermissionsExt, process::Stdio, time::Duration};

use alien_client_core::runtime_credentials::RuntimeCapability;
use chrono::{TimeDelta, Utc};
use tokio::process::Command;

#[tokio::test]
async fn capability_file_configuration_starts_application_with_and_without_telemetry() {
    for (telemetry, valid) in [(false, true), (true, true), (false, false), (true, false)] {
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
            .env("ALIEN_IDENTITY_PROVIDER", "capabilityFile")
            .env("ALIEN_MANAGER_URL", "http://127.0.0.1:1")
            .env("ALIEN_MANAGER_AUDIENCE", "startup-test")
            .env("ALIEN_DEPLOYMENT_ID", "deployment-one")
            .env("ALIEN_TELEMETRY_CAPABILITY_FILE", &capability)
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
        if !valid {
            let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
                .await
                .expect("invalid identity should fail startup promptly")
                .unwrap();
            assert!(!status.success());
            assert!(
                !marker.exists(),
                "a legacy credential cannot start an identity-mode workload"
            );
            continue;
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "runtime exited before application startup (telemetry={telemetry})"
                );
                if marker.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("application should start with the shared identity provider configuration");
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "started");
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
}
