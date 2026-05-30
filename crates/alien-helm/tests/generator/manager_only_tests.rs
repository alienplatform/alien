//! Manager-only scenarios — stacks with no `Frozen` infrastructure
//! resources. Charts still render `examples/<target>.yaml` files for
//! IRSA / Workload Identity / Federated Identity.

use super::helpers::{assert_helm_valid, render, snapshot_chart};
use alien_core::{
    Container, ContainerCode, Daemon, DaemonCode, Ingress, KubernetesCertificateMode,
    KubernetesExposureSettings, KubernetesGatewayRouteProfile, KubernetesIngressRouteProfile,
    KubernetesRouteProfile, KubernetesSettings, PermissionProfile, ResourceLifecycle, ResourceSpec,
    Stack, StackSettings, ToolchainConfig, Worker, WorkerCode,
};
use alien_helm::test_utils::LinterStatus;
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
fn chart_values_include_kubernetes_exposure_contract() {
    let stack = Stack::new("k8s-exposure".to_string()).build();
    let settings = StackSettings {
        kubernetes: Some(KubernetesSettings {
            cluster: None,
            exposure: Some(KubernetesExposureSettings::Generated {
                route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
                    controller: Some("eks.amazonaws.com/alb".to_string()),
                    ingress_class_name: "alien-alb".to_string(),
                    labels: Default::default(),
                    annotations: Default::default(),
                    provider: None,
                }),
                certificate: KubernetesCertificateMode::ManagedTlsSecret {
                    secret_name_template: "alien-{{ resourceId }}-tls".to_string(),
                },
            }),
        }),
        ..StackSettings::default()
    };
    let chart = render(&stack, settings);
    let values = chart.files.get("values.yaml").expect("values.yaml");

    assert!(values.contains("stackSettings:"));
    assert!(values.contains("kubernetes:"));
    assert!(values.contains("exposure:"));
    assert!(values.contains("mode: generated"));
    assert!(values.contains("routeApi: ingress"));
    assert!(values.contains("ingressClassName: alien-alb"));
    assert!(values.contains("mode: managedTlsSecret"));
    assert!(values.contains("secretNameTemplate: alien-{{ resourceId }}-tls"));
    assert!(!values.contains("secret_name_template"));
}

#[test]
fn chart_removes_manual_public_ingress_values_and_template() {
    let worker = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "registry.example.com/api:1".to_string(),
        })
        .permissions("runtime".to_string())
        .ingress(Ingress::Public)
        .build();
    let stack = Stack::new("no-manual-ingress".to_string())
        .permission(
            "runtime",
            PermissionProfile::new().global(["worker/management"]),
        )
        .add(worker, ResourceLifecycle::Live)
        .build();
    let chart = render(&stack, StackSettings::default());
    let values = chart.files.get("values.yaml").expect("values.yaml");
    let schema = chart
        .files
        .get("values.schema.json")
        .expect("values.schema.json");
    let configmap = chart
        .files
        .get("templates/configmap.yaml")
        .expect("configmap");

    assert!(!chart.files.contains_key("templates/app-ingress.yaml"));
    assert!(!values.contains("hostless"));
    assert!(!values.contains("className"));
    assert!(!values.contains("secretName"));
    assert!(!schema.contains("\"hostless\""));
    assert!(!schema.contains("\"className\""));
    assert!(!schema.contains("\"secretName\""));
    assert!(!configmap.contains("$service.host"));
    assert!(!configmap.contains("$service.publicUrl"));
}

