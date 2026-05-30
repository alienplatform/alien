//! EKS CloudFormation target coverage.

use super::helpers::render_built_ins_target;
use alien_cloudformation::{CloudFormationTarget, RegistrationMode};
use alien_core::{
    KubernetesCluster, KubernetesClusterOwnership, KubernetesClusterProvider,
    KubernetesHeartbeatMode, PermissionProfile, RemoteStackManagement, ResourceLifecycle,
    ServiceAccount, Stack, StackSettings,
};

#[test]
fn eks_target_renders_managed_cluster_and_kubernetes_import_payload() {
    let stack = Stack::new("kubernetes".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("alien".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins_target(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        CloudFormationTarget::Eks,
        "kubernetes",
        "eks managed cluster",
    );

    assert!(yaml.contains("Type: AWS::EKS::Cluster"));
    assert!(yaml.contains("Type: AWS::IAM::OIDCProvider"));
    assert!(yaml.contains("DeploymentPlatform:"));
    assert!(yaml.contains("Value: kubernetes"));
    assert!(yaml.contains("DeploymentBasePlatform:"));
    assert!(yaml.contains("Value: aws"));
    assert!(yaml.contains("DeploymentHelmValues:"));
    assert!(yaml.contains("namespace: alien"));
    assert!(yaml.contains("clusterBootstrap:"));
    assert!(yaml.contains("metricsServer:"));
    let bootstrap_ingress = yaml
        .split("clusterBootstrap:")
        .nth(1)
        .expect("helm values should include clusterBootstrap")
        .split("compute:")
        .next()
        .expect("clusterBootstrap ingress should precede compute bootstrap");
    assert!(!bootstrap_ingress.contains("targetType"));
    assert!(yaml.contains("NodePools:"));
    assert!(yaml.contains("- system"));
    assert!(!yaml.contains("general-purpose"));
}

#[test]
fn eks_target_irsa_references_generated_cluster_resource() {
    let stack = Stack::new("kubernetes".to_string())
        .permission(
            "execution",
            PermissionProfile::new().global(["storage/data-read"]),
        )
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("alien".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins_target(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        CloudFormationTarget::Eks,
        "kubernetes",
        "eks managed cluster with irsa",
    );

    assert!(yaml.contains("- KubernetesCluster"));
    assert!(!yaml.contains("- Kubernetes\n"));
    assert!(yaml.contains("system:serviceaccount:alien:${AWS::StackName}-manager-sa"));
    assert!(yaml.contains("system:serviceaccount:alien:${AWS::StackName}-execution-sa"));
}
