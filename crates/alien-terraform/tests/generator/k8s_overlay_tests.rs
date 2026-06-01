//! Kubernetes identity overlay — EKS / GKE / AKS targets.
//!
//! EKS overlays add a `kubernetes_service_account` next to the AWS
//! emitter's `aws_iam_role`, with the `eks.amazonaws.com/role-arn`
//! annotation. AKS overlays add the same kind of overlay on top of
//! `azurerm_user_assigned_identity` — the federated identity
//! credential trusts the AKS cluster's OIDC issuer and the K8s SA
//! carries `azure.workload.identity/client-id`. Both modules pass
//! `terraform fmt -check` + `terraform validate` against the cloud providers.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    AzureResourceGroup, Container, ContainerCode, Ingress, KubernetesCertificateMode,
    KubernetesCluster, KubernetesClusterOwnership, KubernetesClusterProvider,
    KubernetesExposureSettings, KubernetesHeartbeatMode, KubernetesIngressRouteProfile,
    KubernetesRouteProfile, KubernetesSettings, ManagementPermissions, Network, NetworkSettings,
    PermissionProfile, PermissionsConfig, RemoteStackManagement, ResourceLifecycle, ResourceSpec,
    ServiceAccount, Stack, StackSettings, Storage, Worker, WorkerCode,
};
use alien_terraform::{
    generate_terraform_module, TerraformHelmInstall, TerraformOptions, TerraformRegistration,
    TerraformTarget, TfRegistry,
};

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
                assert!(main.contains("command = \"aws\""));
                assert!(main.contains("eks\", \"get-token\""));
                assert!(main.contains("routeApi         = \"ingress\""));
                assert!(main.contains("controller       = \"eks.amazonaws.com/alb\""));
                assert!(main.contains("ingressClassName = \"alb\""));
                assert!(main.contains("provider   = \"awsAlb\""));
                assert!(main.contains("mode = \"none\""));
                assert!(!main.contains("clusterBootstrap"));
                assert!(!main.contains("metricsServer"));
                assert!(!main.contains("eksAutoMode"));
                assert!(cluster.contains(
                    r#""kubernetes.io/cluster/${local.resource_prefix}-k8s" = "shared""#
                ));
                assert!(
                    !cluster.contains(r#""kubernetes.io/cluster/$${local.resource_prefix}-k8s""#)
                );
            }
            TerraformTarget::Gke => {
                assert!(main.contains("routeApi         = \"gateway\""));
                assert!(main.contains("gatewayClassName = \"gke-l7-global-external-managed\""));
                assert!(main.contains("listenerPort     = 80"));
                assert!(main.contains("provider          = \"gkeGateway\""));
                assert!(main.contains("mode = \"none\""));
                assert!(main.contains("gke-gcloud-auth-plugin"));
                assert!(main.contains("provideClusterInfo = true"));
                assert!(cluster.contains(r#"data "google_client_config" "current""#));
                assert!(!main.contains("client-certificate-data"));
                assert!(!main.contains("client-key-data"));
            }
            TerraformTarget::Aks => {
                assert!(main.contains("routeApi         = \"gateway\""));
                assert!(main.contains("gatewayClassName = \"azure-alb-external\""));
                assert!(main.contains("listenerPort     = 80"));
                assert!(main.contains(
                    "\"alb.networking.azure.io/alb-namespace\" = var.kubernetes_namespace"
                ));
                assert!(main.contains(
                    "\"alb.networking.azure.io/alb-name\"      = \"${local.resource_prefix}-alb\""
                ));
                assert!(main.contains("provider     = \"azureApplicationGatewayForContainers\""));
                assert!(main.contains("albNamespace = var.kubernetes_namespace"));
                assert!(main.contains("albName      = \"${local.resource_prefix}-alb\""));
                assert!(main.contains("mode = \"none\""));
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
    for (target, provider) in [
        (TerraformTarget::Eks, KubernetesClusterProvider::Eks),
        (TerraformTarget::Aks, KubernetesClusterProvider::Aks),
    ] {
        let stack_builder = Stack::new(format!("{}-public-endpoint", target.name()))
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
            );
        let stack = if target == TerraformTarget::Aks {
            stack_builder
                .add(
                    Network::new("default-network".to_string())
                        .settings(NetworkSettings::Create {
                            cidr: None,
                            availability_zones: 2,
                        })
                        .build(),
                    ResourceLifecycle::Frozen,
                )
                .build()
        } else {
            stack_builder.build()
        };

        let module = render(&stack, target, StackSettings::default());
        let cluster = module
            .get("kubernetes.tf")
            .expect("cluster resource file should render");
        if target == TerraformTarget::Eks {
            let locals = module.get("locals.tf").expect("locals should render");
            assert!(locals.contains(r#"controller       = "eks.amazonaws.com/alb""#));
            assert!(!locals.contains("eksAutoMode"));
            assert!(!locals.contains("arm64NodePool"));
            assert!(!cluster.contains("IngressClassParams"));
            assert!(!cluster.contains(r#"resource "kubernetes_manifest" "kubernetes_alb"#));
        }
        if target == TerraformTarget::Aks {
            assert!(cluster.contains(
                r#"resource "azapi_resource_action" "kubernetes_service_networking_provider_registration""#
            ));
            assert!(cluster.contains(
                r#""/subscriptions/${var.azure_subscription_id}/providers/Microsoft.ServiceNetworking""#
            ));
            assert!(cluster.contains(r#"action      = "register""#));
            assert!(
                cluster.contains(r#"resource "azapi_update_resource" "kubernetes_alb_controller""#)
            );
            assert!(cluster.contains(
                "azapi_resource_action.kubernetes_service_networking_provider_registration"
            ));
            assert!(cluster.contains("node_count     = 2"));
            assert!(cluster.contains(
                r#"resource "azurerm_role_assignment" "kubernetes_current_client_kubernetes_rbac_cluster_admin""#
            ));
            assert!(cluster.contains(r#""Azure Kubernetes Service RBAC Cluster Admin""#));
            assert!(cluster.contains("tenant_id          = var.azure_tenant_id"));
            assert!(cluster.contains(r#"applicationLoadBalancer = {"#));
            assert!(cluster.contains(r#"installation = "Standard""#));
            assert!(cluster
                .contains(r#"data "azurerm_user_assigned_identity" "kubernetes_alb_controller""#));
            assert!(cluster.contains("applicationloadbalancer-${local.kubernetes_cluster_name}"));
            assert!(cluster.contains(
                r#"resource "azurerm_role_assignment" "kubernetes_alb_controller_association_subnet_network_contributor""#
            ));
            assert!(cluster.contains("azurerm_subnet.default_network_alb.id"));
            assert!(cluster.contains(r#""Network Contributor""#));
            let locals = module.get("locals.tf").expect("locals should render");
            assert!(locals.contains(
                r#"nonsensitive(try(yamldecode(azurerm_kubernetes_cluster.kubernetes[0].kube_config_raw)["current-context"]"#
            ));
            assert!(locals.contains("azureApplicationGatewayForContainers"));
            assert!(locals.contains("associationSubnetId = azurerm_subnet.default_network_alb.id"));
        }
        assert_terraform_valid(&module, &format!("{}_public_endpoint", target.name()));
    }
}

#[test]
fn aks_managed_cluster_sizes_node_pool_for_kubernetes_workload_requests() {
    fn app_container(id: &str, cpu: &str, memory: &str) -> Container {
        Container::new(id.to_string())
            .code(ContainerCode::Image {
                image: format!("example.com/{id}:latest"),
            })
            .cpu(ResourceSpec {
                min: cpu.to_string(),
                desired: cpu.to_string(),
            })
            .memory(ResourceSpec {
                min: memory.to_string(),
                desired: memory.to_string(),
            })
            .permissions("app".to_string())
            .build()
    }

    let stack_builder = Stack::new("aks-full-stack-capacity".to_string())
        .permission("app", PermissionProfile::new())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Aks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("default".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        );

    let stack = [
        ("postgres", "0.5", "512Mi"),
        ("redis", "250m", "256Mi"),
        ("api", "0.5", "512Mi"),
        ("worker", "0.25", "512Mi"),
        ("scheduler", "0.25", "256Mi"),
        ("dashboard", "0.25", "256Mi"),
        ("gateway", "0.25", "128Mi"),
    ]
    .into_iter()
    .fold(stack_builder, |builder, (id, cpu, memory)| {
        builder.add(app_container(id, cpu, memory), ResourceLifecycle::Live)
    })
    .build();

    let module = render(&stack, TerraformTarget::Aks, StackSettings::default());
    let cluster = module
        .get("kubernetes.tf")
        .expect("cluster resource file should render");

    assert!(cluster.contains("vm_size    = \"Standard_D4s_v3\""));
    assert!(
        cluster.contains("node_count = 1"),
        "AKS create-mode should fit the full-stack workload within a 4-vCPU node pool"
    );
    assert_terraform_valid(&module, "aks_full_stack_capacity");
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
    let locals = module.get("locals.tf").expect("locals should render");
    let management_config_line = locals
        .lines()
        .find(|line| line.contains("deployment_management_config"))
        .expect("locals should include deployment_management_config");
    assert!(management_config_line.ends_with("= null"));
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
fn managed_kubernetes_cluster_emitters_do_not_apply_in_cluster_manifests() {
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

        let variables = module.get("variables.tf").expect("variables should render");
        assert!(!variables.contains(r#"variable "install_metrics_server""#));

        let cluster = module
            .get("kubernetes.tf")
            .expect("cluster resource file should render");
        assert!(!cluster.contains("metrics-server"));
        assert!(!cluster.contains("IngressClassParams"));
        assert!(!cluster.contains("kind       = \"StorageClass\""));
        assert!(!cluster.contains(r#"resource "kubernetes_manifest""#));

        let locals = module.get("locals.tf").expect("locals should render");
        assert!(!locals.contains("metricsServer"));
        assert!(!locals.contains("registry.k8s.io/metrics-server/metrics-server:v0.8.1"));
        assert!(!locals.contains("arm64NodePool"));

        assert_terraform_valid(
            &module,
            &format!(
                "{}_managed_cluster_without_in_cluster_manifests",
                target.name()
            ),
        );
    }
}

#[test]
fn registered_kubernetes_module_installs_provider_rendered_helm_values() {
    let stack = Stack::new("eks-manager-fetch-values".to_string())
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
    let registry = TfRegistry::built_in();
    let module = generate_terraform_module(
        &stack,
        TerraformTarget::Eks,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: Some(TerraformRegistration {
                provider_name: "alien".to_string(),
                provider_source: "pkg.example.com/acme/app".to_string(),
                provider_version: "1.0.0".to_string(),
                resource_type: "deployment".to_string(),
                release_id: Some("rel-test".to_string()),
                setup_target: "kubernetes".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
            }),
            helm_install: Some(TerraformHelmInstall {
                chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
                release_name: "acme-agent".to_string(),
            }),
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("module should render");

    let locals = module
        .get("locals.tf")
        .expect("registered module should include locals");
    assert!(!locals.contains("helm_values"));

    if let Some(outputs) = module.get("outputs.tf") {
        assert!(!outputs.contains("helm_values"));
    }

    let helm = module
        .get("helm.tf")
        .expect("registered module with helm install should include helm.tf");
    assert!(helm.contains("alien_deployment.this.helm_values"));
    assert!(!helm.contains("local.helm_values"));

    let import = module
        .get("import.tf")
        .expect("registered module should include import.tf");
    assert!(import.contains("release_id"));
    assert!(import.contains("\"rel-test\""));

    let providers = module
        .get("providers.tf")
        .expect("registered Kubernetes module should include providers.tf");
    assert!(providers.contains("kubernetes = {"));
    assert!(!providers.contains("kubernetes {"));
}

#[test]
fn registered_gke_kubernetes_module_declares_dynamic_network_inputs() {
    let stack = Stack::new("gke-manager-fetch-values".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Gke)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("alien".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Network::new("default-network".to_string())
                .settings(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = TfRegistry::built_in();
    let module = generate_terraform_module(
        &stack,
        TerraformTarget::Gke,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: Some(TerraformRegistration {
                provider_name: "alien".to_string(),
                provider_source: "pkg.example.com/acme/app".to_string(),
                provider_version: "1.0.0".to_string(),
                resource_type: "deployment".to_string(),
                release_id: Some("rel-test".to_string()),
                setup_target: "kubernetes".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
            }),
            helm_install: Some(TerraformHelmInstall {
                chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
                release_name: "acme-agent".to_string(),
            }),
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("module should render");

    let variables = module
        .get("variables.tf")
        .expect("registered GKE module should include variables");
    for variable in [
        "network_mode",
        "network_cidr",
        "availability_zones",
        "network_name",
        "subnet_name",
        "network_region",
    ] {
        assert!(
            variables.contains(&format!("variable \"{variable}\"")),
            "variables.tf should declare {variable}"
        );
    }

    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();
    assert!(rendered.contains("var.network_mode"));
    assert!(rendered.contains("var.network_name"));
    assert!(rendered.contains("var.subnet_name"));
    assert!(rendered.contains("var.network_region"));
    assert!(!rendered.contains("${local.resource_prefix}-default_network"));
    assert!(rendered.contains("${local.resource_prefix}-default-network"));
    assert!(rendered.contains("gke-gcloud-auth-plugin"));
    assert!(rendered.contains(r#"data "google_client_config" "current""#));
    assert!(rendered.contains("data.google_client_config.current.access_token"));
    assert!(!rendered.contains("client-certificate-data"));
    assert!(!rendered.contains("client-key-data"));
}
