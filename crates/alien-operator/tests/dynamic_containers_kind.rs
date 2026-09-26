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
        let reports = reconcile(client, namespace, deployment_id, targets, None)
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
    let config = alien_k8s_clients::KubernetesClientConfig::try_kubeconfig()
        .await
        .expect("Kind kubeconfig");
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
    until_running(&client, &namespace, deployment_id, &[rotated, second]).await;
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

    for _ in 0..30 {
        reconcile(&client, &namespace, deployment_id, &[], None)
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
