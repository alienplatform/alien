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
    KubernetesSettings, ManagementPermissions, PermissionProfile, PermissionsConfig,
    RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack, StackSettings, Storage,
    Worker, WorkerCode,
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
fn eks_storage_profile_permissions_attach_to_irsa_role() {
    let stack = Stack::new("eks-storage-permissions".to_string())
        .permissions(PermissionsConfig::new().with_profile(
            "app",
            PermissionProfile::new().resource("files", ["storage/data-read", "storage/data-write"]),
        ))
        .add(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("app-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let module = render(&stack, TerraformTarget::Eks, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents.as_ref())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("resource \"aws_iam_role_policy\" \"files_app_sa_storage-data-write_1\"")
    );
    assert!(rendered.contains("\"s3:PutObject\""));
    assert!(rendered.contains("arn:aws:s3:::${aws_s3_bucket.files.bucket}/*"));
    assert_terraform_valid(&module, "eks_storage_profile_permissions");
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
            "exposure = jsondecode(try(jsondecode(var.stack_settings_json).kubernetes.exposure, null) == null ? jsonencode(local.kubernetes_exposure) : jsonencode(jsondecode(var.stack_settings_json).kubernetes.exposure))"
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
fn managed_kubernetes_clusters_install_generated_public_endpoint_support() {
    for (target, provider, expected) in [
        (
            TerraformTarget::Eks,
            KubernetesClusterProvider::Eks,
            r#"kind       = "IngressClassParams""#,
        ),
        (
            TerraformTarget::Aks,
            KubernetesClusterProvider::Aks,
            r#"resource "azapi_update_resource" "kubernetes_alb_controller""#,
        ),
    ] {
        let stack = Stack::new(format!("{}-public-endpoint", target.name()))
            .add(
                KubernetesCluster::new("kubernetes".to_string())
                    .provider(provider)
                    .ownership(KubernetesClusterOwnership::Managed)
                    .namespace("default".to_string())
                    .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Worker::new("api".to_string())
                    .code(WorkerCode::Image {
                        image: "example.com/api:latest".to_string(),
                    })
                    .permissions("execution".to_string())
                    .ingress(Ingress::Public)
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();

        let module = render(&stack, target, StackSettings::default());
        let cluster = module
            .get("kubernetes.tf")
            .expect("cluster resource file should render");
        assert!(cluster.contains(expected));
        if target == TerraformTarget::Eks {
            assert!(cluster.contains(r#"controller = "eks.amazonaws.com/alb""#));
        }
        if target == TerraformTarget::Aks {
            assert!(cluster.contains(r#"applicationLoadBalancer = {"#));
            assert!(cluster.contains(r#"installation = "Standard""#));
        }
        assert_terraform_valid(&module, &format!("{}_public_endpoint", target.name()));
    }
}

#[test]
fn eks_managed_cluster_with_remote_management_irsa_is_valid() {
    let stack = Stack::new("eks-managed-remote-management".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource("kubernetes", ["kubernetes-cluster/heartbeat"]),
        ))
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("default".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("remote-stack-management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let settings = StackSettings {
        deployment_model: alien_core::DeploymentModel::Pull,
        ..StackSettings::default()
    };

    let module = render(&stack, TerraformTarget::Eks, settings);
    snapshot_module("eks_managed_cluster_remote_management_irsa", &module);
    assert_terraform_valid(&module, "eks_managed_cluster_remote_management_irsa");
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
        "exposure = jsondecode(try(jsondecode(var.stack_settings_json).kubernetes.exposure, null) == null ? jsonencode(local.kubernetes_exposure) : jsonencode(jsondecode(var.stack_settings_json).kubernetes.exposure))"
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
