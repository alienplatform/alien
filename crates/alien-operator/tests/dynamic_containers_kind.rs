//! Run explicitly against a disposable Kind cluster; never against the default kube context.

use std::collections::BTreeMap;
use std::process::Command;
use std::time::Duration;

use alien_core::sync::{DynamicContainerStatus, TargetDynamicContainer};
use alien_k8s_clients::{KubernetesClient, KubernetesClientConfigExt};
use alien_operator::loops::dynamic_containers::reconcile;

fn require_kind_context() {
    let expected = std::env::var("ALIEN_DYNAMIC_KIND_CONTEXT").expect("set explicit Kind context");
    assert!(
        expected.starts_with("kind-"),
        "only Kind contexts are accepted"
    );
    let kubeconfig = std::env::var("KUBECONFIG").expect("set explicit disposable kubeconfig");
    assert!(!kubeconfig.is_empty());
    let output = Command::new("kubectl")
        .args(["config", "current-context"])
        .output()
        .expect("kubectl current-context");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
}

fn compute_stack() -> alien_core::Stack {
    let architecture = match std::env::consts::ARCH {
        "aarch64" => alien_core::instance_catalog::Architecture::Arm64,
        "x86_64" => alien_core::instance_catalog::Architecture::X86_64,
        other => panic!("unsupported Kind host architecture {other}"),
    };
    let compute = alien_core::ComputeCluster::new("compute".to_string())
        .capacity_group(alien_core::CapacityGroup {
            group_id: "apps".to_string(),
            instance_type: None,
            profile: Some(alien_core::MachineProfile {
                cpu: "2".to_string(),
                memory_bytes: 4 << 30,
                ephemeral_storage_bytes: 20 << 30,
                architecture: Some(architecture),
                gpu: None,
            }),
            min_size: 1,
            max_size: 3,
            scale_policy: None,
            nested_virtualization: None,
        })
        .dynamic_container_pool("apps".to_string())
        .build();
    alien_core::Stack::new("portable".to_string())
        .add(compute, alien_core::ResourceLifecycle::Frozen)
        .build()
}

fn target(name: &str, generation: u64, replicas: u32) -> TargetDynamicContainer {
    TargetDynamicContainer {
        name: name.to_string(),
        generation,
        // A fixed test image is loaded into Kind before the run. Production
        // admission requires an approved digest through the container API.
        image: "docker.io/library/memcached:alien-kind-test".to_string(),
        cpu: "0.1".to_string(),
        memory: "128Mi".to_string(),
        replicas,
        ports: vec![11211],
        deleted: false,
        env: BTreeMap::new(),
        secret_env: BTreeMap::from([("API_KEY".to_string(), "secret-test-value".to_string())]),
        health_check: None,
        suspended_reason: None,
    }
}

