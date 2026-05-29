//! Kubernetes identity overlay — EKS / GKE / AKS targets.
//!
//! EKS overlays add a `kubernetes_service_account` next to the AWS
//! emitter's `aws_iam_role`, with the `eks.amazonaws.com/role-arn`
//! annotation. AKS overlays add the same kind of overlay on top of
//! `azurerm_user_assigned_identity` — the federated identity
//! credential trusts the AKS cluster's OIDC issuer and the K8s SA
//! carries `azure.workload.identity/client-id`. Both modules pass
//! `terraform fmt -check` + `terraform validate` against the cloud +
//! Kubernetes providers.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    AzureResourceGroup, Ingress, KubernetesCertificateMode, KubernetesCluster,
    KubernetesClusterOwnership, KubernetesClusterProvider, KubernetesExposureSettings,
    KubernetesHeartbeatMode, KubernetesIngressRouteProfile, KubernetesRouteProfile,
    KubernetesSettings, ResourceLifecycle, ServiceAccount, Stack, StackSettings, Storage, Worker,
    WorkerCode,
};
use alien_terraform::TerraformTarget;

fn storage_data_read_service_account() -> ServiceAccount {
    ServiceAccount::new("execution-sa".to_string())
        .stack_permission_set(
            alien_permissions::get_permission_set("storage/data-read")
                .expect("storage/data-read permission set")
                .clone(),
        )
        .build()
}

#[test]
fn eks_overlay_emits_irsa_service_account_annotation() {
    let stack = Stack::new("eks-overlay".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            storage_data_read_service_account(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Eks, StackSettings::default());
    snapshot_module("eks_overlay_irsa", &module);
    assert_terraform_valid(&module, "eks_overlay_irsa");
}

#[test]
fn gke_overlay_emits_workload_identity_binding() {
    let stack = Stack::new("gke-overlay".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            storage_data_read_service_account(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gke, StackSettings::default());
    snapshot_module("gke_overlay_workload_identity", &module);
    assert_terraform_valid(&module, "gke_overlay_workload_identity");
}

#[test]
fn aks_overlay_emits_workload_identity_federated_credential() {
    let stack = Stack::new("aks-overlay".to_string())
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            storage_data_read_service_account(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aks, StackSettings::default());
    snapshot_module("aks_overlay_workload_identity", &module);
    assert_terraform_valid(&module, "aks_overlay_workload_identity");
}

#[test]
fn eks_overlay_leaves_live_workloads_to_helm() {
    let storage = Storage::new("data".to_string()).build();
    let function = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "example.com/api:latest".to_string(),
        })
        .permissions("execution-sa".to_string())
        .ingress(Ingress::Public)
        .link(&storage)
        .build();
    let stack = Stack::new("eks-live-workload".to_string())
        .add(storage, ResourceLifecycle::Frozen)
        .add(
            storage_data_read_service_account(),
            ResourceLifecycle::Frozen,
        )
        .add(function, ResourceLifecycle::Live)
        .build();

    let module = render(&stack, TerraformTarget::Eks, StackSettings::default());
    snapshot_module("eks_overlay_live_workload_helm_handoff", &module);
    assert_terraform_valid(&module, "eks_overlay_live_workload_helm_handoff");
}