#[test]
fn chart_role_rbac_is_selected_by_kubernetes_route_api() {
    let stack = Stack::new("route-rbac".to_string()).build();
    let settings = StackSettings {
        kubernetes: Some(KubernetesSettings {
            cluster: None,
            exposure: Some(KubernetesExposureSettings::Generated {
                route: KubernetesRouteProfile::Gateway(KubernetesGatewayRouteProfile {
                    controller: Some("gateway.networking.k8s.io/gateway".to_string()),
                    gateway_class_name: "gke-l7-global-external-managed".to_string(),
                    listener_port: 443,
                    labels: Default::default(),
                    annotations: Default::default(),
                    provider: None,
                }),
                certificate: KubernetesCertificateMode::ManagedTlsSecret {
                    secret_name_template: "alien-{{ resourceId }}-tls".to_string(),
                },
            }),
        }),
        ..StackSettings::default()
    };
    let chart = render(&stack, settings);
    let role = chart.files.get("templates/role.yaml").expect("role");
    let values = chart.files.get("values.yaml").expect("values.yaml");

    assert!(values.contains("routeApi: gateway"));
    assert!(role.contains("resources: [\"networkpolicies\"]"));
    assert!(role.contains("resources: [\"ingresses\"]"));
    assert!(role.contains("gateway.networking.k8s.io"));
    assert!(role.contains("resources: [\"gateways\", \"httproutes\"]"));
    assert!(role.contains("eq $routeApi \"ingress\""));
    assert!(role.contains("eq $routeApi \"gateway\""));

    let rendered = alien_helm::test_utils::helm_template(&chart.files, None);
    match &rendered.status {
        LinterStatus::Passed => {
            assert!(rendered
                .stdout
                .contains(r#"resources: ["networkpolicies"]"#));
            assert!(rendered
                .stdout
                .contains(r#"apiGroups: ["gateway.networking.k8s.io"]"#));
            assert!(rendered
                .stdout
                .contains(r#"resources: ["gateways", "httproutes"]"#));
            assert!(!rendered.stdout.contains(r#"resources: ["ingresses"]"#));
        }
        LinterStatus::Skipped(reason) => {
            eprintln!("skipped rendered route RBAC assertions: {reason}");
        }
        LinterStatus::Failed(_) => rendered.assert_ok("rendered route RBAC"),
    }
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
    assert!(values.contains("heartbeat:"));
    assert!(values.contains("nodes:"));

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
fn gcp_base_platform_config_renders_agent_environment() {
    let stack = Stack::new("gcp-runtime-config".to_string()).build();
    let chart = render(&stack, StackSettings::default());
    let files = chart.files.clone();
    let values = files.get("values.yaml").expect("values.yaml");
    let gcp_values = values
        .replace("basePlatform: null", "basePlatform: gcp")
        .replace("projectId: \"\"", "projectId: alien-test-target")
        .replace("region: \"\"", "region: us-east4");

    let rendered = alien_helm::test_utils::helm_template(&files, Some(&gcp_values));
    match &rendered.status {
        LinterStatus::Passed => {
            assert!(rendered.stdout.contains("name: BASE_PLATFORM"));
            assert!(rendered.stdout.contains("value: \"gcp\""));
            assert!(rendered.stdout.contains("name: GCP_PROJECT_ID"));
            assert!(rendered.stdout.contains("value: \"alien-test-target\""));
            assert!(rendered.stdout.contains("name: GOOGLE_CLOUD_PROJECT"));
            assert!(rendered.stdout.contains("name: GCP_REGION"));
            assert!(rendered.stdout.contains("value: \"us-east4\""));
        }
        LinterStatus::Skipped(reason) => {
            eprintln!("skipped GCP runtime config render assertions: {reason}");
        }
        LinterStatus::Failed(_) => rendered.assert_ok("GCP runtime config render"),
    }
}

#[test]
fn cluster_bootstrap_renders_only_when_enabled() {
    let stack = Stack::new("cluster-bootstrap".to_string()).build();
    let chart = render(&stack, StackSettings::default());
    let files = chart.files.clone();

    let values = files.get("values.yaml").expect("values.yaml");
    assert!(values.contains("clusterBootstrap:"));
    assert!(values.contains("metricsServer:"));
    assert!(values.contains("storageClass:"));
    assert!(values.contains("eksAutoMode:"));
    assert!(values.contains("arm64NodePool:"));

    let default_rendered = alien_helm::test_utils::helm_template(&files, None);
    match &default_rendered.status {
        LinterStatus::Passed => {
            assert!(!default_rendered.stdout.contains("kind: StorageClass"));
            assert!(!default_rendered.stdout.contains("kind: IngressClass"));
            assert!(!default_rendered.stdout.contains("kind: IngressClassParams"));
            assert!(!default_rendered.stdout.contains("kind: NodePool"));
            assert!(!default_rendered.stdout.contains("metrics-server"));
        }
        LinterStatus::Skipped(reason) => {
            eprintln!("skipped default cluster bootstrap assertions: {reason}");
        }
        LinterStatus::Failed(_) => default_rendered.assert_ok("default cluster bootstrap render"),
    }

    let enabled_values = values
        .replace(
        "clusterBootstrap:\n  metricsServer:\n    enabled: false\n    image: registry.k8s.io/metrics-server/metrics-server:v0.8.1\n  storageClass:\n    default:\n      enabled: false\n      name: \"\"\n      provisioner: \"\"\n      parameters: {}\n  ingress:\n    eksAutoMode:\n      enabled: false",
        "clusterBootstrap:\n  metricsServer:\n    enabled: true\n    image: registry.k8s.io/metrics-server/metrics-server:v0.8.1\n  storageClass:\n    default:\n      enabled: true\n      name: gp3\n      provisioner: ebs.csi.eks.amazonaws.com\n      parameters:\n        type: gp3\n        fsType: ext4\n        encrypted: \"true\"\n  ingress:\n    eksAutoMode:\n      enabled: true",
        )
        .replace("arm64NodePool:\n        enabled: false", "arm64NodePool:\n        enabled: true");
    let enabled_rendered = alien_helm::test_utils::helm_template(&files, Some(&enabled_values));
    match &enabled_rendered.status {
        LinterStatus::Passed => {
            assert!(enabled_rendered.stdout.contains("kind: StorageClass"));
            assert!(enabled_rendered.stdout.contains("name: \"gp3\""));
            assert!(enabled_rendered
                .stdout
                .contains("storageclass.kubernetes.io/is-default-class: \"true\""));
            assert!(enabled_rendered
                .stdout
                .contains("provisioner: \"ebs.csi.eks.amazonaws.com\""));
            assert!(enabled_rendered.stdout.contains("kind: IngressClassParams"));
            assert!(enabled_rendered.stdout.contains("kind: IngressClass"));
            assert!(enabled_rendered
                .stdout
                .contains("controller: \"eks.amazonaws.com/alb\""));
            assert!(enabled_rendered.stdout.contains("kind: NodePool"));
            assert!(enabled_rendered
                .stdout
                .contains("name: \"general-purpose-arm64\""));
            assert!(enabled_rendered.stdout.contains("kubernetes.io/arch"));
            assert!(enabled_rendered.stdout.contains("\"arm64\""));
            assert!(enabled_rendered.stdout.contains("name: metrics-server"));
            assert!(enabled_rendered.stdout.contains("kind: APIService"));
            assert!(enabled_rendered
                .stdout
                .contains("registry.k8s.io/metrics-server/metrics-server:v0.8.1"));
        }
        LinterStatus::Skipped(reason) => {
            eprintln!("skipped enabled cluster bootstrap assertions: {reason}");
        }
        LinterStatus::Failed(_) => enabled_rendered.assert_ok("enabled cluster bootstrap render"),
    }
}

#[test]
fn heartbeat_collection_rbac_is_namespace_scoped_with_optional_node_reads() {
    let stack = Stack::new("heartbeat-rbac".to_string()).build();
    let chart = render(&stack, StackSettings::default());
    let files = chart.files.clone();

    let role = files.get("templates/role.yaml").expect("role");
    assert!(role.contains(r#"resources: ["events"]"#));
    assert!(
        role.contains(r#"resources: ["deployments", "statefulsets", "daemonsets", "replicasets"]"#)
    );
    assert!(role.contains(r#"apiGroups: ["metrics.k8s.io"]"#));
    assert!(role.contains(r#"resources: ["pods"]"#));

    let cluster_role = files
        .get("templates/clusterrole.yaml")
        .expect("cluster role");
    assert!(cluster_role.contains(r#"resources: ["nodes"]"#));
    assert!(cluster_role.contains(r#"apiGroups: ["metrics.k8s.io"]"#));

    alien_helm::test_utils::helm_template_and_validate(&files, None)
        .assert_ok("heartbeat RBAC default values");

    let values = files.get("values.yaml").expect("values.yaml");
    let disabled_values = values.replace(
        "heartbeat:\n  collection:\n    nodes:\n      enabled: true",
        "heartbeat:\n  collection:\n    nodes:\n      enabled: false",
    );
    alien_helm::test_utils::helm_template_and_validate(&files, Some(&disabled_values))
        .assert_ok("heartbeat RBAC node collection disabled");

    let rendered = alien_helm::test_utils::helm_template(&files, Some(&disabled_values));
    match &rendered.status {
        LinterStatus::Passed => {
            assert!(!rendered.stdout.contains("kind: ClusterRole"));
            assert!(!rendered.stdout.contains("kind: ClusterRoleBinding"));
            assert!(!rendered.stdout.contains(r#"resources: ["nodes"]"#));
            assert!(rendered.stdout.contains(r#"resources: ["events"]"#));
            assert!(rendered.stdout.contains(
                r#"resources: ["deployments", "statefulsets", "daemonsets", "replicasets"]"#
            ));
            assert!(rendered.stdout.contains(r#"apiGroups: ["metrics.k8s.io"]"#));
            assert!(rendered.stdout.contains(r#"resources: ["pods"]"#));
        }
        LinterStatus::Skipped(reason) => {
            eprintln!("skipped rendered RBAC assertions: {reason}");
        }
        LinterStatus::Failed(_) => {
            rendered.assert_ok("rendered heartbeat RBAC node collection disabled")
        }
    }
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
