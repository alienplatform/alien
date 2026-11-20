//! Kubernetes identity overlay — EKS / GKE / AKS targets.
//!
//! EKS overlays add a `kubernetes_service_account` next to the AWS
//! emitter's `aws_iam_role`, with the `eks.amazonaws.com/role-arn`
//! annotation. AKS overlays add the same kind of overlay on top of
//! `azurerm_user_assigned_identity` — the federated identity
//! credential trusts the AKS cluster's OIDC issuer and the K8s SA
//! carries `azure.workload.identity/client-id`. Both modules pass
//! `terraform fmt -check` + `terraform validate` against the cloud providers.

use super::helpers::{
    assert_terraform_formatted, assert_terraform_valid,
    assert_terraform_variable_plan_invalid_contains, gate_input, prepare_for_target, render,
    resource_attributes, snapshot_module,
};
use alien_core::{
    ArtifactRegistry, AzureResourceGroup, AzureStorageAccount, CapacityGroup, ComputeCluster,
    ComputePoolSelection, ComputeSettings, Container, ContainerCode, KubernetesCertificateMode,
    KubernetesCluster, KubernetesClusterOwnership, KubernetesClusterProvider,
    KubernetesExposureSettings, KubernetesHeartbeatMode, KubernetesIngressRouteProfile,
    KubernetesRouteProfile, KubernetesSettings, ManagementPermissions, Network, NetworkSettings,
    PermissionProfile, PermissionsConfig, Postgres, RemoteStackManagement, ResourceLifecycle,
    ResourceSpec, Sandbox, ServiceAccount, Stack, StackSettings, Storage, Vault, Worker,
    WorkerCode,
};
use alien_terraform::{
    generate_product_terraform_module, generate_terraform_module, TerraformHelmInstall,
    TerraformOptions, TerraformRegistration, TerraformTarget, TfEmitter, TfFragment, TfRegistry,
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
fn kubernetes_compute_pools_do_not_emit_cloud_fleets_or_choices() {
    let baseline = Stack::new("portable".to_string()).build();
    let stack = Stack::new("portable".to_string())
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "apps".to_string(),
                    instance_type: None,
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                    scale_policy: None,
                    nested_virtualization: None,
                    data_disk_gib_range: None,
                })
                .dynamic_container_pool("apps".to_string())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let settings = StackSettings {
        compute: Some(ComputeSettings {
            containers: Default::default(),
            pools: [(
                "apps".to_string(),
                ComputePoolSelection::Fixed {
                    machines: 2,
                    machine: Some("provider-machine".to_string()),
                    failure_domains: None,
                    data_disk_gib: None,
                },
            )]
            .into_iter()
            .collect(),
        }),
        ..Default::default()
    };
    // Compare the complete install artifact, including registration and advanced
    // settings. No cloud emitter exists for this resource in the built-in registry:
    // accidentally dispatching it to a cloud backend fails before comparison.
    for target in [
        TerraformTarget::Eks,
        TerraformTarget::Gke,
        TerraformTarget::Aks,
    ] {
        let expected = render(&baseline, target, StackSettings::default());
        let actual = render(&stack, target, settings.clone());
        assert_eq!(
            actual.files, expected.files,
            "logical pools changed {target:?} infrastructure"
        );
    }
}

