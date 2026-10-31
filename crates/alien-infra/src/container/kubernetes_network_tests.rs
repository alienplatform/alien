use super::*;
use crate::core::kubernetes_manifest_test_support::KubernetesManifestTestHarness;
use alien_core::{
    ComputeCluster, ContainerCode, Resource, ResourceLifecycle, ResourceRef, ResourceSpec, Worker,
    WorkerCode,
};

fn container(id: &str, cluster: &str) -> Container {
    Container::new(id.into())
        .code(ContainerCode::Image {
            image: "python:3.13-slim".into(),
        })
        .cpu(ResourceSpec {
            min: "100m".into(),
            desired: "100m".into(),
        })
        .memory(ResourceSpec {
            min: "64Mi".into(),
            desired: "64Mi".into(),
        })
        .permissions("default".into())
        .cluster(cluster.into())
        .port(8080)
        .build()
}

fn source_stack(target: &Container) -> Stack {
    let mut linked = container("linked", "other");
    linked.links.push(ResourceRef::new(
        Container::RESOURCE_TYPE,
        target.id.clone(),
    ));
    let worker = Worker::new("worker".into())
        .permissions("default".into())
        .code(WorkerCode::Image {
            image: "python:3.13-slim".into(),
        })
        .link(target)
        .build();
    let mut sandbox = alien_core::Sandbox::new("session-source".into())
        .egress(alien_core::SandboxEgress::Internet)
        .lifecycle(alien_core::SandboxLifecyclePolicy {
            max_lifetime_seconds: Some(300),
            idle_pause_seconds: None,
        })
        .code(alien_core::SandboxCode::Image {
            image: "python:3.13-slim".into(),
            bundled_from: None,
        })
        .build();
    sandbox.links.push(ResourceRef::new(
        Container::RESOURCE_TYPE,
        target.id.clone(),
    ));
    Stack::new("test".into())
        .add(sandbox, ResourceLifecycle::Frozen)
        .add(target.clone(), ResourceLifecycle::Live)
        .add(
            container("cluster-member", "primary"),
            ResourceLifecycle::Live,
        )
        .add(container("unlinked", "other"), ResourceLifecycle::Live)
        .add(linked, ResourceLifecycle::Live)
        .add(worker, ResourceLifecycle::Live)
        .build()
}

