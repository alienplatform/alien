//! EKS CloudFormation target coverage.

use super::helpers::render_built_ins_target;
use alien_cloudformation::{CloudFormationTarget, RegistrationMode};
use alien_core::{
    KubernetesCertificateMode, KubernetesCluster, KubernetesClusterOwnership,
    KubernetesClusterProvider, KubernetesExposureSettings, KubernetesHeartbeatMode,
    KubernetesIngressRouteProfile, KubernetesRouteProfile, KubernetesRouteProviderOptions,
    KubernetesSettings, PermissionProfile, RemoteStackManagement, ResourceLifecycle,
    ServiceAccount, Stack, StackSettings, Storage,
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
    assert!(!yaml.contains("DeploymentHelmValues:"));
    assert!(yaml.contains("KubernetesClusterRole:"));
    let cluster_role = yaml
        .split("KubernetesClusterRole:")
        .nth(1)
        .expect("template should include EKS cluster role")
        .split("KubernetesNodeRole:")
        .next()
        .expect("cluster role should precede node role");
    assert!(cluster_role.contains("sts:AssumeRole"));
    assert!(cluster_role.contains("sts:TagSession"));
    let node_role = yaml
        .split("KubernetesNodeRole:")
        .nth(1)
        .expect("template should include EKS node role")
        .split("Kubernetes:")
        .next()
        .expect("node role should precede cluster");
    assert!(node_role.contains("sts:AssumeRole"));
    assert!(!node_role.contains("sts:TagSession"));
    assert!(!yaml.contains("Type: AWS::EKS::Nodegroup"));
    assert!(!yaml.contains("KubernetesManagedNodeRole:"));
    assert!(!yaml.contains("t4g.medium"));
    assert!(!yaml.contains("managedAcmImport"));
    assert!(yaml.contains("NodePools:"));
    assert!(yaml.contains("- system"));
    assert!(yaml.contains("- general-purpose"));
}

#[test]
fn eks_target_remote_management_uses_management_role_trust() {
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
        "eks remote management",
    );

    assert!(yaml.contains("DeploymentManagementConfig:"));
    assert!(yaml.contains("Value: 'null'") || yaml.contains("Value: null"));
    assert!(yaml.contains("ManagementRole:"));
    assert!(yaml.contains("sts:AssumeRole"));
    assert!(yaml.contains("AllowManagingRole"));
    assert!(!yaml.contains("sts:AssumeRoleWithWebIdentity"));
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
    assert!(yaml.contains("KubernetesOidcProvider:"));
    assert!(yaml.contains("OpenIdConnectIssuerUrl"));
    assert!(!yaml.contains("EbsCsiAddon"));
    assert!(!yaml.contains("aws-ebs-csi-driver"));
}

#[test]
fn eks_target_preserves_configured_kubernetes_exposure() {
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
    let mut settings = StackSettings::default();
    settings.kubernetes = Some(KubernetesSettings {
        cluster: None,
        exposure: Some(KubernetesExposureSettings::Generated {
            route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
                controller: Some("eks.amazonaws.com/alb".to_string()),
                ingress_class_name: "custom-alb".to_string(),
                provider: Some(KubernetesRouteProviderOptions::AwsAlb {
                    scheme: "internet-facing".to_string(),
                    target_type: "ip".to_string(),
                    ip_address_type: None,
                    subnet_ids: vec![],
                }),
                ..Default::default()
            }),
            certificate: KubernetesCertificateMode::None,
        }),
    });

    let yaml = render_built_ins_target(
        &stack,
        settings,
        RegistrationMode::OutputsFallback,
        CloudFormationTarget::Eks,
        "kubernetes",
        "eks managed cluster no tls exposure",
    );

    assert!(yaml.contains("namespace: alien"));
    assert!(yaml.contains("mode: generated"));
    assert!(yaml.contains("mode: none"));
    assert!(yaml.contains("ingressClassName: custom-alb"));
    assert!(!yaml.contains("managedAcmImport"));
}

#[test]
fn eks_custom_domain_requires_acm_certificate_arn() {
    let stack = Stack::new("kubernetes".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("default".to_string())
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
        "eks custom domain certificate validation",
    );
    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");

    assert_eq!(
        template["Parameters"]["CertificateArn"]["AllowedPattern"],
        serde_json::Value::String(
            "^$|^arn:aws(-[a-z]+)?:acm:[a-z0-9-]+:[0-9]{12}:certificate/.+$".to_string()
        )
    );

    let rule = &template["Rules"]["CustomDomainCertificate"]["Assertions"][0];
    assert_eq!(
        rule["AssertDescription"],
        serde_json::Value::String(
            "CertificateArn must be set to an AWS ACM certificate ARN when DomainName is set."
                .to_string()
        )
    );
    assert!(
        rule["Assert"]["Fn::Or"].is_array(),
        "custom-domain certificate rule should be a CloudFormation Fn::Or assertion"
    );
}

#[test]
fn eks_target_attaches_storage_permissions_to_irsa_service_account() {
    let stack = Stack::new("kubernetes".to_string())
        .permission(
            "app",
            PermissionProfile::new().resource("files", ["storage/data-write"]),
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
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("app-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins_target(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        CloudFormationTarget::Eks,
        "kubernetes",
        "eks storage permissions",
    );

    assert!(yaml.contains("Type: AWS::IAM::Policy"));
    assert!(yaml.contains("- Ref: AppSaRole"));
    assert!(yaml.contains("s3:PutObject"));
    assert!(yaml.contains("arn:${AWS::Partition}:s3:::${Files}/*"));
}
