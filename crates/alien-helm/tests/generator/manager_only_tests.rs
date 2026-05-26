//! Manager-only scenarios — stacks with no `Frozen` infrastructure
//! resources. Charts still render `examples/<target>.yaml` files for
//! IRSA / Workload Identity / Federated Identity.

use super::helpers::{assert_helm_valid, render, snapshot_chart};
use alien_core::{
    Container, ContainerCode, Daemon, DaemonCode, Ingress, PermissionProfile, ResourceLifecycle,
    ResourceSpec, Stack, StackSettings, ToolchainConfig, Worker, WorkerCode,
};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};

#[test]
fn pure_worker_chart_emits_service_for_public_ingress() {
    let worker = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "registry.example.com/api:1".to_string(),
        })
        .permissions("runtime".to_string())
        .ingress(Ingress::Public)
        .build();
    let stack = Stack::new("pure-fn".to_string())
        .permission(
            "runtime",
            PermissionProfile::new().global(["worker/management"]),
        )
        .add(worker, ResourceLifecycle::Live)
        .build();
    let chart = render(&stack, StackSettings::default());
    snapshot_chart("manager_only_pure_worker", &chart);
    assert_helm_valid(&chart, "manager_only_pure_worker");
}

#[test]
fn manager_chart_uses_explicit_secrets_and_restricted_defaults() {
    let stack = Stack::new("manager-only".to_string()).build();
    let chart = render(&stack, StackSettings::default());

    let values = chart.files.get("values.yaml").expect("values.yaml");
    assert!(values.contains("existingSecret:"));
    assert!(values.contains("readOnlyRootFilesystem: true"));
    assert!(values.contains("allowPrivilegeEscalation: false"));
    assert!(values.contains("automountServiceAccountToken: true"));
    assert!(values.contains("persistence:"));
    assert!(values.contains("networkPolicy:"));
    assert!(values.contains("pdb:"));

    let secret = chart.files.get("templates/secret.yaml").expect("secret");
    assert!(!secret.contains("randAlphaNum"));
    assert!(secret.contains(".Values.management.existingSecret.name"));
    assert!(secret.contains(".Values.runtime.encryption.existingSecret.name"));

    let deployment = chart
        .files
        .get("templates/deployment.yaml")
        .expect("deployment");
    assert!(deployment.contains("securityContext:"));
    assert!(deployment.contains("mountPath: /tmp"));
    assert!(deployment.contains("mountPath: {{ .Values.runtime.data.mountPath | quote }}"));
}

#[test]
fn runtime_service_targets_manager_port_name() {
    let stack = Stack::new("manager-api".to_string()).build();
    let chart = render(&stack, StackSettings::default());
    let service = chart.files.get("templates/service.yaml").expect("service");

    assert!(service.contains("targetPort: otlp"));
    assert!(!service.contains("targetPort: http"));
}

#[test]
fn chart_generation_fails_when_source_workloads_remain() {
    let registry = HelmRegistry::built_in();
    let worker = Worker::new("source-worker".to_string())
        .code(WorkerCode::Source {
            src: "worker".to_string(),
            toolchain: ToolchainConfig::TypeScript { binary_name: None },
        })
        .permissions("runtime".to_string())
        .build();
    let container = Container::new("source-container".to_string())
        .code(ContainerCode::Source {
            src: "container".to_string(),
            toolchain: ToolchainConfig::Docker {
                dockerfile: None,
                build_args: None,
                target: None,
            },
        })
        .cpu(ResourceSpec {
            min: "100m".to_string(),
            desired: "500m".to_string(),
        })
        .memory(ResourceSpec {
            min: "128Mi".to_string(),
            desired: "512Mi".to_string(),
        })
        .permissions("runtime".to_string())
        .port(8080)
        .build();
    let daemon = Daemon::new("source-daemon".to_string())
        .code(DaemonCode::Source {
            src: "daemon".to_string(),
            toolchain: ToolchainConfig::Rust {
                binary_name: "daemon".to_string(),
            },
        })
        .permissions("runtime".to_string())
        .build();

    for (stack, expected) in [
        (
            Stack::new("source-worker-stack".to_string())
                .add(worker, ResourceLifecycle::Live)
                .build(),
            "Worker 'source-worker' still has source code",
        ),
        (
            Stack::new("source-container-stack".to_string())
                .add(container, ResourceLifecycle::Live)
                .build(),
            "Container 'source-container' still has source code",
        ),
        (
            Stack::new("source-daemon-stack".to_string())
                .add(daemon, ResourceLifecycle::Live)
                .build(),
            "Daemon 'source-daemon' still has source code",
        ),
    ] {
        let error = generate_helm_chart(
            &stack,
            HelmOptions {
                registry: &registry,
                stack_settings: StackSettings::default(),
                chart_name: stack.id().to_string(),
            },
        )
        .expect_err("source workloads should fail before Helm rendering");
        assert!(
            error.to_string().contains(expected),
            "expected error to contain {expected:?}, got {error}"
        );
    }
}