#[test]
fn cluster_members_and_linkers_gain_reach_and_unlink_revokes_it() {
    let target = container("target", "primary");
    let mut stack = source_stack(&target);
    assert_eq!(
        allowed_sources(&target, &stack),
        [
            "cluster-member",
            "linked",
            "session-source",
            "target",
            "worker"
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    );
    stack
        .resources
        .get_mut("linked")
        .unwrap()
        .config
        .downcast_mut::<Container>()
        .unwrap()
        .links
        .clear();
    assert!(!allowed_sources(&target, &stack).contains("linked"));
    assert!(allowed_sources(&target, &stack).contains("cluster-member"));
}

#[test]
fn missing_sources_deny_ingress_and_foreign_deployments_cannot_own_policy() {
    let target = container("target", "primary");
    let harness = KubernetesManifestTestHarness::new(Resource::new(target.clone()), vec![]);
    let ctx = harness.ctx();
    let mut policy = build_policy(&target, "target", "test-ns", &ctx);
    assert!(policy
        .spec
        .as_ref()
        .unwrap()
        .ingress
        .as_ref()
        .unwrap()
        .is_empty());
    require_owner(&policy, &ctx, &target.id).unwrap();
    policy
        .metadata
        .labels
        .as_mut()
        .unwrap()
        .insert("alien.dev/deployment".into(), "another-deployment".into());
    assert!(require_owner(&policy, &ctx, &target.id).is_err());
}

/// Applies the controller's actual policy through its authenticated client. The
/// companion traffic probe uses this isolated namespace to exercise CNI behavior.
#[tokio::test]
#[ignore = "requires ALIEN_TEST_KUBECONFIG and an isolated alien-network-test namespace"]
async fn live_policy_client_round_trip() {
    use alien_core::{ClientConfig, KubernetesClientConfig};
    let path = std::env::var("ALIEN_TEST_KUBECONFIG").expect("explicit test kubeconfig");
    let target = container("target", "primary");
    let mut stack = source_stack(&target);
    // Optional public-address fixtures let the traffic probe distinguish these
    // deployment exclusions from the built-in private/reserved-address floor.
    if let Ok(cidr) = std::env::var("ALIEN_TEST_CONTAINER_OVERLAY_CIDR") {
        let overlay = ComputeCluster::new("probe-overlay".into())
            .container_cidr(cidr)
            .build();
        stack.resources.extend(
            Stack::new("probe".into())
                .add(overlay, ResourceLifecycle::Frozen)
                .build()
                .resources,
        );
    }
    let harness = KubernetesManifestTestHarness::new(Resource::new(target.clone()), vec![]);
    let mut ctx = harness.ctx();
    ctx.desired_stack = &stack;
    let mut deployment_config = ctx.deployment_config.clone();
    if let Ok(cidr) = std::env::var("ALIEN_TEST_DEPLOYMENT_CIDR") {
        deployment_config.stack_settings.network = Some(alien_core::NetworkSettings::Create {
            cidr: Some(cidr),
            availability_zones: 1,
        });
    }
    ctx.deployment_config = &deployment_config;
    ctx.client_config = ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::Kubeconfig {
        kubeconfig_path: Some(path),
        context: None,
        cluster: None,
        user: None,
        namespace: Some("alien-network-test".into()),
        additional_headers: None,
    }));
    let mut controller =
        KubernetesContainerController::mock_ready("target", "alien-network-test", false);
    controller
        .reconcile_network_policy(&target, "target", "alien-network-test", &ctx)
        .await
        .unwrap();
    let client = ctx
        .service_provider
        .get_kubernetes_network_policy_client(ctx.get_kubernetes_config().unwrap())
        .await
        .unwrap();
    let original = client
        .get_network_policy("alien-network-test", "target")
        .await
        .unwrap();
    // Updating the same owned object must preserve its UID and use resourceVersion.
    let mut changed = target.clone();
    changed.ports[0].port = 8082;
    controller
        .reconcile_network_policy(&changed, "target", "alien-network-test", &ctx)
        .await
        .unwrap();
    let updated = client
        .get_network_policy("alien-network-test", "target")
        .await
        .unwrap();
    assert_eq!(original.metadata.uid, updated.metadata.uid);
    assert!(updated.metadata.resource_version.is_some());
    // Exercise deletion too, then restore the real policy for the traffic probe.
    controller
        .delete_network_policy("alien-network-test", &target.id, &ctx)
        .await
        .unwrap();
    let error = client
        .get_network_policy("alien-network-test", "target")
        .await
        .unwrap_err();
    assert!(matches!(
        error.error,
        Some(ClientError::RemoteResourceNotFound { .. })
    ));
    controller
        .reconcile_network_policy(&target, "target", "alien-network-test", &ctx)
        .await
        .unwrap();
    let mut outbound = container("outbound", "other");
    outbound.ports.clear();
    outbound.egress = Some(alien_core::ContainerEgress::Internet);
    let mut outbound_controller =
        KubernetesContainerController::mock_ready("outbound", "alien-network-test", false);
    outbound_controller
        .reconcile_network_policy(&outbound, "outbound", "alien-network-test", &ctx)
        .await
        .unwrap();

    // Two actual pods with this resource identity exercise replica-only reach.
    let mut replicated = container("replicated", "primary");
    replicated.ports[0].port = 3129;
    replicated.egress = Some(alien_core::ContainerEgress::Internet);
    let mut replica_stack = stack.clone();
    replica_stack.resources.extend(
        Stack::new("replica-probe".into())
            .add(replicated.clone(), ResourceLifecycle::Live)
            .build()
            .resources,
    );
    ctx.desired_stack = &replica_stack;
    let mut replica_controller =
        KubernetesContainerController::mock_ready("replicated", "alien-network-test", false);
    replica_controller
        .reconcile_network_policy(&replicated, "replicated", "alien-network-test", &ctx)
        .await
        .unwrap();

    // Exercise real Service API immutability, updates, retirement and ownership
    // before leaving the headless endpoint available for the live DNS probe.
    let services = ctx
        .service_provider
        .get_kubernetes_service_client(ctx.get_kubernetes_config().unwrap())
        .await
        .unwrap();
    replica_controller
        .reconcile_internal_service(&replicated, "replicated", "alien-network-test", &ctx)
        .await
        .unwrap();
    let original_client = services
        .get_service("alien-network-test", "replicated")
        .await
        .unwrap();
    let original_peer = services
        .get_service("alien-network-test", "replicated-peers")
        .await
        .unwrap();
    assert_ne!(
        original_client.spec.as_ref().unwrap().cluster_ip.as_deref(),
        Some("None")
    );
    assert_eq!(
        original_peer.spec.as_ref().unwrap().cluster_ip.as_deref(),
        Some("None")
    );
    let mut changed = replicated.clone();
    changed.ports[0].port = 3130;
    replica_controller
        .reconcile_internal_service(&changed, "replicated", "alien-network-test", &ctx)
        .await
        .unwrap();
    let updated_client = services
        .get_service("alien-network-test", "replicated")
        .await
        .unwrap();
    let updated_peer = services
        .get_service("alien-network-test", "replicated-peers")
        .await
        .unwrap();
    assert_eq!(original_client.metadata.uid, updated_client.metadata.uid);
    assert_eq!(
        original_client.spec.as_ref().unwrap().cluster_ip,
        updated_client.spec.as_ref().unwrap().cluster_ip
    );
    assert_eq!(original_peer.metadata.uid, updated_peer.metadata.uid);
    assert_eq!(
        updated_peer.spec.as_ref().unwrap().ports.as_ref().unwrap()[0].port,
        3130
    );

    changed.egress = None;
    replica_controller
        .reconcile_internal_service(&changed, "replicated", "alien-network-test", &ctx)
        .await
        .unwrap();
    let removed = services
        .get_service("alien-network-test", "replicated-peers")
        .await
        .unwrap_err();
    assert!(matches!(
        removed.error,
        Some(ClientError::RemoteResourceNotFound { .. })
    ));
    assert!(replica_controller.peer_service_name.is_none());
    assert_eq!(
        services
            .get_service("alien-network-test", "replicated")
            .await
            .unwrap()
            .metadata
            .uid,
        original_client.metadata.uid
    );

    replica_controller
        .reconcile_internal_service(&replicated, "replicated", "alien-network-test", &ctx)
        .await
        .unwrap();
    let mut foreign = services
        .get_service("alien-network-test", "replicated-peers")
        .await
        .unwrap();
    foreign
        .metadata
        .labels
        .as_mut()
        .unwrap()
        .insert("alien.dev/deployment".into(), "foreign".into());
    services
        .update_service("alien-network-test", "replicated-peers", &foreign)
        .await
        .unwrap();
    let foreign = services
        .get_service("alien-network-test", "replicated-peers")
        .await
        .unwrap();
    let error = replica_controller
        .reconcile_internal_service(&replicated, "replicated", "alien-network-test", &ctx)
        .await
        .unwrap_err();
    assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
    replica_controller
        .delete_internal_service(
            "alien-network-test",
            "replicated-peers",
            &replicated.id,
            &ctx,
            false,
        )
        .await
        .unwrap();
    let mut preserved = services
        .get_service("alien-network-test", "replicated-peers")
        .await
        .unwrap();
    assert_eq!(
        preserved.metadata.resource_version,
        foreign.metadata.resource_version
    );
    preserved
        .metadata
        .labels
        .as_mut()
        .unwrap()
        .insert("alien.dev/deployment".into(), "test".into());
    services
        .update_service("alien-network-test", "replicated-peers", &preserved)
        .await
        .unwrap();

    let mut no_ports = replicated.clone();
    no_ports.ports.clear();
    replica_controller
        .reconcile_internal_service(&no_ports, "replicated", "alien-network-test", &ctx)
        .await
        .unwrap();
    for name in ["replicated", "replicated-peers"] {
        let error = services
            .get_service("alien-network-test", name)
            .await
            .unwrap_err();
        assert!(matches!(
            error.error,
            Some(ClientError::RemoteResourceNotFound { .. })
        ));
    }
    replica_controller
        .reconcile_internal_service(&replicated, "replicated", "alien-network-test", &ctx)
        .await
        .unwrap();
}

#[test]
fn node_local_dns_is_reachable_only_from_inside_the_cluster() {
    let in_cluster = "search ns.svc.cluster.local svc.cluster.local cluster.local\nnameserver 169.254.20.10\noptions ndots:5\n";
    let rule = resolver_rule(Some(in_cluster)).expect("cluster resolver rule");
    let peers = rule.to.unwrap();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].ip_block.as_ref().unwrap().cidr, "169.254.20.10/32");
    assert!(peers[0].ip_block.as_ref().unwrap().except.is_none());
    assert_eq!(
        rule.ports.unwrap(),
        vec![port(53, "UDP"), port(53, "TCP")],
        "only DNS, never other traffic to the resolver"
    );
    // A controller outside the cluster reads the host's resolvers, not the cluster's.
    assert!(resolver_rule(Some("nameserver 10.0.0.2\nsearch corp.example\n")).is_none());
    assert!(resolver_rule(None).is_none());
}