async fn until_running(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
    targets: &[TargetDynamicContainer],
) {
    for _ in 0..60 {
        let reports = reconcile(
            client,
            namespace,
            deployment_id,
            targets,
            None,
            &compute_stack(),
        )
        .await
        .expect("reconcile");
        assert!(
            reports
                .iter()
                .all(|report| report.status != DynamicContainerStatus::Failing),
            "{reports:?}"
        );
        if reports
            .iter()
            .all(|report| report.status == DynamicContainerStatus::Running)
        {
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("containers did not become ready");
}

#[tokio::test]
#[ignore = "requires explicit disposable Kind kubeconfig"]
async fn two_independent_containers_update_and_delete() {
    require_kind_context();
    let namespace = std::env::var("ALIEN_DYNAMIC_KIND_NAMESPACE").expect("set Kind namespace");
    assert!(namespace.starts_with("alien-dynamic-"));
    let deployment_id = "dep_kind_dynamic_1040";
    let mut config = alien_k8s_clients::KubernetesClientConfig::try_kubeconfig()
        .await
        .expect("Kind kubeconfig");
    if let alien_core::KubernetesClientConfig::Kubeconfig {
        namespace: configured_namespace,
        ..
    } = &mut config
    {
        *configured_namespace = Some(namespace.clone());
    }
    // Exercise setup handoff and the real namespaced API, not a mocked controller.
    let stack = compute_stack();
    let deployment_config = alien_core::DeploymentConfig::builder()
        .stack_settings(alien_core::StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let executor = alien_infra::StackExecutor::builder(
        &stack,
        alien_core::ClientConfig::Kubernetes(Box::new(config.clone())),
    )
    .deployment_config(&deployment_config)
    .lifecycle_filter(vec![alien_core::ResourceLifecycle::Frozen])
    .build()
    .expect("build existing-node executor");
    let mut imported = alien_core::StackState::new(alien_core::Platform::Kubernetes);
    for _ in 0..5 {
        imported = executor
            .continue_imported(imported)
            .await
            .expect("continue existing-node compute handoff")
            .next_state;
        if imported.resources["compute"].status == alien_core::ResourceStatus::Running {
            break;
        }
    }
    assert_eq!(
        imported.resources["compute"].status,
        alien_core::ResourceStatus::Running
    );
    assert!(imported.resources["compute"].outputs.is_none());
    let client = KubernetesClient::new(
        alien_infra::resolve_kubeconfig(&config)
            .await
            .expect("resolve Kind kubeconfig"),
    )
    .await
    .expect("Kubernetes client");

    let mut first = target("first", 1, 1);
    first.ports = vec![11211, 11212];
    let second = target("second", 1, 1);
    let mut invalid_stack = compute_stack();
    invalid_stack
        .resources
        .get_mut("compute")
        .unwrap()
        .config
        .downcast_mut::<alien_core::ComputeCluster>()
        .unwrap()
        .dynamic_container_pool = Some("missing".to_string());
    let rejected = reconcile(
        &client,
        &namespace,
        deployment_id,
        &[first.clone()],
        None,
        &invalid_stack,
    )
    .await
    .expect("invalid admission is reported per target");
    assert_eq!(rejected[0].status, DynamicContainerStatus::Failing);
    assert!(client
        .list_secrets(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None
        )
        .await
        .unwrap()
        .items
        .is_empty());
    assert!(client
        .list_services(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None
        )
        .await
        .unwrap()
        .items
        .is_empty());
    assert!(client
        .list_deployments(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None
        )
        .await
        .unwrap()
        .items
        .is_empty());

    until_running(
        &client,
        &namespace,
        deployment_id,
        &[first.clone(), second.clone()],
    )
    .await;

    let owned = client
        .list_deployments(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None,
        )
        .await
        .expect("owned Deployments");
    assert_eq!(owned.items.len(), 2);
    let expected_architecture = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "amd64"
    };
    for workload in &owned.items {
        let selector = workload
            .spec
            .as_ref()
            .unwrap()
            .template
            .spec
            .as_ref()
            .unwrap()
            .node_selector
            .as_ref()
            .unwrap();
        assert_eq!(
            selector.get("kubernetes.io/arch").map(String::as_str),
            Some(expected_architecture)
        );
    }

    let services = client
        .list_services(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None,
        )
        .await
        .expect("owned Services");
    let multiport = services
        .items
        .iter()
        .find(|service| {
            service
                .spec
                .as_ref()
                .is_some_and(|spec| spec.ports.as_ref().is_some_and(|ports| ports.len() == 2))
        })
        .expect("multiport Service");
    let port_names: Vec<_> = multiport
        .spec
        .as_ref()
        .unwrap()
        .ports
        .as_ref()
        .unwrap()
        .iter()
        .map(|port| port.name.as_deref())
        .collect();
    assert_eq!(port_names, [Some("tcp-11211"), Some("tcp-11212")]);
    let versions: BTreeMap<_, _> = owned
        .items
        .iter()
        .map(|item| {
            (
                item.metadata.name.clone().unwrap(),
                item.metadata.resource_version.clone().unwrap(),
            )
        })
        .collect();
    reconcile(
        &client,
        &namespace,
        deployment_id,
        &[first.clone(), second.clone()],
        None,
        &compute_stack(),
    )
    .await
    .expect("idempotent reconcile");
    let unchanged = client
        .list_deployments(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None,
        )
        .await
        .expect("unchanged Deployments");
    for deployment in &unchanged.items {
        assert_eq!(
            deployment.metadata.resource_version.as_ref().unwrap(),
            versions
                .get(deployment.metadata.name.as_ref().unwrap())
                .unwrap(),
            "an unchanged target must not roll the Deployment"
        );
    }
    for deployment in &owned.items {
        let pod = deployment
            .spec
            .as_ref()
            .unwrap()
            .template
            .spec
            .as_ref()
            .unwrap();
        assert_eq!(pod.automount_service_account_token, Some(false));
        assert!(pod.image_pull_secrets.is_none());
        assert_eq!(pod.security_context.as_ref().unwrap().fs_group, Some(65532));
        let app = &pod.containers[0];
        assert_eq!(
            app.security_context.as_ref().unwrap().run_as_user,
            Some(65532)
        );
        assert_eq!(
            app.security_context
                .as_ref()
                .unwrap()
                .read_only_root_filesystem,
            Some(true)
        );
        let secret = app
            .env
            .as_ref()
            .unwrap()
            .iter()
            .find(|entry| entry.name == "API_KEY")
            .unwrap();
        assert!(secret.value.is_none());
        assert!(secret.value_from.as_ref().unwrap().secret_key_ref.is_some());
    }

    let mut updated = first.clone();
    updated.generation = 2;
    updated.replicas = 2;
    until_running(
        &client,
        &namespace,
        deployment_id,
        &[updated.clone(), second.clone()],
    )
    .await;
    let owned = client
        .list_deployments(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None,
        )
        .await
        .expect("owned Deployments after update");
    let replicas: Vec<i32> = owned
        .items
        .iter()
        .map(|item| item.spec.as_ref().unwrap().replicas.unwrap())
        .collect();
    assert!(replicas.contains(&1) && replicas.contains(&2));

    // Changing a secret advances the Pod template generation, so both replicas
    // consume the new value instead of keeping the old environment in memory.
    let mut rotated = updated.clone();
    rotated.generation = 3;
    rotated
        .secret_env
        .insert("API_KEY".to_string(), "rotated-test-value".to_string());
    until_running(
        &client,
        &namespace,
        deployment_id,
        &[rotated.clone(), second.clone()],
    )
    .await;
    let rotated_deployment = client
        .list_deployments(
            &namespace,
            Some(format!(
                "alien.dev/dynamic-deployment={deployment_id},alien.dev/dynamic-container=first"
            )),
            None,
        )
        .await
        .expect("rotated Deployment")
        .items
        .into_iter()
        .next()
        .expect("first Deployment");
    let pod = rotated_deployment.spec.unwrap().template.spec.unwrap();
    assert!(pod.containers[0].env.as_ref().unwrap().iter().any(|entry| {
        entry.name == "ALIEN_DYNAMIC_GENERATION" && entry.value.as_deref() == Some("3")
    }));
    let secret = client
        .list_secrets(
            &namespace,
            Some(format!(
                "alien.dev/dynamic-deployment={deployment_id},alien.dev/dynamic-container=first"
            )),
            None,
        )
        .await
        .expect("rotated Secret")
        .items
        .into_iter()
        .find(|secret| secret.metadata.name.as_deref().unwrap().ends_with("-env"))
        .expect("environment Secret");
    assert_eq!(secret.data.unwrap()["API_KEY"].0, b"rotated-test-value");

    reconcile(
        &client,
        &namespace,
        deployment_id,
        &[rotated.clone(), second.clone()],
        Some(("docker.io", "synthetic-test-token")),
        &compute_stack(),
    )
    .await
    .expect("add manager registry credential");
    let registry_secrets = client
        .list_secrets(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None,
        )
        .await
        .expect("registry Secrets");
    assert!(registry_secrets.items.iter().any(|secret| {
        secret
            .metadata
            .name
            .as_deref()
            .unwrap()
            .ends_with("-registry")
    }));
    // A rejected Service update must leave the old workload's pull credential
    // intact. This models a failure before the Deployment can change image.
    let mut rejected = rotated.clone();
    rejected.image = "external.example/other:latest".to_string();
    rejected.ports = vec![0];
    let failed = reconcile(
        &client,
        &namespace,
        deployment_id,
        &[rejected, second.clone()],
        Some(("docker.io", "synthetic-test-token")),
        &compute_stack(),
    )
    .await
    .expect("report rejected Service update");
    assert_eq!(failed[0].status, DynamicContainerStatus::Failing);
    let secrets_after_failure = client
        .list_secrets(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None,
        )
        .await
        .expect("registry Secret after failed update");
    assert!(secrets_after_failure.items.iter().any(|secret| {
        secret
            .metadata
            .name
            .as_deref()
            .unwrap()
            .ends_with("-registry")
    }));
    until_running(
        &client,
        &namespace,
        deployment_id,
        &[rotated, second.clone()],
    )
    .await;
    let registry_secrets = client
        .list_secrets(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None,
        )
        .await
        .expect("unused registry Secrets");
    assert!(registry_secrets.items.iter().all(|secret| {
        !secret
            .metadata
            .name
            .as_deref()
            .unwrap()
            .ends_with("-registry")
    }));

    // Invalid active placement must not strand a requested deletion or mutate
    // the blocked application's existing workload.
    let mut deleted = first.clone();
    deleted.deleted = true;
    let existing_second = client
        .list_deployments(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None,
        )
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|item| {
            item.metadata.labels.as_ref().is_some_and(|labels| {
                labels
                    .get("alien.dev/dynamic-container")
                    .is_some_and(|name| name == "second")
            })
        })
        .expect("second workload exists");
    let mut deletion_stopped = false;
    for _ in 0..30 {
        let reports = reconcile(
            &client,
            &namespace,
            deployment_id,
            &[deleted.clone(), second.clone()],
            None,
            &invalid_stack,
        )
        .await
        .expect("mixed blocked active and deleted targets");
        assert_eq!(reports[1].status, DynamicContainerStatus::Failing);
        if reports[0].status == DynamicContainerStatus::Stopped {
            deletion_stopped = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    assert!(
        deletion_stopped,
        "deleted target must finish removing all owned resources"
    );
    let deleted_selector =
        format!("alien.dev/dynamic-deployment={deployment_id},alien.dev/dynamic-container=first");
    assert!(client
        .list_services(&namespace, Some(deleted_selector.clone()), None)
        .await
        .unwrap()
        .items
        .is_empty());
    assert!(client
        .list_secrets(&namespace, Some(deleted_selector), None)
        .await
        .unwrap()
        .items
        .is_empty());
    let remaining = client
        .list_deployments(
            &namespace,
            Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
            None,
        )
        .await
        .unwrap();
    assert_eq!(remaining.items.len(), 1);
    assert_eq!(
        remaining.items[0].metadata.resource_version,
        existing_second.metadata.resource_version
    );

    for _ in 0..30 {
        reconcile(
            &client,
            &namespace,
            deployment_id,
            &[],
            None,
            &compute_stack(),
        )
        .await
        .expect("delete owned objects");
        let remaining = client
            .list_deployments(
                &namespace,
                Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
                None,
            )
            .await
            .expect("remaining Deployments");
        if remaining.items.is_empty() {
            let services = client
                .list_services(
                    &namespace,
                    Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
                    None,
                )
                .await
                .expect("remaining Services");
            let secrets = client
                .list_secrets(
                    &namespace,
                    Some(format!("alien.dev/dynamic-deployment={deployment_id}")),
                    None,
                )
                .await
                .expect("remaining Secrets");
            if services.items.is_empty() && secrets.items.is_empty() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("owned Kubernetes objects survived empty target");
}