#[test]
fn managed_kubernetes_cluster_emitters_export_runtime_metadata() {
    for (target, provider, expected) in [
        (
            TerraformTarget::Eks,
            KubernetesClusterProvider::Eks,
            "aws_eks_cluster",
        ),
        (
            TerraformTarget::Gke,
            KubernetesClusterProvider::Gke,
            "google_container_cluster",
        ),
        (
            TerraformTarget::Aks,
            KubernetesClusterProvider::Aks,
            "azurerm_kubernetes_cluster",
        ),
    ] {
        let stack = Stack::new(format!("{}-managed-cluster", target.name()))
            .add(
                KubernetesCluster::new("kubernetes".to_string())
                    .provider(provider)
                    .ownership(KubernetesClusterOwnership::Managed)
                    .namespace("default".to_string())
                    .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let module = render(&stack, target, StackSettings::default());
        let cluster = module
            .get("kubernetes.tf")
            .expect("cluster resource file should render");
        assert!(cluster.contains(expected));
        assert!(cluster.contains("kubernetes_cluster_mode"));

        let outputs = module.get("outputs.tf").expect("outputs should render");
        assert!(outputs.contains("kubernetes_kubeconfig"));
        assert!(!outputs.contains("kubernetes_ingress_class"));
        assert!(!outputs.contains("kubernetes_ingress_annotations"));
        assert!(!outputs.contains("kubernetes_public_host_suffix"));

        let main = module.get("locals.tf").expect("locals should render");
        assert!(main.contains("kubernetes_exposure"));
        assert!(main.contains(
            "exposure = coalesce(try(jsondecode(var.stack_settings_json).kubernetes.exposure, null), local.kubernetes_exposure)"
        ));
        match target {
            TerraformTarget::Eks => {
                assert!(main.contains("routeApi         = \"ingress\""));
                assert!(main.contains("controller       = \"eks.amazonaws.com/alb\""));
                assert!(main.contains("ingressClassName = \"alb\""));
                assert!(main.contains("provider   = \"awsAlb\""));
                assert!(main.contains("mode   = \"managedAcmImport\""));
            }
            TerraformTarget::Gke => {
                assert!(main.contains("routeApi         = \"gateway\""));
                assert!(main.contains("gatewayClassName = \"gke-l7-global-external-managed\""));
                assert!(main.contains("provider          = \"gkeGateway\""));
                assert!(main.contains("mode               = \"managedTlsSecret\""));
            }
            TerraformTarget::Aks => {
                assert!(main.contains("routeApi         = \"gateway\""));
                assert!(main.contains("gatewayClassName = \"azure-alb-external\""));
                assert!(main.contains("provider = \"azureApplicationGatewayForContainers\""));
                assert!(main.contains("mode               = \"managedTlsSecret\""));
            }
            _ => unreachable!("only managed Kubernetes targets are tested here"),
        }

        assert_terraform_valid(
            &module,
            &format!("{}_managed_kubernetes_cluster", target.name()),
        );
    }
}

#[test]
fn managed_kubernetes_cluster_preserves_stack_settings_exposure() {
    let stack = Stack::new("eks-custom-exposure".to_string())
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
    let settings = StackSettings {
        kubernetes: Some(KubernetesSettings {
            cluster: None,
            exposure: Some(KubernetesExposureSettings::Custom {
                domain: "api.example.com".to_string(),
                route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
                    controller: Some("eks.amazonaws.com/alb".to_string()),
                    ingress_class_name: "alb".to_string(),
                    labels: Default::default(),
                    annotations: Default::default(),
                    provider: None,
                }),
                certificate: KubernetesCertificateMode::AwsAcmArn {
                    certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/customer"
                        .to_string(),
                },
            }),
        }),
        ..StackSettings::default()
    };

    let module = render(&stack, TerraformTarget::Eks, settings);
    let locals = module.get("locals.tf").expect("locals should render");
    let variables = module.get("variables.tf").expect("variables should render");

    assert!(variables.contains("api.example.com"));
    assert!(variables.contains("certificateArn"));
    assert!(variables.contains("arn:aws:acm:us-east-1:123456789012:certificate/customer"));
    assert!(locals.contains(
        "exposure = coalesce(try(jsondecode(var.stack_settings_json).kubernetes.exposure, null), local.kubernetes_exposure)"
    ));
    assert_terraform_valid(&module, "eks_custom_exposure");
}

#[test]
fn managed_kubernetes_cluster_emitters_install_metrics_server_for_created_clusters() {
    for (target, provider) in [
        (TerraformTarget::Eks, KubernetesClusterProvider::Eks),
        (TerraformTarget::Gke, KubernetesClusterProvider::Gke),
        (TerraformTarget::Aks, KubernetesClusterProvider::Aks),
    ] {
        let stack = Stack::new(format!("{}-metrics-server", target.name()))
            .add(
                KubernetesCluster::new("kubernetes".to_string())
                    .provider(provider)
                    .ownership(KubernetesClusterOwnership::Managed)
                    .namespace("default".to_string())
                    .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let module = render(&stack, target, StackSettings::default());
        snapshot_module(
            &format!("{}_managed_cluster_metrics_server", target.name()),
            &module,
        );

        let variables = module.get("variables.tf").expect("variables should render");
        assert!(variables.contains(r#"variable "install_metrics_server""#));
        assert!(variables.contains("default     = true"));

        let providers = module.get("providers.tf").expect("providers should render");
        assert!(providers.contains(r#"provider "kubernetes""#));

        let cluster = module
            .get("kubernetes.tf")
            .expect("cluster resource file should render");
        assert!(cluster.contains("metrics-server"));
        assert!(cluster.contains(
            r#"count = var.kubernetes_cluster_mode == "create" && var.install_metrics_server ? 1 : 0"#
        ));

        assert_terraform_valid(&module, &format!("{}_metrics_server", target.name()));
    }
}