#[test]
fn eks_overlay_use_default_network_emits_az_filtered_default_vpc_subnets() {
    // EKS deployments rendered with `network = Some(UseDefault)` must build
    // `subnet_ids` from the account's default VPC. This regression test
    // exercises the active branch the prior snapshot tests don't:
    //
    //   - aws_vpcs.<label>_default filtered by is-default
    //   - aws_availability_zones.<label>_eks_supported {
    //       exclude_zone_ids = var.unsupported_availability_zone_ids
    //     }
    //   - aws_subnets.<label>_default filtered by VPC id, default-for-az and AZ id
    //   - subnet_ids ternary that routes through the data source when
    //     network_mode == "use-default".
    //
    // Before the fix the ternary fell through to `[]` and AWS rejected the
    // apply with "Attribute vpc_config.0.subnet_ids requires 1 item minimum"
    // (ALIEN-202).
    let stack = Stack::new("eks-use-default".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(NetworkSettings::UseDefault)
                .build(),
            ResourceLifecycle::Frozen,
        )
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
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            storage_data_read_service_account(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Eks, StackSettings::default());

    // Concatenate the whole module so we can search across files.
    let rendered = module
        .iter()
        .map(|(_, contents)| contents.as_ref())
        .collect::<Vec<_>>()
        .join("\n");

    // The default VPC + AZ-filtered subnet data sources must be present.
    assert!(
        rendered.contains("data \"aws_vpcs\" \"default_network_default\""),
        "expected `data aws_vpcs default_network_default` block, got:\n{}",
        rendered
    );
    assert!(
        rendered.contains("data \"aws_subnets\" \"default_network_default\""),
        "expected `data aws_subnets default_network_default` block"
    );
    assert!(
        rendered.contains("exclude_zone_ids = var.unsupported_availability_zone_ids"),
        "expected exclude_zone_ids wired to the user-overridable variable"
    );
    // The EKS subnet_ids ternary must route through the default data source
    // for use-default. Looking for the specific terminal branch is enough.
    assert!(
        rendered.contains("data.aws_subnets.default_network_default.ids"),
        "expected EKS subnet_ids to reference the default-VPC subnet data source"
    );
    // The variable itself must be declared with AZ IDs (not names) as the
    // default — `us-east-1e`'s zone id is `use1-az3`.
    assert!(
        rendered.contains("variable \"unsupported_availability_zone_ids\""),
        "expected unsupported_availability_zone_ids variable declared"
    );
    assert!(
        rendered.contains("\"use1-az3\""),
        "expected `use1-az3` (us-east-1e's stable zone id) in the default list"
    );

    snapshot_module("eks_overlay_use_default", &module);
    assert_terraform_valid(&module, "eks_overlay_use_default");
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

/// An install from an earlier release still records the cloud deployer vault, so an upgraded
/// module keeps its IAM resources at the same addresses: removing them is a separate change.
#[test]
fn upgraded_gke_and_eks_modules_keep_the_deployer_vault_iam_and_registration() {
    let deployer_vault_addresses = |target: TerraformTarget| {
        let stack = Stack::new("deployer-vault-upgrade".to_string())
            .permission(
                "execution",
                PermissionProfile::new().resource("deployer-secrets", ["vault/data-read"]),
            )
            .add(
                ServiceAccount::new("execution-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Vault::new("deployer-secrets".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let module = render(&stack, target, StackSettings::default());
        let rendered = module.files.values().cloned().collect::<String>();
        let addresses: Vec<String> = rendered
            .lines()
            .filter(|line| {
                line.starts_with("resource \"")
                    && line.contains("secret")
                    && line.contains("execution_sa")
            })
            .map(|line| line.trim_end_matches(" {").to_string())
            .collect();
        (addresses, module)
    };

    // The cluster targets address the vault's IAM exactly as the cloud targets, which an
    // earlier release's module used.
    for (kubernetes, cloud) in [
        (TerraformTarget::Gke, TerraformTarget::Gcp),
        (TerraformTarget::Eks, TerraformTarget::Aws),
    ] {
        let (addresses, module) = deployer_vault_addresses(kubernetes);
        let (cloud_addresses, _) = deployer_vault_addresses(cloud);
        assert!(
            !addresses.is_empty(),
            "{kubernetes:?} no longer emits the deployer vault's IAM"
        );
        assert_eq!(addresses, cloud_addresses, "{kubernetes:?} addresses moved");
        assert!(
            module
                .get("locals.tf")
                .unwrap()
                .contains("deployer-secrets"),
            "{kubernetes:?} still registers the vault"
        );
    }
}

#[test]
fn gke_management_service_account_has_one_workload_identity_role_resource() {
    let stack = Stack::new("gke-management".to_string())
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gke, StackSettings::default());

    // The binding is authoritative for the role, so any other resource granting the same role
    // on the same service account is overwritten by it on every apply.
    let bindings = resource_attributes(&module, "google_service_account_iam_binding");
    let members = resource_attributes(&module, "google_service_account_iam_member");
    let mut grants_by_service_account: std::collections::BTreeMap<(String, String), usize> =
        Default::default();
    for (_, attributes) in bindings.iter().chain(members.iter()) {
        *grants_by_service_account
            .entry((
                attributes["service_account_id"].clone(),
                attributes["role"].clone(),
            ))
            .or_default() += 1;
    }
    for ((service_account, role), count) in &grants_by_service_account {
        assert_eq!(
            *count, 1,
            "{service_account} has {count} resources granting {role}"
        );
    }

    let (label, binding) = bindings
        .iter()
        .find(|(_, attributes)| attributes["role"].contains("roles/iam.workloadIdentityUser"))
        .expect("the management service account has a Workload Identity binding");
    assert!(
        label.ends_with("_workload_identity"),
        "the binding keeps its address: {label}"
    );
    assert!(
        binding["members"].contains("workload_identity_config")
            && binding["members"].contains("principal://iam.googleapis.com/"),
        "the binding lists the Kubernetes service account and the federated principal: {}",
        binding["members"]
    );
    assert!(
        members.is_empty(),
        "no separate member resource for the same role"
    );

    // Upgrade: the old member address is forgotten, not destroyed.
    let rendered = module.files.values().cloned().collect::<String>();
    assert!(
        rendered.contains("google_service_account_iam_member."),
        "removed block names the old member address"
    );
    assert!(rendered.contains("destroy = false"));
    assert_terraform_valid(&module, "gke_management_workload_identity");
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
        .public_endpoint(alien_core::WorkerPublicEndpoint {
            name: "api".to_string(),
            host_label: None,
            wildcard_subdomains: false,
        })
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
            "exposure = jsondecode(try(local.advanced_settings.kubernetes.exposure, null) == null ? jsonencode(local.kubernetes_exposure) : jsonencode(local.advanced_settings.kubernetes.exposure))"
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
                assert!(main.contains("listenerPort     = 443"));
                assert!(main.contains("provider          = \"gkeGateway\""));
                assert!(main.contains("mode               = \"managedTlsSecret\""));
                assert!(main.contains("secretNameTemplate = \"{{ workloadName }}-tls\""));
                assert!(!main.contains("mode = \"none\""));
                assert!(main.contains("gke-gcloud-auth-plugin"));
                assert!(main.contains("provideClusterInfo = true"));
                assert!(cluster.contains(r#"data "google_client_config" "current""#));
                assert!(!main.contains("client-certificate-data"));
                assert!(!main.contains("client-key-data"));
            }
            TerraformTarget::Aks => {
                assert!(main.contains("routeApi         = \"gateway\""));
                assert!(main.contains("gatewayClassName = \"azure-alb-external\""));
                assert!(main.contains("listenerPort     = 443"));
                assert!(main.contains("mode               = \"managedTlsSecret\""));
                assert!(main.contains("secretNameTemplate = \"{{ workloadName }}-tls\""));
                assert!(main.contains(
                    "\"alb.networking.azure.io/alb-namespace\" = var.kubernetes_namespace"
                ));
                assert!(main.contains(
                    "\"alb.networking.azure.io/alb-name\"      = \"${local.resource_prefix}-alb\""
                ));
                assert!(main.contains("provider     = \"azureApplicationGatewayForContainers\""));
                assert!(main.contains("albNamespace = var.kubernetes_namespace"));
                assert!(main.contains("albName      = \"${local.resource_prefix}-alb\""));
                assert!(!main.contains("mode = \"none\""));
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
                    .public_endpoint(alien_core::WorkerPublicEndpoint {
                        name: "api".to_string(),
                        host_label: None,
                        wildcard_subdomains: false,
                    })
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
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let settings = StackSettings {
        deployment_model: alien_core::DeploymentModel::Pull,
        ..StackSettings::default()
    };

    let module = render(&stack, TerraformTarget::Eks, settings);
    let locals = module.get("locals.tf").expect("locals should render");
    assert!(
        locals.contains("deployment_management_config"),
        "locals should include deployment_management_config"
    );
    assert!(
        locals.contains(r#"var.deployment_model == "push""#),
        "management config should follow the typed deployment_model input"
    );
    assert!(
        locals.contains("} : null"),
        "pull-mode deployments should register without push management config"
    );
    snapshot_module("eks_managed_cluster_remote_management_irsa", &module);
    assert_terraform_valid(&module, "eks_managed_cluster_remote_management_irsa");
}

/// The module decides the cluster source from `kubernetes_cluster_mode`. A caller that sends no
/// `kubernetes` advanced settings (the portal sends none) must still register the ownership the
/// setup policy needs.
#[test]
fn kubernetes_module_sends_cluster_ownership_from_the_cluster_mode() {
    for (target, provider) in [
        (TerraformTarget::Eks, KubernetesClusterProvider::Eks),
        (TerraformTarget::Gke, KubernetesClusterProvider::Gke),
        (TerraformTarget::Aks, KubernetesClusterProvider::Aks),
    ] {
        let stack = Stack::new("cluster-ownership".to_string())
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
        let locals = module.get("locals.tf").expect("locals should render");
        assert!(
            locals.contains(
                r#"cluster = merge(try(local.advanced_settings.kubernetes.cluster, {}), { ownership = var.kubernetes_cluster_mode == "create" ? "managed" : "existing" })"#
            ),
            "{target:?} must derive kubernetes.cluster.ownership from kubernetes_cluster_mode:\n{locals}"
        );
        assert_terraform_valid(&module, &format!("{target:?} cluster ownership"));
    }
}

#[test]
fn eks_created_network_with_irsa_and_remote_management_is_valid() {
    let stack = Stack::new("eks-created-network-identity".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource("kubernetes", ["kubernetes-cluster/heartbeat"]),
        ))
        .add(
            Network::new("default-network".to_string())
                .settings(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
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
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
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
    assert_terraform_valid(&module, "eks_created_network_identity");
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

    assert!(variables.contains(r#"variable "custom_domain_name""#));
    assert!(variables.contains(r#"variable "custom_domain_certificate_arn""#));
    assert!(variables.contains("api.example.com"));
    assert!(variables.contains("arn:aws:acm:us-east-1:123456789012:certificate/customer"));
    assert!(locals.contains("generated_kubernetes_exposure"));
    assert!(locals.contains("custom_domain_name"));
    assert!(locals.contains("certificateArn = var.custom_domain_certificate_arn"));
    assert!(locals.contains(
        "exposure = jsondecode(try(local.advanced_settings.kubernetes.exposure, null) == null ? jsonencode(local.kubernetes_exposure) : jsonencode(local.advanced_settings.kubernetes.exposure))"
    ));
    assert_terraform_valid(&module, "eks_custom_exposure");
}

#[test]
fn eks_custom_domain_requires_acm_certificate_arn() {
    let stack = Stack::new("eks-custom-domain-without-cert".to_string())
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
                certificate: KubernetesCertificateMode::None,
            }),
        }),
        ..StackSettings::default()
    };

    let module = render(&stack, TerraformTarget::Eks, settings);
    assert_terraform_variable_plan_invalid_contains(
        &module,
        "eks_custom_domain_requires_acm_certificate_arn",
        &[
            ("name", "example"),
            ("token", "example-token"),
            ("custom_domain_name", "api.example.com"),
        ],
        "custom_domain_certificate_arn must be a valid AWS ACM certificate ARN",
    );
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

/// Terraform destroys the guard before the Helm release and the registration,
/// so the Operator still runs when retained data stops the destroy.
#[test]
fn registered_kubernetes_module_destroys_the_retained_data_guard_first() {
    let stack = Stack::new("eks-destroy-guard".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("production".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = TfRegistry::built_in();
    let options = |helm_install| TerraformOptions {
        setup_identity: None,
        pins: alien_core::SetupPins::compiled_in(),
        display_name: None,
        registry: &registry,
        stack_settings: StackSettings::default(),
        registration: Some(TerraformRegistration {
            provider_name: "acme_app".to_string(),
            provider_source: "pkg.example.com/acme/app".to_string(),
            provider_version: "1.0.0".to_string(),
            resource_type: "deployment".to_string(),
            release_id: Some("rel-test".to_string()),
            setup_target: "kubernetes".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            destroy_guard: true,
        }),
        helm_install,
        supported_aws_regions: Vec::new(),
    };
    let helm_install = || {
        Some(TerraformHelmInstall {
            chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
            release_name: "acme-operator".to_string(),
        })
    };
    let product = generate_product_terraform_module(
        &stack,
        TerraformTarget::Eks,
        options(helm_install()),
        false,
    )
    .expect("product module should render");
    let plain = generate_terraform_module(&stack, TerraformTarget::Eks, options(helm_install()))
        .expect("plain module should render");
    let without_helm = generate_terraform_module(&stack, TerraformTarget::Eks, options(None))
        .expect("module without Helm should render");

    for (mode, module) in [("product", &product), ("plain", &plain)] {
        let helm = module.get("helm.tf").expect("helm.tf");
        let compact = helm.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            compact.contains("resource \"acme_app_destroy_guard\" \"this\" {"),
            "{mode}: {helm}"
        );
        assert!(
            compact.contains("depends_on = [ acme_app_deployment.this, helm_release.runtime,"),
            "{mode}: {helm}"
        );
        assert!(compact.contains("deployment_id = acme_app_deployment.this.deployment_id"));
        assert!(compact.contains("management_url = var.management_url"));
        let readme = module.get("README.md").expect("README.md");
        assert!(
            readme.contains(
                "wait until its status is `teardown-required`, `retention-required` or `deleted`"
            ),
            "{mode} README names the erase-first order"
        );
        assert!(
            readme.contains("needs one `terraform apply` with this module"),
            "{mode} README names the upgrade step"
        );
    }
    // Without a Helm install there is no Operator to order before: no guard.
    assert!(without_helm
        .iter()
        .all(|(_, contents)| !contents.contains("destroy_guard")));
}

/// Resource blocks of a rendered module by address, each as its exact text.
fn resource_blocks(
    module: &alien_terraform::ModuleFiles,
) -> std::collections::BTreeMap<String, String> {
    let mut blocks = std::collections::BTreeMap::new();
    for (file, contents) in module.iter() {
        if !file.ends_with(".tf") {
            continue;
        }
        let lines: Vec<&str> = contents.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            if let Some(rest) = line.strip_prefix("resource \"") {
                let parts: Vec<&str> = rest.split('"').collect();
                let address = format!("{}.{}", parts[0], parts[2]);
                let mut end = i;
                while lines[end] != "}" {
                    end += 1;
                }
                let text = lines[i..=end].join("\n");
                assert!(
                    blocks.insert(address.clone(), text).is_none(),
                    "{address} defined twice"
                );
                i = end;
            }
            i += 1;
        }
    }
    blocks
}

/// A module made before the guard existed (`destroy_guard` false) and the same
/// module with it differ only by the guard, so the first plan of an upgrade
/// adds one resource and destroys or replaces nothing.
#[test]
fn destroy_guard_is_the_only_change_between_old_and_new_registered_modules() {
    let stack = Stack::new("eks-destroy-guard-upgrade".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("production".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = TfRegistry::built_in();
    let render = |destroy_guard: bool, product: bool| {
        let options = TerraformOptions {
            setup_identity: None,
            pins: alien_core::SetupPins::compiled_in(),
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: Some(TerraformRegistration {
                provider_name: "acme_app".to_string(),
                provider_source: "pkg.example.com/acme/app".to_string(),
                provider_version: "1.0.0".to_string(),
                resource_type: "deployment".to_string(),
                release_id: Some("rel-test".to_string()),
                setup_target: "kubernetes".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                destroy_guard,
            }),
            helm_install: Some(TerraformHelmInstall {
                chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
                release_name: "acme-operator".to_string(),
            }),
            supported_aws_regions: Vec::new(),
        };
        if product {
            generate_product_terraform_module(&stack, TerraformTarget::Eks, options, false)
        } else {
            generate_terraform_module(&stack, TerraformTarget::Eks, options)
        }
        .expect("module should render")
    };

    for product in [true, false] {
        let old = render(false, product);
        let new = render(true, product);
        let old_blocks = resource_blocks(&old);
        let new_blocks = resource_blocks(&new);

        assert!(
            old_blocks
                .keys()
                .all(|address| new_blocks.contains_key(address)),
            "every old resource address stays"
        );
        let added: Vec<&String> = new_blocks
            .keys()
            .filter(|address| !old_blocks.contains_key(*address))
            .collect();
        assert_eq!(added, vec!["acme_app_destroy_guard.this"]);
        for (address, text) in &old_blocks {
            assert_eq!(&new_blocks[address], text, "{address} is byte-identical");
        }
        for (file, contents) in new.iter() {
            for forbidden in ["moved {", "replace_triggered_by"] {
                assert_eq!(
                    contents.matches(forbidden).count(),
                    old.get(file)
                        .map_or(0, |old_contents| old_contents.matches(forbidden).count()),
                    "{file} gains no `{forbidden}`"
                );
            }
        }
        // Outside the guard block and the README, nothing differs.
        for (file, contents) in old.iter() {
            if file == "README.md" {
                continue;
            }
            let new_contents = new.get(file).expect("file stays");
            let without_guard = new_contents.replace(
                &format!("{}\n", new_blocks["acme_app_destroy_guard.this"]),
                "",
            );
            let normalize = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
            assert_eq!(
                normalize(&without_guard),
                normalize(contents),
                "{file} differs only by the guard"
            );
        }
        // Without the flag, neither the module nor its README mention a guard.
        assert!(old
            .iter()
            .all(|(_, contents)| !contents.contains("destroy_guard")
                && !contents.contains("teardown-required")));
    }
}

#[test]
fn registered_kubernetes_module_installs_provider_rendered_helm_values() {
    let stack = Stack::new("eks-registered-setup-values".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("production".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = TfRegistry::built_in();
    let module = generate_product_terraform_module(
        &stack,
        TerraformTarget::Eks,
        TerraformOptions {
            setup_identity: None,
            pins: alien_core::SetupPins::compiled_in(),
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: Some(TerraformRegistration {
                provider_name: "acme_app".to_string(),
                provider_source: "pkg.example.com/acme/app".to_string(),
                provider_version: "1.0.0".to_string(),
                resource_type: "deployment".to_string(),
                release_id: Some("rel-test".to_string()),
                setup_target: "kubernetes".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                destroy_guard: false,
            }),
            helm_install: Some(TerraformHelmInstall {
                chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
                release_name: "acme-operator".to_string(),
            }),
            supported_aws_regions: Vec::new(),
        },
        false,
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
    assert!(helm.contains("acme_app_deployment.this.helm_values"));
    // The `helm_values` output carries the merged values the release installs with.
    assert!(helm.contains("output \"helm_values\""));
    let compact_values = helm.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(compact_values
        .contains("chart_values_provider_yaml = acme_app_deployment.this.helm_values"));
    assert!(compact_values
        .contains("chart_values_provider = yamldecode(local.chart_values_provider_yaml)"));
    assert_eq!(
        helm.matches("acme_app_deployment.this.helm_values").count(),
        1
    );
    assert!(compact_values.contains("sensitive = true"));
    assert!(!helm.contains("local.helm_values"));
    assert!(helm.contains("resource \"kubernetes_secret_v1\" \"remote_operator_credentials\""));
    assert!(helm.contains("var.remote_operator_sync_token"));
    assert!(helm.contains("var.remote_operator_encryption_key"));
    assert!(helm.contains("encryptionKeySha256 = var.remote_operator_encryption_key != null ? sha256(var.remote_operator_encryption_key)"));
    assert!(helm.contains("var.remote_operator_collector_token"));
    assert!(helm.contains("\"collector-token\""));
    let compact_helm = helm.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        compact_helm.contains("ignore_changes = [ data[\"encryption-key\"] ]"),
        "{helm}"
    );
    assert!(helm.contains("bootstrapIdentity = var.remote_operator_bootstrap_identity"));
    assert!(helm.contains("syncTokenRevision = var.remote_operator_sync_token_revision"));
    assert!(helm.contains(
        "collectorTokenRevision = var.remote_operator_collector_token != null ? sha256(var.remote_operator_collector_token) : \"\""
    ));
    assert!(helm.contains(
        "podLabels = var.remote_operator_enabled ? { \"alien.dev/credentials-secret-uid\" = kubernetes_secret_v1.remote_operator_credentials[0].metadata[0].uid } : {}"
    ));
    assert!(!helm.contains("yamldecode(acme_app_deployment.this.helm_values).management.token"));
    assert!(helm.contains("remoteOperator = {"));
    assert!(helm.contains("enabled = var.remote_operator_enabled"));
    assert!(helm.contains("data \"kubernetes_resources\" \"remote_operator_namespace\""));
    assert!(helm.contains("data \"kubernetes_resources\" \"remote_operator_identity_records\""));
    assert!(!helm.contains("remote_operator_terraform_owners"));
    assert!(compact_helm
        .contains("kind = local.remote_operator_namespace_exists ? \"ConfigMap\" : \"Namespace\""));
    assert!(compact_helm.contains(
        "namespace = local.remote_operator_namespace_exists ? local.remote_operator_lifecycle_namespace : null"
    ));
    assert!(helm.contains(
        "local.remote_operator_namespace_exists ? \"metadata.name=${local.remote_operator_identity_record_name}\" : \"metadata.name=${local.remote_operator_lifecycle_namespace}\""
    ));
    assert!(helm.contains("remote_operator_release_prefix"));
    assert!(helm.contains("remote_operator_identity_record_name"));
    assert!(helm.contains("resource \"terraform_data\" \"remote_operator_ownership\""));
    assert!(compact_helm.contains("ignore_changes = [ input ]"));
    assert!(helm.contains("remote_operator_pinned_ownership"));
    assert!(helm.contains("substr(sha256("));
    assert!(!helm.contains("label_selector"));
    assert!(helm.contains("count = var.kubernetes_namespace_create ? 1 : 0"));
    assert!(
        helm.contains("count = var.helm_install_enabled && var.remote_operator_enabled ? 1 : 0")
    );
    assert!(helm.contains("resource \"terraform_data\" \"remote_operator_lifecycle_guard\""));
    assert!(helm.contains(
        "local.remote_operator_pinned_ownership.managed ? (local.remote_operator_requested_managed"
    ));
    assert!(!helm.contains("remote_operator_terraform_owner"));
    assert!(helm.contains("terraform apply -replace=terraform_data.remote_operator_ownership"));
    assert!(helm.contains("kubernetes_secret_v1.remote_operator_credentials"));
    assert!(compact_helm.contains("atomic = true"));
    assert!(!compact_helm.contains("cleanup_on_fail"));
    assert!(compact_helm.contains("wait = true"));
    assert!(compact_helm.contains("timeout = 900"));
    let variables = module
        .get("variables.tf")
        .expect("registered module should include variables");
    assert!(variables.contains("variable \"remote_operator_sync_token\""));
    // Terraform before 1.12 evaluates both sides of `||`, so a null token must not reach
    // `trimspace` through `x == null || ...`.
    for token in [
        "remote_operator_sync_token",
        "remote_operator_collector_token",
    ] {
        assert!(
            variables.contains(&format!(
                "var.{token} == null ? true : length(trimspace(var.{token})) > 0"
            )),
            "{token} validation must not rely on short-circuit"
        );
    }
    assert!(!variables.contains("== null || length("));
    assert!(variables.contains("variable \"remote_operator_encryption_key\""));
    assert!(variables.contains("variable \"remote_operator_collector_token\""));
    assert!(variables.contains("variable \"remote_operator_enabled\""));
    assert!(variables.contains("variable \"remote_operator_bootstrap_identity\""));
    assert!(variables.contains("variable \"remote_operator_sync_token_revision\""));
    assert!(variables.contains(
        "var.remote_operator_sync_token_revision >= 0 && floor(var.remote_operator_sync_token_revision) == var.remote_operator_sync_token_revision"
    ));
    assert!(variables.contains("Never reuse the product deployment token"));
    assert!(variables.contains("default     = false"));
    let readme = module
        .get("README.md")
        .expect("registered module should document lifecycle behavior");
    assert!(readme.contains("identity PVC"));
    assert!(readme.contains("permanently retires the setup"));
    assert!(readme.contains("disabling it in place is unsupported"));
    assert!(readme.contains("separately managed Helm release"));
    assert!(readme.contains("terraform output -raw helm_values > values.yaml"));
    assert!(readme.contains("Supply `remote_operator_collector_token` only when"));
    assert!(readme.contains("Keep the credentials required by the chart populated"));
    assert!(readme.contains("terraform apply -replace=terraform_data.remote_operator_ownership"));
    let sync_token_variable = variables
        .split("variable \"remote_operator_sync_token\"")
        .nth(1)
        .and_then(|rest| rest.split("\nvariable ").next())
        .expect("sync token variable block");
    assert!(sync_token_variable
        .split_whitespace()
        .collect::<String>()
        .contains("default=null"));

    let registration = module
        .get("registration.tf")
        .expect("registered module should include registration.tf");
    assert!(registration.contains("release_id"));
    assert!(registration.contains("\"rel-test\""));

    let providers = module
        .get("providers.tf")
        .expect("registered Kubernetes module should include providers.tf");
    assert!(providers.contains("kubernetes = {"));
    assert!(providers.contains("provider \"kubernetes\""));
    assert!(
        providers.contains("host                   = data.aws_eks_cluster.target.endpoint"),
        "{providers}"
    );
}

#[test]
fn registered_gke_kubernetes_module_declares_dynamic_network_inputs() {
    let stack = Stack::new("gke-registered-setup-values".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Gke)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("production".to_string())
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
    let module = generate_product_terraform_module(
        &stack,
        TerraformTarget::Gke,
        TerraformOptions {
            setup_identity: None,
            pins: alien_core::SetupPins::compiled_in(),
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: Some(TerraformRegistration {
                provider_name: "acme_app".to_string(),
                provider_source: "pkg.example.com/acme/app".to_string(),
                provider_version: "1.0.0".to_string(),
                resource_type: "deployment".to_string(),
                release_id: Some("rel-test".to_string()),
                setup_target: "kubernetes".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                destroy_guard: false,
            }),
            helm_install: Some(TerraformHelmInstall {
                chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
                release_name: "acme-operator".to_string(),
            }),
            supported_aws_regions: Vec::new(),
        },
        false,
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

#[test]
fn eks_declared_network_is_shared_with_cluster_in_every_mode() {
    for network in [
        NetworkSettings::UseDefault,
        NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        },
        NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-0123456789abcdef0".to_string(),
            public_subnet_ids: vec!["subnet-public".to_string()],
            private_subnet_ids: vec!["subnet-private".to_string()],
            security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
        },
    ] {
        let stack = Stack::new("shared-network".to_string())
            .add(
                Network::new("default-network".to_string())
                    .settings(network)
                    .build(),
                ResourceLifecycle::Frozen,
            )
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
        let module = render(&stack, TerraformTarget::Eks, StackSettings::default());
        let rendered = module
            .iter()
            .map(|(_, v)| v.as_ref())
            .collect::<Vec<&str>>()
            .join("\n");
        assert!(!rendered.contains("resource \"aws_vpc\" \"kubernetes\""));
        assert!(!rendered.contains("resource \"aws_subnet\" \"kubernetes_private\""));
        assert!(rendered.contains("data.aws_subnets.kubernetes_private_selected[0].ids"));
        assert!(rendered.contains("data.aws_subnets.kubernetes_public_selected[0].ids"));
        // Provider validation checks every generated reference against the actual topology.
        assert_terraform_valid(&module, "eks_shared_network");
    }
}

#[test]
fn aks_brought_network_uses_data_subnet_for_node_pool() {
    let stack = Stack::new("aks-byo".to_string())
        .add(Network::new("default-network".to_string()).settings(NetworkSettings::ByoVnetAzure {
            vnet_resource_id: "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/shared/providers/Microsoft.Network/virtualNetworks/shared".to_string(),
            public_subnet_name: "public".to_string(), private_subnet_name: "private".to_string(),
            application_gateway_subnet_name: None, private_endpoint_subnet_name: None,
        }).build(), ResourceLifecycle::Frozen)
        .add(KubernetesCluster::new("kubernetes".to_string()).provider(KubernetesClusterProvider::Aks)
            .ownership(KubernetesClusterOwnership::Managed).namespace("default".to_string()).heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata).build(), ResourceLifecycle::Frozen).build();
    let module = render(&stack, TerraformTarget::Aks, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, v)| v.as_ref())
        .collect::<Vec<&str>>()
        .join("\n");
    assert!(rendered.contains("vnet_subnet_id = data.azurerm_subnet.default_network_private.id"));
    assert_terraform_valid(&module, "aks_byo_shared_network");
}

#[test]
fn the_charts_infrastructure_output_names_binding_types_and_skips_a_declined_store() {
    let stack = Stack::new("chart-gated".to_string())
        .inputs(vec![gate_input(
            "extraStore",
            "Extra store",
            "Create the extra store.",
        )])
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
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Storage::new("extra".to_string()).build(),
            ResourceLifecycle::Frozen,
            "extraStore",
        )
        .add(
            ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Eks, StackSettings::default());
    let outputs = module.get("outputs.tf").expect("outputs.tf");
    let compact = outputs.split_whitespace().collect::<Vec<_>>().join(" ");

    // The chart's binding type is snake_case.
    assert!(
        compact.contains("type = \"artifact_registry\""),
        "{outputs}"
    );
    assert!(!compact.contains("artifact-registry\""), "{outputs}");
    // The declined store is left out while its gate is false, and never read without an index.
    assert!(compact.contains("merge("), "{outputs}");
    assert!(compact.contains("var.input_extra_store ?"), "{outputs}");
    assert!(
        !compact.contains("aws_s3_bucket.extra.bucket"),
        "a gated bucket needs its count index: {outputs}"
    );
    assert_terraform_valid(&module, "eks_chart_gated_bindings");
}

#[test]
fn eks_module_outputs_the_charts_infrastructure_values() {
    let stack = Stack::new("chart-bindings".to_string())
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
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("artifacts".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Eks, StackSettings::default());
    let outputs = module.get("outputs.tf").expect("outputs.tf");
    let compact = outputs.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(
        compact.contains("output \"chart_infrastructure_json\""),
        "{outputs}"
    );
    // One entry per Storage, each with the chart's resource type and the
    // bucket this module creates.
    for id in ["data", "artifacts"] {
        assert!(
            compact.contains(&format!("{id} = {{")) || compact.contains(&format!("\"{id}\" = {{")),
            "{id}: {outputs}"
        );
        assert!(
            compact.contains(&format!("bucketName = aws_s3_bucket.{id}.bucket")),
            "{id}: {outputs}"
        );
    }
    assert!(compact.contains("type = \"storage\""), "{outputs}");
    assert!(compact.contains("service = \"s3\""), "{outputs}");
    // The identity annotations the chart's service accounts need come from the module too.
    assert!(
        compact.contains("output \"chart_service_accounts_json\""),
        "{outputs}"
    );
    assert!(
        compact.contains("serviceAccountPrefix = local.resource_prefix"),
        "{outputs}"
    );
    assert!(compact.contains("local.helm_service_accounts"), "{outputs}");
    assert!(
        compact.contains("local.helm_manager_service_account"),
        "{outputs}"
    );
}

fn eks_stack_with_databases() -> Stack {
    Stack::new("with-databases".to_string())
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
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::Postgres::new("db".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::Redis::new("cache".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

/// A stack with a Postgres or Redis resource renders on EKS: the module leaves them out and
/// says the installer supplies their bindings. Before this, rendering failed with
/// IMPORT_REGISTRATION_MISSING.
#[test]
fn eks_module_leaves_out_resources_the_installer_binds() {
    let module = render(
        &eks_stack_with_databases(),
        TerraformTarget::Eks,
        StackSettings::default(),
    );
    let all = whole_module(&module);
    assert!(
        !all.contains("aws_rds") && !all.contains("aws_rds_cluster"),
        "{all}"
    );
    let outputs = module.get("outputs.tf").expect("outputs.tf");
    let compact = outputs.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        compact.contains("output \"installer_supplied_bindings\""),
        "{outputs}"
    );
    assert!(compact.contains("db = \"postgres\""), "{outputs}");
    assert!(compact.contains("cache = \"redis\""), "{outputs}");
    let readme = module.get("README.md").expect("README.md");
    assert!(readme.contains("## Bindings you supply"), "{readme}");
    assert!(readme.contains("`db` (postgres)") && readme.contains("`cache` (redis)"));
    // What the module does create is still bound as before.
    assert!(
        compact.contains("output \"chart_infrastructure_json\""),
        "{outputs}"
    );
    assert_terraform_valid(&module, "eks_installer_bindings");
}

/// An emitter an embedder registers for the type always wins over the skip.
#[test]
fn a_registered_emitter_still_renders_its_resource() {
    use alien_terraform::{TfEmitter, TfFragment};

    struct Marker;
    impl TfEmitter for Marker {
        fn emit(
            &self,
            _ctx: &alien_core::import::EmitContext<'_>,
        ) -> alien_core::Result<TfFragment> {
            Ok(TfFragment::default())
        }
        fn emit_import_ref(
            &self,
            _ctx: &alien_core::import::EmitContext<'_>,
        ) -> alien_core::Result<hcl::Expression> {
            Ok(hcl::Expression::String("registered-postgres".to_string()))
        }
    }

    let mut registry = TfRegistry::built_in();
    registry.register("postgres", alien_core::Platform::Aws, Marker);
    let module = super::helpers::render_with_registry(
        &eks_stack_with_databases(),
        TerraformTarget::Eks,
        StackSettings::default(),
        &registry,
    )
    .expect("renders");
    let outputs = module.get("outputs.tf").expect("outputs.tf");
    let compact = outputs.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(compact.contains("cache = \"redis\""), "{outputs}");
    assert!(!compact.contains("db = \"postgres\""), "{outputs}");
    assert!(
        whole_module(&module).contains("registered-postgres"),
        "the registered emitter's import data is in the registration"
    );
}

fn whole_module(module: &alien_terraform::ModuleFiles) -> String {
    module
        .iter()
        .map(|(_, contents)| contents.as_ref())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A new EKS cluster starts on Kubernetes 1.36 (Nitro sandboxes need it on Auto
/// Mode), and an existing cluster is never upgraded by the module: the version
/// is ignored after creation, so `terraform plan` shows no change for it.
#[test]
fn a_new_eks_cluster_starts_on_1_36_and_an_existing_one_is_not_upgraded() {
    let stack = Stack::new("eks-version".to_string())
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
    let module = render(&stack, TerraformTarget::Eks, StackSettings::default());
    let rendered = whole_module(&module);
    let cluster = rendered
        .split("resource \"aws_eks_cluster\"")
        .nth(1)
        .expect("the module creates an EKS cluster");
    let cluster = cluster.split("\nresource ").next().unwrap();
    let compact = cluster.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(compact.contains("version = \"1.36\""), "{cluster}");
    assert!(
        compact
            .replace(' ', "")
            .contains("lifecycle{ignore_changes=[version]}"),
        "{cluster}"
    );
}

/// EKS Auto Mode ignores NetworkPolicy until the network policy controller is on (checked on
/// a real 1.36 Auto Mode cluster: a deny-all policy let traffic through). A stack with a
/// sandbox needs enforcement; a stack without one keeps the addon exactly as before, so an
/// existing install's plan does not change.
#[test]
fn eks_turns_on_network_policy_enforcement_only_when_the_stack_has_a_sandbox() {
    use alien_core::{SandboxCode, SandboxEgress, SandboxLifecyclePolicy};

    let cluster = || {
        KubernetesCluster::new("kubernetes".to_string())
            .provider(KubernetesClusterProvider::Eks)
            .ownership(KubernetesClusterOwnership::Managed)
            .namespace("default".to_string())
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
            .build()
    };
    let vpc_cni = |stack: &Stack| {
        let module = render(stack, TerraformTarget::Eks, StackSettings::default());
        let rendered = whole_module(&module);
        let block = rendered
            .split("resource \"aws_eks_addon\" \"kubernetes_vpc_cni\"")
            .nth(1)
            .expect("vpc-cni addon")
            .split("\nresource ")
            .next()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        block
    };

    let plain = Stack::new("eks-plain".to_string())
        .add(cluster(), ResourceLifecycle::Frozen)
        .build();
    assert!(!vpc_cni(&plain).contains("configuration_values"));

    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "registry.example.com/agent:latest".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    let with_sandbox = Stack::new("eks-sandbox".to_string())
        .add(cluster(), ResourceLifecycle::Frozen)
        .add(sandbox, ResourceLifecycle::Live)
        .build();
    let block = vpc_cni(&with_sandbox);
    assert!(block.contains("enableNetworkPolicy = \"true\""), "{block}");
    // Enforcement only: `strict` would deny every Pod that has no policy, the chart's own
    // Pods and hook Jobs included.
    assert!(!block.contains("NETWORK_POLICY_ENFORCING_MODE"), "{block}");
    assert_terraform_valid(
        &render(
            &with_sandbox,
            TerraformTarget::Eks,
            StackSettings::default(),
        ),
        "eks_network_policy_enforcement",
    );
}

/// Azure fills in these node pool upgrade settings when a pool does not set
/// them, so the generated pools state them. Otherwise every plan after the
/// create shows the settings being removed.
#[test]
fn aks_node_pools_state_the_azure_default_upgrade_settings() {
    use alien_core::{SandboxCode, SandboxEgress, SandboxLifecyclePolicy};

    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "registry.example.com/agent:latest".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    let stack = Stack::new("aks-upgrade".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Aks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("default".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(sandbox, ResourceLifecycle::Live)
        .build();
    let module = render(&stack, TerraformTarget::Aks, StackSettings::default());
    let rendered = whole_module(&module);
    let compact = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let settings = "upgrade_settings { drain_timeout_in_minutes = 0 max_surge = \"10%\" node_soak_duration_in_minutes = 0 }";

    let cluster = rendered
        .split("resource \"azurerm_kubernetes_cluster\"")
        .nth(1)
        .expect("cluster resource");
    let cluster = cluster.split("\nresource ").next().unwrap();
    let pool_start = cluster.find("default_node_pool {").expect("default pool");
    let default_pool = &cluster[pool_start..];
    let default_pool = &default_pool[..default_pool.find("\n  }\n").unwrap_or(default_pool.len())];
    assert!(compact(default_pool).contains(settings), "{default_pool}");

    let kata = rendered
        .split("resource \"azurerm_kubernetes_cluster_node_pool\"")
        .nth(1)
        .expect("kata pool");
    let kata = kata.split("\nresource ").next().unwrap();
    assert!(compact(kata).contains(settings), "{kata}");
}

#[test]
fn aks_adds_a_kata_node_pool_only_when_the_stack_has_a_sandbox() {
    use alien_core::{SandboxCode, SandboxEgress, SandboxLifecyclePolicy};

    let plain = Stack::new("aks-plain".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Aks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("default".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let rendered = whole_module(&render(
        &plain,
        TerraformTarget::Aks,
        StackSettings::default(),
    ));
    assert!(!rendered.contains("KataVmIsolation"), "{rendered}");
    assert!(!rendered.contains("azurerm_kubernetes_cluster_node_pool"));

    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "registry.example.com/agent:latest".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    let with_sandbox = Stack::new("aks-sandbox".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Aks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("default".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(sandbox, ResourceLifecycle::Live)
        .build();
    let module = render(
        &with_sandbox,
        TerraformTarget::Aks,
        StackSettings::default(),
    );
    let rendered = whole_module(&module);
    let pool = rendered
        .split("resource \"azurerm_kubernetes_cluster_node_pool\"")
        .nth(1)
        .expect("a sandbox adds a node pool");
    let pool = pool.split("\nresource ").next().unwrap();
    let compact = pool.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        compact.contains("workload_runtime = \"KataVmIsolation\""),
        "{pool}"
    );
    assert!(compact.contains("os_sku = \"AzureLinux\""), "{pool}");
    assert!(compact.contains("vm_size = \"Standard_D4s_v3\""), "{pool}");
    // Only a cluster this module creates gets the pool.
    assert!(
        compact.contains("count = var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
        "{pool}"
    );
    assert_terraform_valid(&module, "aks_kata_node_pool");
}

fn app_workload() -> Container {
    Container::new("app".to_string())
        .code(ContainerCode::Image {
            image: "example.com/app:latest".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.5".to_string(),
            desired: "0.5".to_string(),
        })
        .memory(ResourceSpec {
            min: "512Mi".to_string(),
            desired: "512Mi".to_string(),
        })
        .permissions("app".to_string())
        .build()
}

fn declared_cluster(ownership: KubernetesClusterOwnership) -> KubernetesCluster {
    KubernetesCluster::new("kubernetes".to_string())
        .provider(KubernetesClusterProvider::Eks)
        .ownership(ownership)
        .namespace("default".to_string())
        .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
        .build()
}

/// What an embedder registers for a database or cache: a resource setup creates. The module's
/// content for it does not matter to these tests, only that setup creates it (a type with no
/// emitter is left to the installer to bind).
struct EmbedderDatabase;

impl TfEmitter for EmbedderDatabase {
    fn emit(&self, _ctx: &alien_core::import::EmitContext<'_>) -> alien_core::Result<TfFragment> {
        Ok(TfFragment::default())
    }

    fn emit_import_ref(
        &self,
        _ctx: &alien_core::import::EmitContext<'_>,
    ) -> alien_core::Result<hcl::expr::Expression> {
        Ok(hcl::expr::Expression::Object(Default::default()))
    }
}

fn registry_with_database_emitters() -> TfRegistry {
    let mut registry = TfRegistry::built_in();
    for platform in [
        alien_core::Platform::Aws,
        alien_core::Platform::Gcp,
        alien_core::Platform::Azure,
    ] {
        registry.register(Postgres::RESOURCE_TYPE, platform, EmbedderDatabase);
        registry.register(alien_core::Redis::RESOURCE_TYPE, platform, EmbedderDatabase);
    }
    registry
}

/// What the source stack adds to the one workload every case here deploys.
#[derive(Clone, Copy)]
enum Source {
    Workload,
    Database,
    /// A cloud database and nothing that runs on the cluster, so no cluster is declared.
    DatabaseOnly,
    /// A Redis instance next to the workload.
    Redis,
    /// Postgres plus a Redis instance the deployer can decline (`redisEnabled`).
    DatabaseWithGatedRedis,
    GatedDatabase,
}

/// An EKS module for `source`, rendered from the stack the preflights prepare for it, the way
/// `alien render` does: the network and cluster are the ones the mutations add.
async fn render_prepared_eks(source: Source) -> alien_terraform::ModuleFiles {
    render_prepared(source, TerraformTarget::Eks).await
}

/// [`render_prepared_eks`] for any Kubernetes target.
async fn render_prepared(source: Source, target: TerraformTarget) -> alien_terraform::ModuleFiles {
    render_prepared_in(source, target, &TfRegistry::built_in()).await
}

/// [`render_prepared`] with a registry that has emitters for the databases and caches, as an
/// embedder's does: setup creates them rather than leaving them to the installer.
async fn render_prepared_with_emitters(
    source: Source,
    target: TerraformTarget,
) -> alien_terraform::ModuleFiles {
    render_prepared_in(source, target, &registry_with_database_emitters()).await
}

async fn render_prepared_in(
    source: Source,
    target: TerraformTarget,
    registry: &TfRegistry,
) -> alien_terraform::ModuleFiles {
    let stack = prepared_stack(source, target).await;
    super::helpers::render_with_registry(&stack, target, StackSettings::default(), registry)
        .expect("module should render")
}

/// The source stack for `source`, prepared the way the preflights prepare it for `target`.
async fn prepared_stack(source: Source, target: TerraformTarget) -> Stack {
    let mut builder =
        Stack::new("k8s-network-gate".to_string()).permission("app", PermissionProfile::new());
    if matches!(source, Source::GatedDatabase) {
        builder = builder.inputs(vec![gate_input(
            "databaseEnabled",
            "Enable the database",
            "Whether to create the database.",
        )]);
    }
    if matches!(source, Source::DatabaseWithGatedRedis) {
        builder = builder.inputs(vec![gate_input(
            "redisEnabled",
            "Enable Redis",
            "Whether to create the cache.",
        )]);
    }
    if !matches!(source, Source::DatabaseOnly) {
        builder = builder.add(app_workload(), ResourceLifecycle::Live);
    }
    builder = match source {
        Source::Workload => builder,
        Source::DatabaseWithGatedRedis => builder
            .add(
                Postgres::new("db".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_enabled_when(
                alien_core::Redis::new("cache".to_string()).build(),
                ResourceLifecycle::Frozen,
                "redisEnabled",
            ),
        Source::Redis => builder.add(
            alien_core::Redis::new("cache".to_string()).build(),
            ResourceLifecycle::Frozen,
        ),
        Source::Database | Source::DatabaseOnly => builder.add(
            Postgres::new("db".to_string()).build(),
            ResourceLifecycle::Frozen,
        ),
        Source::GatedDatabase => builder.add_enabled_when(
            Postgres::new("db".to_string()).build(),
            ResourceLifecycle::Frozen,
            "databaseEnabled",
        ),
    };
    prepare_for_target(builder.build(), target).await
}

/// A stack that declares a cluster and has a `default-network` in it: one the stack declares
/// itself, or one marked as the network mutation's. (Template validation rejects a stack that
/// declares these; a stack a manager prepares can carry them.)
fn render_declared_cluster(
    cluster: KubernetesClusterOwnership,
    network_added_by_preflight: Option<bool>,
) -> alien_terraform::ModuleFiles {
    let mut builder = Stack::new("eks-declared".to_string());
    if network_added_by_preflight.is_some() {
        builder = builder.add(
            Network::new("default-network".to_string())
                .settings(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        );
    }
    let mut stack = builder
        .add(declared_cluster(cluster), ResourceLifecycle::Frozen)
        .build();
    if network_added_by_preflight == Some(true) {
        let entry = stack
            .resources
            .get_mut("default-network")
            .expect("the network was added");
        entry.display_name = Some(alien_core::PREPARED_NETWORK_DISPLAY_NAME.to_string());
        entry.description = Some(alien_core::PREPARED_NETWORK_DESCRIPTION.to_string());
    }
    render(&stack, TerraformTarget::Eks, StackSettings::default())
}

fn compact(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

const CREATE_GATE: &str = "var.kubernetes_cluster_mode == \"create\"";
/// The network gate with no database: a created cluster, or a network the deployer requests in
/// the advanced settings.
const NETWORK_GATE: &str =
    "(var.kubernetes_cluster_mode == \"create\" || local.deployment_requests_network)";

/// An existing cluster is already attached to a network, and the manager prepares a stack with
/// none for it, so the module must neither create a VPC nor register `default-network` unless it
/// creates the cluster. It must also register the cluster as `existing`, not `managed`.
#[tokio::test]
async fn eks_creates_and_registers_a_default_network_only_with_a_created_cluster() {
    let module = render_prepared_eks(Source::Workload).await;
    let network = compact(
        module
            .get("default_network.tf")
            .expect("default_network.tf"),
    );
    assert!(
        network.contains(&format!(
            "count = {NETWORK_GATE} ? var.network_mode == \"create-new\" ? 1 : 0 : 0"
        )),
        "{network}"
    );
    let locals = compact(module.get("locals.tf").expect("locals.tf"));
    assert!(
        locals.contains(&format!(
            "deployment_resources = concat({NETWORK_GATE} ? [{{ id = \"default-network\""
        )),
        "{locals}"
    );
    assert!(
        locals.contains(&format!(
            "ownership = {CREATE_GATE} ? \"managed\" : \"existing\""
        )),
        "{locals}"
    );
    let settings = locals
        .split("deployment_settings")
        .nth(1)
        .expect("locals should define deployment_settings");
    assert!(
        settings.contains(&format!(
            "jsondecode({CREATE_GATE} ? jsonencode({{ network = jsondecode("
        )),
        "network must be registered only for a created cluster: {settings}"
    );
    assert_terraform_valid(&module, "eks_network_gated_on_cluster_mode");
}

/// The same rule holds for GKE and AKS: an existing cluster has its own network, so the module
/// neither creates nor registers `default-network` unless it creates the cluster.
#[tokio::test]
async fn gke_and_aks_create_and_register_a_default_network_only_with_a_created_cluster() {
    for target in [TerraformTarget::Gke, TerraformTarget::Aks] {
        let module = render_prepared(Source::Workload, target).await;
        let locals = compact(module.get("locals.tf").expect("locals.tf"));
        assert!(
            locals.contains(&format!(
                "deployment_resources = concat({NETWORK_GATE} ? [{{ id = \"default-network\""
            )),
            "{target:?}: {locals}"
        );
        let network = compact(
            module
                .get("default_network.tf")
                .expect("default_network.tf"),
        );
        assert!(network.contains(NETWORK_GATE), "{target:?}: {network}");
        assert_terraform_valid(
            &module,
            &format!("{target:?}_network_gated_on_cluster_mode"),
        );
    }
}

/// A stack with a cloud database keeps its network in every mode on every cloud.
#[tokio::test]
async fn gke_and_aks_keep_the_default_network_for_a_stack_with_a_database() {
    for target in [TerraformTarget::Gke, TerraformTarget::Aks] {
        let module = render_prepared(Source::Database, target).await;
        assert_network_in_every_mode(&module);
    }
}

/// The manager still gives a stack with a cloud database a network of its own for an existing
/// cluster (`kubernetes_requires_cloud_network`), so that network stays in every mode.
#[tokio::test]
async fn eks_keeps_the_default_network_for_a_stack_with_a_database() {
    let module = render_prepared_eks(Source::Database).await;
    assert_network_in_every_mode(&module);
}

/// The manager drops a declined frozen resource before it prepares the stack, so a database
/// behind `enabled_when` keeps the network only while its input is on.
#[tokio::test]
async fn eks_keeps_the_default_network_only_while_a_gated_database_is_accepted() {
    let module = render_prepared_eks(Source::GatedDatabase).await;
    let gate = format!(
        "({CREATE_GATE} || local.deployment_requests_network || var.input_database_enabled)"
    );
    let network = compact(
        module
            .get("default_network.tf")
            .expect("default_network.tf"),
    );
    assert!(
        network.contains(&format!(
            "count = {gate} ? var.network_mode == \"create-new\" ? 1 : 0 : 0"
        )),
        "{network}"
    );
    let locals = compact(module.get("locals.tf").expect("locals.tf"));
    assert!(
        locals.contains(&format!(
            "deployment_resources = concat({gate} ? [{{ id = \"default-network\""
        )),
        "{locals}"
    );
    assert_terraform_valid(&module, "eks_network_gated_on_database_input");
}

/// A network the stack declares is always expected, whatever the cluster mode.
#[test]
fn eks_keeps_a_default_network_the_stack_declares() {
    let module = render_declared_cluster(KubernetesClusterOwnership::Existing, Some(false));
    assert_network_in_every_mode(&module);
}

/// A cluster the stack declares as `managed` makes the manager expect the network, so it stays
/// in every mode; for a declared `existing` cluster the added network follows the cluster mode.
#[test]
fn eks_follows_the_ownership_of_a_cluster_the_stack_declares() {
    let managed = render_declared_cluster(KubernetesClusterOwnership::Managed, Some(true));
    assert_network_in_every_mode(&managed);

    let existing = render_declared_cluster(KubernetesClusterOwnership::Existing, Some(true));
    let locals = compact(existing.get("locals.tf").expect("locals.tf"));
    assert!(
        locals.contains(&format!(
            "deployment_resources = concat({NETWORK_GATE} ? [{{ id = \"default-network\""
        )),
        "{locals}"
    );
}

fn assert_network_in_every_mode(module: &alien_terraform::ModuleFiles) {
    let locals = compact(module.get("locals.tf").expect("locals.tf"));
    assert!(
        locals.contains("deployment_resources = [ { id = \"default-network\""),
        "{locals}"
    );
    let network = module
        .get("default_network.tf")
        .expect("default_network.tf");
    // Kept in every mode: no resource waits on the cluster-mode gate. (The lookups name the
    // cluster mode only in their preconditions.)
    assert!(
        !network.contains("deployment_requests_network"),
        "{network}"
    );
}

/// Addresses in the `depends_on` of one rendered resource block (parsed HCL).
fn guard_depends_on(block_text: &str) -> Vec<String> {
    let body = hcl::parse(block_text).expect("guard block parses");
    let block = body.blocks().next().expect("one resource block");
    let attribute = block
        .body()
        .attributes()
        .find(|attribute| attribute.key() == "depends_on")
        .expect("guard has depends_on");
    match attribute.expr() {
        hcl::Expression::Array(items) => items
            .iter()
            .map(|item| match item {
                hcl::Expression::Traversal(traversal) => {
                    let hcl::Expression::Variable(root) = &traversal.expr else {
                        panic!("unexpected depends_on root: {item:?}");
                    };
                    let mut address = root.to_string();
                    for operator in &traversal.operators {
                        let hcl::TraversalOperator::GetAttr(name) = operator else {
                            panic!("unexpected depends_on operator: {item:?}");
                        };
                        address = format!("{address}.{name}");
                    }
                    address
                }
                other => panic!("unexpected depends_on item: {other:?}"),
            })
            .collect(),
        other => panic!("depends_on is not a list: {other:?}"),
    }
}

/// The property that makes a refused `terraform destroy` remove nothing: the
/// destroy guard lists every other managed resource of the module (all .tf
/// files) in its `depends_on`, so Terraform destroys the guard first.
#[test]
fn destroy_guard_is_destroyed_before_every_other_resource() {
    let registry = TfRegistry::built_in();
    for (target, provider) in [
        (TerraformTarget::Eks, KubernetesClusterProvider::Eks),
        (TerraformTarget::Gke, KubernetesClusterProvider::Gke),
        (TerraformTarget::Aks, KubernetesClusterProvider::Aks),
    ] {
        for retained_data in [false, true] {
            for product in [false, true] {
                let mut stack = Stack::new("guard-first".to_string()).add(
                    KubernetesCluster::new("kubernetes".to_string())
                        .provider(provider.clone())
                        .ownership(KubernetesClusterOwnership::Managed)
                        .namespace("production".to_string())
                        .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                        .build(),
                    ResourceLifecycle::Frozen,
                );
                if retained_data {
                    if target == TerraformTarget::Aks {
                        stack = stack
                            .add(
                                AzureResourceGroup::new("default-resource-group".to_string())
                                    .build(),
                                ResourceLifecycle::Frozen,
                            )
                            .add(
                                alien_core::AzureStorageAccount::new(
                                    "default-storage-account".to_string(),
                                )
                                .build(),
                                ResourceLifecycle::Frozen,
                            );
                    }
                    stack = stack.add(
                        Storage::new("data".to_string()).build(),
                        ResourceLifecycle::Frozen,
                    );
                }
                let stack = stack.build();
                let options = TerraformOptions {
                    setup_identity: None,
                    pins: alien_core::SetupPins::compiled_in(),
                    display_name: None,
                    registry: &registry,
                    stack_settings: StackSettings::default(),
                    registration: Some(TerraformRegistration {
                        provider_name: "acme_app".to_string(),
                        provider_source: "pkg.example.com/acme/app".to_string(),
                        provider_version: "1.0.0".to_string(),
                        resource_type: "deployment".to_string(),
                        release_id: Some("rel-test".to_string()),
                        setup_target: "kubernetes".to_string(),
                        setup_fingerprint: "test".to_string(),
                        setup_fingerprint_version: 1,
                        destroy_guard: true,
                    }),
                    helm_install: Some(TerraformHelmInstall {
                        chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
                        release_name: "acme-operator".to_string(),
                    }),
                    supported_aws_regions: Vec::new(),
                };
                let module = if product {
                    generate_product_terraform_module(&stack, target, options, false)
                } else {
                    generate_terraform_module(&stack, target, options)
                }
                .unwrap_or_else(|error| panic!("{target:?} module should render: {error}"));
                let label = format!("{target:?} retained={retained_data} product={product}");

                let blocks = resource_blocks(&module);
                let guard = "acme_app_destroy_guard.this";
                assert!(blocks.contains_key(guard), "{label}: guard emitted");
                if retained_data {
                    assert!(
                        blocks.contains_key("terraform_data.retention_inventory"),
                        "{label}: retention inventory present"
                    );
                }
                if product {
                    assert!(
                        blocks.contains_key("terraform_data.remote_operator_ownership"),
                        "{label}: Remote Operator state latch present"
                    );
                }
                // The guard lists every other managed resource directly.
                let others: std::collections::BTreeSet<String> = blocks
                    .keys()
                    .filter(|address| address.as_str() != guard)
                    .cloned()
                    .collect();
                let listed: std::collections::BTreeSet<String> =
                    guard_depends_on(&blocks[guard]).into_iter().collect();
                assert_eq!(
                    others, listed,
                    "{label}: the guard's depends_on must be exactly every other resource"
                );
            }
        }
    }
}

/// A real plan, with mocked providers, of the module `source` renders for `target` against an
/// existing cluster. `asserts` are HCL `assert` blocks over the plan.
async fn plan_existing_cluster(target: TerraformTarget, source: Source, asserts: &str) {
    plan_existing_cluster_with(
        target,
        source,
        (
            "projects/p/global/networks/cluster-net",
            "projects/p/regions/europe-west1/subnetworks/cluster-subnet",
        ),
        "",
        asserts,
    )
    .await
}

/// [`plan_existing_cluster`] with the network and subnetwork the GKE cluster reports, extra
/// variables, and the body of the `run` block (`expect_failures` or asserts).
async fn plan_existing_cluster_with(
    target: TerraformTarget,
    source: Source,
    gke_cluster: (&str, &str),
    extra_variables: &str,
    asserts: &str,
) {
    let (cluster_network, cluster_subnetwork) = gke_cluster;
    let (mocks, variables) = match target {
        TerraformTarget::Gke => (
            format!(r#"mock_provider "google" {{
  mock_data "google_container_cluster" {{
    defaults = {{
      workload_identity_config = [{{ workload_pool = "p.svc.id.goog" }}]
      network = "{cluster_network}"
      subnetwork = "{cluster_subnetwork}"
    }}
  }}
  mock_data "google_compute_network" {{
    defaults = {{ self_link = "https://www.googleapis.com/compute/v1/projects/p/global/networks/cluster-net" }}
  }}
  mock_data "google_compute_subnetwork" {{
    defaults = {{ self_link = "https://www.googleapis.com/compute/v1/projects/p/regions/europe-west1/subnetworks/cluster-subnet" }}
  }}
}}"#),
            r#"gke_cluster_name = "c"
  gke_cluster_location = "europe-west1-b"
  gcp_project = "p"
  gcp_region = "europe-west1""#,
        ),
        TerraformTarget::Aks => (
            r#"mock_provider "azurerm" {
  mock_data "azurerm_client_config" {
    defaults = { tenant_id = "00000000-0000-0000-0000-000000000001", object_id = "00000000-0000-0000-0000-000000000001", client_id = "00000000-0000-0000-0000-000000000001" }
  }
}"#.to_string(),
            r#"aks_cluster_name = "c"
  aks_cluster_resource_group_name = "rg""#,
        ),
        TerraformTarget::Eks => (
            r#"mock_provider "aws" {
  mock_data "aws_availability_zones" {
    defaults = { names = ["us-east-1a", "us-east-1b", "us-east-1c"] }
  }
  mock_data "aws_eks_cluster" {
    defaults = { identity = [{ oidc = [{ issuer = "https://oidc.eks.example.com/id/ABC" }] }] }
  }
}
mock_provider "tls" {
  mock_data "tls_certificate" {
    defaults = { certificates = [{ sha1_fingerprint = "0000000000000000000000000000000000000000" }] }
  }
}"#
            .to_string(),
            r#"eks_cluster_name = "c""#,
        ),
        other => panic!("not an EKS, GKE or AKS target: {other:?}"),
    };
    let module = render_prepared_with_emitters(source, target).await;
    let mut files: indexmap::IndexMap<String, String> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(path, body)| (path.to_string(), body.to_string()))
        .collect();
    // Every other variable the module requires gets a placeholder.
    let required = hcl::parse(files.get("variables.tf").expect("variables.tf"))
        .expect("variables.tf parses")
        .blocks()
        .filter(|block| block.identifier() == "variable")
        .filter(|block| !block.body().attributes().any(|a| a.key() == "default"))
        .filter_map(|block| {
            block
                .labels()
                .first()
                .map(|label| label.as_str().to_string())
        })
        .filter(|name| name != "kubernetes_cluster_mode" && !name.contains("cluster_"))
        .filter(|name| !matches!(name.as_str(), "gcp_project" | "gcp_region"))
        .map(|name| format!("{name} = \"xx\""))
        .collect::<Vec<_>>()
        .join("\n  ");
    files.insert(
        "existing.tftest.hcl".to_string(),
        format!(
            r#"{mocks}
variables {{
  {required}
  management_url = "https://manager.example.com"
  kubernetes_cluster_mode = "existing"
  {variables}
  {extra_variables}
}}
run "existing_cluster" {{
  command = plan
{asserts}
}}
"#
        ),
    );
    super::helpers::test_utils::terraform_test(&files)
        .assert_ok(format!("{target:?} existing-cluster plan"));
}

/// A real plan of the GKE/AKS module in existing-cluster mode: the module creates no network,
/// registers none, and nothing that reads the network's outputs is evaluated (a gated network
/// has no instance to read).
#[tokio::test]
async fn gke_and_aks_existing_cluster_plans_with_no_network() {
    for target in [TerraformTarget::Gke, TerraformTarget::Aks] {
        plan_existing_cluster(
            target,
            Source::Workload,
            r#"  assert {
    condition = !contains([for r in local.deployment_resources : r.id], "default-network")
    error_message = "an existing cluster registers no network"
  }
  assert {
    condition = try(local.deployment_settings.network, null) == null
    error_message = "an existing cluster registers no network setting"
  }"#,
        )
        .await;
    }
}

/// A stack with a cloud database keeps a network in an existing cluster, and it is the
/// cluster's: looked up from the cluster, created by no one, registered as one the deployer
/// brought, with the settings that name it.
#[tokio::test]
async fn gke_existing_cluster_with_a_database_uses_the_cluster_network() {
    plan_existing_cluster(
        TerraformTarget::Gke,
        Source::Database,
        r#"  assert {
    condition = length(google_compute_network.default_network) == 0 && length(google_compute_subnetwork.default_network_workload) == 0 && length(google_compute_router.default_network_router) == 0
    error_message = "an existing cluster must not get a VPC, subnet or router"
  }
  assert {
    condition = data.google_compute_network.default_network[0].name == "cluster-net" || local.existing_network_name == "cluster-net"
    error_message = "the network is looked up by the cluster's network name"
  }
  assert {
    condition = local.existing_subnet_name == "cluster-subnet" && local.existing_network_region == "europe-west1"
    error_message = "the subnet and region are the cluster's"
  }
  assert {
    condition = [for r in local.deployment_resources : r if r.id == "default-network"][0].importData.isByoVpc == true
    error_message = "the cluster's network is registered as one setup does not own"
  }
  assert {
    condition = local.deployment_settings.network.type == "byo-vpc-gcp" && local.deployment_settings.network.networkName == "cluster-net" && local.deployment_settings.network.subnetName == "cluster-subnet"
    error_message = "the settings name the cluster's network"
  }"#,
    )
    .await;
}

/// An install that created its cluster before the network was gated holds the network's blocks
/// without an index. Gating them adds `count`, which moves every address to `[0]`; without a
/// `moved` block Terraform would destroy the network under the running cluster on the next apply.
#[tokio::test]
async fn gating_the_network_moves_the_addresses_a_created_cluster_already_holds() {
    for target in [
        TerraformTarget::Gke,
        TerraformTarget::Aks,
        TerraformTarget::Eks,
    ] {
        for (label, source) in [
            ("workload", Source::Workload),
            ("database", Source::Database),
        ] {
            let module = render_prepared(source, target).await;
            let mut counted: Vec<String> = Vec::new();
            let mut moved: Vec<String> = Vec::new();
            for (name, text) in module.iter() {
                if !name.ends_with(".tf") {
                    continue;
                }
                let body = hcl::parse(text).expect("module file parses");
                for block in body.blocks() {
                    let labels: Vec<&str> = block.labels().iter().map(|l| l.as_str()).collect();
                    match (block.identifier(), labels.as_slice()) {
                        ("resource", [kind, label]) => {
                            let count = block
                                .body()
                                .attributes()
                                .find(|a| a.key() == "count")
                                .map(|a| a.expr().to_string().replace(' ', ""));
                            // Only a count that is exactly the gate is new; an emitter's own
                            // count already indexed the address.
                            if count.as_deref().is_some_and(|c| {
                                c.contains("deployment_requests_network")
                                    && c.ends_with("?1:0")
                                    && !c.contains("network_mode")
                            }) {
                                counted.push(format!("{kind}.{label}"));
                            }
                        }
                        ("moved", []) => {
                            let attr = |key: &str| {
                                block
                                    .body()
                                    .attributes()
                                    .find(|a| a.key() == key)
                                    .map(|a| a.expr().to_string().replace(' ', ""))
                                    .unwrap_or_default()
                            };
                            moved.push(format!("{}->{}", attr("from"), attr("to")));
                        }
                        _ => {}
                    }
                }
            }
            // EKS and GKE blocks carried their own `count` (the network mode) before the gate,
            // so their addresses are already indexed. AKS blocks had none.
            if target == TerraformTarget::Aks && label == "workload" {
                assert!(
                    !counted.is_empty(),
                    "{target:?}/{label}: the AKS network blocks are gated"
                );
            } else {
                // A stack with a database keeps its network in every mode: ungated, unmoved.
                assert!(counted.is_empty(), "{target:?}/{label}: {counted:?}");
            }
            for address in &counted {
                let expected = format!("{address}->{address}[0]");
                assert!(
                    moved.contains(&expected.replace(' ', "")),
                    "{target:?}: {address} gains a count and needs a moved block; moved = {moved:?}"
                );
            }
        }
    }
}

/// An existing GKE cluster on a Shared VPC (its network belongs to a host project) or with its
/// subnet in another region than `gcp_region` cannot take the database endpoint, which the module
/// creates in its own project and region. The plan says so, before any lookup, instead of
/// failing later with a missing network or an endpoint the cluster cannot reach.
#[tokio::test]
async fn an_existing_gke_cluster_the_database_cannot_reach_fails_at_plan_with_a_clear_message() {
    for (network, subnetwork, failing) in [
        (
            "projects/host-project/global/networks/shared-net",
            "projects/host-project/regions/europe-west1/subnetworks/shared-subnet",
            "data.google_compute_network.default_network, data.google_compute_subnetwork.default_network_existing_subnet",
        ),
        (
            "projects/p/global/networks/cluster-net",
            "projects/p/regions/us-east1/subnetworks/cluster-subnet",
            "data.google_compute_subnetwork.default_network_existing_subnet",
        ),
    ] {
        plan_existing_cluster_with(
            TerraformTarget::Gke,
            Source::Database,
            (network, subnetwork),
            "",
            &format!("  expect_failures = [{failing}]"),
        )
        .await;
    }
}

/// A GKE stack with a database and nothing running on the cluster declares no cluster, so the
/// module has no cluster to look the network up from: it must still validate, and use the
/// module's own network inputs.
#[tokio::test]
async fn a_gke_stack_with_only_a_database_has_no_cluster_lookup() {
    let module = render_prepared(Source::DatabaseOnly, TerraformTarget::Gke).await;
    let text = whole_module(&module);
    assert!(
        !text.contains("kubernetes_existing"),
        "no cluster is declared, so nothing may read its lookup"
    );
    assert_terraform_valid(&module, "gke_database_only");
}

/// A stack with only a Redis instance (no Postgres) still needs a network in every mode on every
/// cloud: the Redis private endpoint refers to its subnet. One predicate answers it for the
/// preflight and both generators.
#[tokio::test]
async fn a_redis_only_stack_keeps_the_default_network_in_every_mode() {
    for target in [
        TerraformTarget::Eks,
        TerraformTarget::Gke,
        TerraformTarget::Aks,
    ] {
        let module = render_prepared(Source::Redis, target).await;
        assert_network_in_every_mode(&module);
        assert_terraform_valid(&module, &format!("{target:?}_redis_only"));
    }
}

/// An existing GKE cluster with a Redis instance keeps the module's own network inputs (as
/// installed modules do): Memorystore replaces itself when its network changes, and its deletion
/// protection refuses that. No cluster lookup is read for it.
#[tokio::test]
async fn an_existing_gke_cluster_with_redis_keeps_the_module_network_inputs() {
    let module = render_prepared(Source::Redis, TerraformTarget::Gke).await;
    let locals: String = whole_module(&module)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        locals.contains("effective_network_mode=var.network_mode"),
        "{locals}"
    );
    assert!(
        !locals.contains("kubernetes_existing[0].network"),
        "{locals}"
    );
}

/// Existing cluster, no database: the cluster may sit in another project (Shared VPC) and region
/// than the module's. Nothing needs a network, so no lookup runs and no precondition fires.
#[tokio::test]
async fn an_existing_gke_cluster_in_another_project_and_region_plans_with_no_database() {
    plan_existing_cluster_with(
        TerraformTarget::Gke,
        Source::Workload,
        (
            "projects/host-project/global/networks/shared-net",
            "projects/host-project/regions/us-east1/subnetworks/shared-subnet",
        ),
        "",
        r#"  assert {
    condition = length(data.google_compute_network.default_network) == 0 && length(data.google_compute_subnetwork.default_network_existing_subnet) == 0
    error_message = "no network is kept, so no lookup runs"
  }"#,
    )
    .await;
}

/// A database with no workload declares no cluster. In an existing-cluster mode the module
/// creates its own network by `network_mode`, and registers exactly that network's settings.
#[tokio::test]
async fn a_gke_database_without_a_cluster_registers_the_network_it_creates() {
    plan_existing_cluster_with(
        TerraformTarget::Gke,
        Source::DatabaseOnly,
        (
            "projects/p/global/networks/unused",
            "projects/p/regions/europe-west1/subnetworks/unused",
        ),
        "",
        r#"  assert {
    condition = length(google_compute_network.default_network) == 1
    error_message = "the module creates the network"
  }
  assert {
    condition = local.deployment_settings.network.type == "create"
    error_message = "the registered settings describe the network that was created"
  }"#,
    )
    .await;
}

/// Postgres plus a Redis instance the deployer can decline, in an existing GKE cluster: with
/// Redis declined the database uses the cluster's network; with it accepted the module keeps its
/// own network inputs (the instance's network cannot change). The registered settings follow.
#[tokio::test]
async fn a_declined_redis_leaves_the_database_on_the_clusters_network() {
    let cluster = (
        "projects/p/global/networks/cluster-net",
        "projects/p/regions/europe-west1/subnetworks/cluster-subnet",
    );
    plan_existing_cluster_with(
        TerraformTarget::Gke,
        Source::DatabaseWithGatedRedis,
        cluster,
        "input_redis_enabled = false",
        r#"  assert {
    condition = local.effective_network_mode == "use-existing" && length(google_compute_network.default_network) == 0
    error_message = "with Redis declined the cluster's network is used and none is created"
  }
  assert {
    condition = local.deployment_settings.network.type == "byo-vpc-gcp" && local.deployment_settings.network.networkName == "cluster-net"
    error_message = "the settings name the cluster's network"
  }"#,
    )
    .await;
    plan_existing_cluster_with(
        TerraformTarget::Gke,
        Source::DatabaseWithGatedRedis,
        cluster,
        "input_redis_enabled = true",
        "  expect_failures = [terraform_data.existing_cluster_network_guard]",
    )
    .await;
    // A deployer who brings the network names the cluster's own: that is theirs to choose.
    plan_existing_cluster_with(
        TerraformTarget::Gke,
        Source::DatabaseWithGatedRedis,
        cluster,
        "input_redis_enabled = true\n  network_mode = \"use-existing\"\n  network_name = \"cluster-net\"\n  subnet_name = \"cluster-subnet\"",
        r#"  assert {
    condition = local.effective_network_mode == "use-existing" && length(google_compute_network.default_network) == 0
    error_message = "the network the deployer names is used and none is created"
  }"#,
    )
    .await;
}

/// Setup cannot put a cache or database endpoint on an existing cluster's network except a GCP
/// Postgres, so the module fails the plan for every other cell with the message that says what
/// to do instead. A cell that stays supported plans clean: Postgres on an existing GKE cluster.
#[tokio::test]
async fn every_database_an_existing_cluster_cannot_reach_is_refused_at_plan() {
    let cluster = (
        "projects/p/global/networks/cluster-net",
        "projects/p/regions/europe-west1/subnetworks/cluster-subnet",
    );
    for (target, source, resource) in [
        (TerraformTarget::Gke, Source::Redis, "redis"),
        (
            TerraformTarget::Gke,
            Source::DatabaseWithGatedRedis,
            "redis",
        ),
        (TerraformTarget::Aks, Source::Redis, "redis"),
        (TerraformTarget::Aks, Source::Database, "postgres"),
        (TerraformTarget::Eks, Source::Redis, "redis"),
        (TerraformTarget::Eks, Source::Database, "postgres"),
    ] {
        let module = render_prepared_with_emitters(source, target).await;
        let guard = whole_module(&module);
        assert!(
            guard.contains("Setup cannot yet place the")
                && guard.contains("Use a cluster that setup creates"),
            "{target:?} {resource}: the module carries the refusal"
        );
        let gated = matches!(source, Source::DatabaseWithGatedRedis);
        plan_existing_cluster_with(
            target,
            source,
            cluster,
            if gated {
                "input_redis_enabled = true"
            } else {
                ""
            },
            "  expect_failures = [terraform_data.existing_cluster_network_guard]",
        )
        .await;
    }
}

/// The refusal does not touch a created cluster, nor the supported cell.
#[tokio::test]
async fn a_created_cluster_and_a_gke_postgres_are_not_refused() {
    for (target, source) in [
        (TerraformTarget::Gke, Source::Database),
        (TerraformTarget::Gke, Source::Workload),
        (TerraformTarget::Aks, Source::Workload),
        (TerraformTarget::Eks, Source::Workload),
    ] {
        let module = render_prepared(source, target).await;
        assert_terraform_valid(&module, &format!("{target:?}_supported_cell"));
    }
    plan_existing_cluster(
        TerraformTarget::Gke,
        Source::Database,
        r#"  assert {
    condition = local.effective_network_mode == "use-existing"
    error_message = "the database goes on the cluster's network"
  }"#,
    )
    .await;
}

/// A database the installer binds to infrastructure it already has (no emitter registered, as in
/// the built-in registry) is the deployer's, so setup refuses nothing for it.
#[tokio::test]
async fn a_database_the_installer_binds_is_not_refused() {
    for (target, source) in [
        (TerraformTarget::Gke, Source::Redis),
        (TerraformTarget::Aks, Source::Database),
        (TerraformTarget::Eks, Source::Redis),
    ] {
        let module = render_prepared(source, target).await;
        assert!(
            module.get("existing-cluster-network-guard.tf").is_none(),
            "{target:?}: nothing setup creates needs a guard"
        );
    }
}

fn existing_cluster_settings(network: Option<NetworkSettings>) -> StackSettings {
    StackSettings {
        kubernetes: Some(KubernetesSettings {
            cluster: Some(alien_core::KubernetesClusterSettings {
                ownership: KubernetesClusterOwnership::Existing,
                namespace: None,
                cloud: None,
            }),
            exposure: None,
        }),
        network,
        ..Default::default()
    }
}

/// A setup rendered for an existing cluster refuses at render time, with the error code and the
/// plain sentence, what setup creates and the cluster could not reach. A database the installer
/// binds, a supported cell and a network the deployer names are not refused.
#[tokio::test]
async fn rendering_for_an_existing_cluster_refuses_what_it_could_not_reach() {
    for (target, source, refused) in [
        (TerraformTarget::Gke, Source::Redis, true),
        (TerraformTarget::Aks, Source::Database, true),
        (TerraformTarget::Eks, Source::Redis, true),
        (TerraformTarget::Gke, Source::Database, false),
    ] {
        let stack = prepared_stack(source, target).await;
        let result = super::helpers::render_with_registry(
            &stack,
            target,
            existing_cluster_settings(None),
            &registry_with_database_emitters(),
        );
        if refused {
            let error = result.expect_err("refused");
            assert_eq!(error.code, "SETUP_PLACEMENT_UNSUPPORTED", "{target:?}");
            assert_eq!(error.http_status_code, Some(422));
            assert!(!error.retryable);
            assert!(
                error.message.starts_with("Setup cannot yet place the")
                    && error.message.contains("Use a cluster that setup creates"),
                "{target:?}: {}",
                error.message
            );
        } else {
            result.expect("a supported cell renders");
        }
        // Not created by setup: left to the installer.
        super::helpers::render_with_registry(
            &stack,
            target,
            existing_cluster_settings(None),
            &TfRegistry::built_in(),
        )
        .expect("an installer-bound database is not refused");
    }
    // A network the deployer names is theirs to choose.
    let stack = prepared_stack(Source::Redis, TerraformTarget::Gke).await;
    super::helpers::render_with_registry(
        &stack,
        TerraformTarget::Gke,
        existing_cluster_settings(Some(NetworkSettings::ByoVpcGcp {
            network_name: "cluster-net".to_string(),
            subnet_name: "cluster-subnet".to_string(),
            region: "europe-west1".to_string(),
        })),
        &registry_with_database_emitters(),
    )
    .expect("a named network is not refused");
}

/// A cluster the stack names something other than `kubernetes` is looked up under its own label.
#[test]
fn a_gke_cluster_with_its_own_name_has_its_network_looked_up_under_that_name() {
    let stack = Stack::new("named".to_string())
        .add(
            KubernetesCluster::new("primary".to_string())
                .provider(KubernetesClusterProvider::Gke)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("default".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Postgres::new("db".to_string()).build(),
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
    let module = super::helpers::render_with_registry(
        &stack,
        TerraformTarget::Gke,
        StackSettings::default(),
        &registry_with_database_emitters(),
    )
    .expect("renders");
    let text = whole_module(&module);
    assert!(text.contains("primary_existing[0]"), "{text}");
    assert!(!text.contains("kubernetes_existing"), "{text}");
    assert_terraform_valid(&module, "gke_named_cluster_with_database");
}

/// A registered module that installs the chart, for the chart's `infrastructure` values.
fn render_registered_with_helm(
    stack: &Stack,
    target: TerraformTarget,
    product: bool,
) -> alien_terraform::ModuleFiles {
    let registry = TfRegistry::built_in();
    let options = TerraformOptions {
        setup_identity: None,
        pins: alien_core::SetupPins::compiled_in(),
        display_name: None,
        registry: &registry,
        stack_settings: StackSettings::default(),
        registration: Some(TerraformRegistration {
            provider_name: "acme_app".to_string(),
            provider_source: "pkg.example.com/acme/app".to_string(),
            provider_version: "1.0.0".to_string(),
            resource_type: "deployment".to_string(),
            release_id: Some("rel-test".to_string()),
            setup_target: "kubernetes".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            destroy_guard: true,
        }),
        helm_install: Some(TerraformHelmInstall {
            chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
            release_name: "acme-operator".to_string(),
        }),
        supported_aws_regions: Vec::new(),
    };
    if product {
        generate_product_terraform_module(stack, target, options, false)
    } else {
        generate_terraform_module(stack, target, options)
    }
    .expect("module should render")
}

fn chart_bound_stack(id: &str, provider: KubernetesClusterProvider, gated_extra: bool) -> Stack {
    let mut stack = Stack::new(id.to_string());
    if gated_extra {
        stack = stack.inputs(vec![gate_input(
            "extraStore",
            "Extra store",
            "Create the extra store.",
        )]);
    }
    if provider == KubernetesClusterProvider::Aks {
        stack = stack
            .add(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureStorageAccount::new("default-storage-account".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
    }
    let stack = stack
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
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    if gated_extra {
        stack
            .add_enabled_when(
                Storage::new("extra".to_string()).build(),
                ResourceLifecycle::Frozen,
                "extraStore",
            )
            .build()
    } else {
        stack.build()
    }
}

/// The release the module installs gets the same `infrastructure` bindings as the
/// `chart_infrastructure_json` output, after the provider's values that render `null`.
#[test]
fn the_installed_chart_gets_the_bindings_of_the_resources_the_module_created() {
    for (target, provider, bucket) in [
        (
            TerraformTarget::Eks,
            KubernetesClusterProvider::Eks,
            "aws_s3_bucket.data",
        ),
        (
            TerraformTarget::Gke,
            KubernetesClusterProvider::Gke,
            "google_storage_bucket.data",
        ),
        (
            TerraformTarget::Aks,
            KubernetesClusterProvider::Aks,
            "azurerm_storage_container.data",
        ),
    ] {
        let stack = chart_bound_stack("chart-helm-bindings", provider, false);
        for product in [false, true] {
            let module = render_registered_with_helm(&stack, target, product);
            let helm = module.get("helm.tf").expect("helm.tf");
            let compact = helm.split_whitespace().collect::<Vec<_>>().join(" ");
            let values = compact
                .split("values = [")
                .nth(1)
                .and_then(|rest| rest.split(" ]").next())
                .unwrap_or_else(|| panic!("no values list: {helm}"));
            let provider_at = values.find("local.chart_values_provider_yaml");
            let infrastructure_at = values.find("yamlencode({ infrastructure = {");
            assert!(
                provider_at.is_some() && infrastructure_at.is_some(),
                "{target:?} product={product}: {values}"
            );
            assert!(
                provider_at < infrastructure_at,
                "the bindings come after the provider's null: {values}"
            );
            assert!(
                values.contains("type = \"storage\"") && values.contains(bucket),
                "{target:?} product={product}: {values}"
            );
            if product {
                assert!(
                    values.find("infrastructure = {") < values.find("remoteOperator"),
                    "{values}"
                );
            }
            // The output carries the same object.
            let outputs = module.get("outputs.tf").expect("outputs.tf");
            assert!(outputs.contains("chart_infrastructure_json"), "{outputs}");
            // The `helm_values` output carries the same infrastructure entry, merged in the
            // release's order: provider, infrastructure, remoteOperator.
            let local = compact
                .split("chart_values_infrastructure = ")
                .nth(1)
                .unwrap_or_else(|| panic!("no chart_values_infrastructure local: {helm}"));
            assert!(
                local.starts_with("{ infrastructure = {")
                    && local.contains("type = \"storage\"")
                    && local.contains(bucket),
                "{target:?} product={product}: {local}"
            );
            let output = compact
                .split("output \"helm_values\"")
                .nth(1)
                .unwrap_or_else(|| panic!("no helm_values output: {helm}"));
            let provider_at = output.find("local.chart_values_provider");
            let infra_at = output.find("local.chart_values_infrastructure");
            assert!(provider_at < infra_at, "{output}");
            if product {
                assert!(
                    infra_at < output.find("remoteOperator = merge("),
                    "{output}"
                );
            }
        }
    }
}

/// A module that creates nothing the chart binds leaves the release's values as they were.
#[test]
fn the_installed_chart_values_are_unchanged_without_chart_bound_resources() {
    let stack = Stack::new("chart-no-bindings".to_string())
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
    for product in [false, true] {
        let module = render_registered_with_helm(&stack, TerraformTarget::Eks, product);
        let helm = module.get("helm.tf").expect("helm.tf");
        assert!(
            !helm.contains("infrastructure"),
            "product={product}: {helm}"
        );
        assert!(!module
            .get("outputs.tf")
            .expect("outputs.tf")
            .contains("chart_infrastructure_json"));
    }
}

/// A resource gated on an input has a binding in the release only while its gate is true.
#[test]
fn the_installed_chart_bindings_follow_the_resource_gates() {
    let stack = chart_bound_stack("chart-helm-gated", KubernetesClusterProvider::Eks, true);
    let module = render_registered_with_helm(&stack, TerraformTarget::Eks, false);
    let helm = module.get("helm.tf").expect("helm.tf");
    let compact = helm.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(compact.contains("var.input_extra_store ?"), "{helm}");
    assert!(compact.contains("merge("), "{helm}");
    assert!(
        !compact.contains("aws_s3_bucket.extra.bucket"),
        "a gated bucket needs its count index: {helm}"
    );
}

/// A real plan, with mocked providers, of a gated chart-bound resource: the installed release's
/// values carry the `infrastructure` entry (and the extra store's binding) only while the gate
/// is on. The registration provider cannot be installed here, so it is the stand-in the
/// verified-setup tests use, and its `helm_values` (which render `infrastructure: null`) are a
/// fixed string.
#[test]
fn the_installed_release_values_carry_a_gated_binding_only_while_the_gate_is_on() {
    let stack = chart_bound_stack("chart-helm-plan", KubernetesClusterProvider::Eks, true);
    let registry = TfRegistry::built_in();
    let module = generate_terraform_module(
        &stack,
        TerraformTarget::Eks,
        TerraformOptions {
            setup_identity: None,
            pins: alien_core::SetupPins::compiled_in(),
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: Some(TerraformRegistration {
                provider_name: "terraform".to_string(),
                provider_source: "registry.example.com/acme/app".to_string(),
                provider_version: "1.0.0".to_string(),
                resource_type: "data".to_string(),
                release_id: Some("rel-test".to_string()),
                setup_target: "kubernetes".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                destroy_guard: false,
            }),
            helm_install: Some(TerraformHelmInstall {
                chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
                release_name: "acme-operator".to_string(),
            }),
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("module should render");
    let mut files = super::verified_setup_tests::stand_in_files(&module);
    let helm = files.get("helm.tf").expect("helm.tf").clone();
    assert!(helm.contains("terraform_data.this.helm_values"), "{helm}");
    files.insert(
        "helm.tf".to_string(),
        helm.replace(
            "terraform_data.this.helm_values",
            "\"infrastructure: null\\n\"",
        ),
    );
    let required = hcl::parse(files.get("variables.tf").expect("variables.tf"))
        .expect("variables.tf parses")
        .blocks()
        .filter(|block| block.identifier() == "variable")
        .filter(|block| !block.body().attributes().any(|a| a.key() == "default"))
        .filter_map(|block| block.labels().first().map(|l| l.as_str().to_string()))
        .filter(|name| {
            ![
                "input_extra_store",
                "management_url",
                "eks_cluster_name",
                "kubernetes_cluster_mode",
                "helm_install_enabled",
            ]
            .contains(&name.as_str())
        })
        .map(|name| format!("{name} = \"xx\""))
        .collect::<Vec<_>>()
        .join("\n  ");
    files.insert(
        "gate.tftest.hcl".to_string(),
        format!(
            r#"mock_provider "aws" {{
  mock_data "aws_availability_zones" {{
    defaults = {{ names = ["us-east-1a", "us-east-1b", "us-east-1c"] }}
  }}
  mock_data "aws_eks_cluster" {{
    defaults = {{ identity = [{{ oidc = [{{ issuer = "https://oidc.eks.example.com/id/ABC" }}] }}] }}
  }}
}}
mock_provider "tls" {{
  mock_data "tls_certificate" {{
    defaults = {{ certificates = [{{ sha1_fingerprint = "0000000000000000000000000000000000000000" }}] }}
  }}
}}
mock_provider "helm" {{}}
variables {{
  {required}
  management_url = "https://manager.example.com"
  eks_cluster_name = "c"
  kubernetes_cluster_mode = "existing"
  helm_install_enabled = true
  resource_prefix = "acme"
}}
run "gate_off" {{
  command = plan
  variables {{
    input_extra_store = false
  }}
  assert {{
    condition = length(helm_release.runtime[0].values) == 2 && join(",", sort(keys(yamldecode(helm_release.runtime[0].values[1]).infrastructure))) == "data"
    error_message = "gate off: the release carries the data store's binding and not the extra store's"
  }}
}}
run "gate_on" {{
  command = plan
  variables {{
    input_extra_store = true
  }}
  assert {{
    condition = join(",", sort(keys(yamldecode(helm_release.runtime[0].values[1]).infrastructure))) == "data,extra"
    error_message = "gate on: both stores are bound in the release"
  }}
}}
"#
        ),
    );
    super::helpers::test_utils::terraform_test(&files)
        .assert_ok("EKS chart-bound gated plan, mocked providers");
}

/// The modules that install the chart pass `terraform fmt -check` as rendered and
/// `terraform validate` with the registration provider swapped for the stand-in the
/// verified-setup tests use (the real one cannot be installed here); its `helm_values` are a
/// fixed string.
#[test]
fn the_modules_that_install_the_chart_are_formatted_and_valid() {
    for (target, provider) in [
        (TerraformTarget::Eks, KubernetesClusterProvider::Eks),
        (TerraformTarget::Gke, KubernetesClusterProvider::Gke),
        (TerraformTarget::Aks, KubernetesClusterProvider::Aks),
    ] {
        let stack = chart_bound_stack("chart-helm-valid", provider, true);
        for product in [false, true] {
            let context = format!("{target:?} product={product}");
            let module = render_registered_with_helm(&stack, target, product);
            // As rendered: formatting needs no provider.
            assert_terraform_formatted(&module, &context);
            // The stand-in registration provider is `terraform_data`.
            let registry = TfRegistry::built_in();
            let options = TerraformOptions {
                setup_identity: None,
                pins: alien_core::SetupPins::compiled_in(),
                display_name: None,
                registry: &registry,
                stack_settings: StackSettings::default(),
                registration: Some(TerraformRegistration {
                    provider_name: "terraform".to_string(),
                    provider_source: "registry.example.com/acme/app".to_string(),
                    provider_version: "1.0.0".to_string(),
                    resource_type: "data".to_string(),
                    release_id: Some("rel-test".to_string()),
                    setup_target: "kubernetes".to_string(),
                    setup_fingerprint: "test".to_string(),
                    setup_fingerprint_version: 1,
                    destroy_guard: false,
                }),
                helm_install: Some(TerraformHelmInstall {
                    chart_ref: "oci://pkg.example.com/acme/app/helm".to_string(),
                    release_name: "acme-operator".to_string(),
                }),
                supported_aws_regions: Vec::new(),
            };
            let stood_in = if product {
                generate_product_terraform_module(&stack, target, options, false)
            } else {
                generate_terraform_module(&stack, target, options)
            }
            .expect("module should render");
            let mut files = super::verified_setup_tests::stand_in_files(&stood_in);
            let helm = files.get("helm.tf").expect("helm.tf").clone();
            assert!(helm.contains("terraform_data.this.helm_values"), "{helm}");
            files.insert(
                "helm.tf".to_string(),
                helm.replace(
                    "terraform_data.this.helm_values",
                    "\"infrastructure: null\\n\"",
                ),
            );
            super::helpers::test_utils::terraform_validate(&files)
                .assert_ok(format!("{context} terraform validate"));
        }
    }
}
