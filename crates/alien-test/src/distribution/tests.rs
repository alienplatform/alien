use super::*;
use alien_core::import::AzureServiceBusNamespaceImportData;
use alien_core::permissions::{ManagementPermissions, PermissionProfile};
use alien_core::ResourceLifecycle;
use alien_core::{
    Container, ContainerCode, KubernetesClusterSettings, Kv, Queue, ResourceSpec, Storage, Vault,
    Worker, WorkerCode,
};

use crate::config::{AwsConfig, AzureConfig, GcpConfig};
use crate::helm_values::to_helm_values_yaml;

mod runtime_values;

fn empty_test_config() -> TestConfig {
    TestConfig {
        aws_mgmt: None,
        aws_target: None,
        aws_resources: crate::config::AwsTestResources {
            s3_bucket: None,
            command_kv_table: None,
            lambda_image: None,
            lambda_execution_role_arn: None,
            ecr_push_role_arn: None,
            ecr_pull_role_arn: None,
            ecr_repository: None,
        },
        gcp_mgmt: None,
        gcp_target: None,
        gcp_resources: crate::config::GcpTestResources {
            gcs_bucket: None,
            cloudrun_image: None,
            gar_repository: None,
        },
        azure_mgmt: None,
        azure_target: None,
        azure_resources: crate::config::AzureTestResources {
            resource_group: None,
            storage_account: None,
            blob_container: None,
            container_app_image: None,
            managed_environment_name: None,
            registry_name: None,
            acr_repository: None,
            shared_container_env: None,
        },
        e2e_artifact_registry: crate::config::E2eArtifactRegistryConfig {
            aws_ar_push_role_arn: None,
            aws_ar_pull_role_arn: None,
            gcp_gar_repository: None,
            gcp_ar_pull_sa_email: None,
            gcp_ar_push_sa_email: None,
            azure_acr_repository: None,
        },
        kubernetes: crate::config::KubernetesTestConfig::default(),
        e2e_network_mode: crate::config::E2eNetworkMode::None,
        kubernetes_cluster_mode: KubernetesClusterMode::Existing,
    }
}

fn test_config_with_platform(platform: Platform) -> TestConfig {
    let mut config = empty_test_config();
    match platform {
        Platform::Aws => {
            let credentials = AwsConfig {
                access_key_id: "test".to_string(),
                secret_access_key: "test".to_string(),
                session_token: None,
                region: "us-east-1".to_string(),
                account_id: Some("123456789012".to_string()),
            };
            config.aws_mgmt = Some(credentials.clone());
            config.aws_target = Some(credentials);
        }
        Platform::Gcp => {
            let credentials = GcpConfig {
                project_id: "test-project".to_string(),
                region: "us-central1".to_string(),
                credentials_json: Some("{}".to_string()),
                management_identity_email: None,
                management_identity_unique_id: None,
            };
            config.gcp_mgmt = Some(credentials.clone());
            config.gcp_target = Some(credentials);
        }
        Platform::Azure => {
            let credentials = AzureConfig {
                subscription_id: "test-subscription".to_string(),
                tenant_id: "test-tenant".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                region: "eastus".to_string(),
                principal_id: Some("test-principal".to_string()),
                oidc_issuer: None,
                oidc_subject: None,
            };
            config.azure_mgmt = Some(credentials.clone());
            config.azure_target = Some(credentials);
        }
        other => panic!("unsupported test platform: {other}"),
    }
    config
}

#[test]
fn managed_kubernetes_distribution_availability_uses_base_cloud() {
    for (flow, base_platform) in [
        (DistributionFlow::CloudFormationEksHelmPull, Platform::Aws),
        (DistributionFlow::TerraformEksHelmPull, Platform::Aws),
        (DistributionFlow::TerraformGkeHelmPull, Platform::Gcp),
        (DistributionFlow::TerraformAksHelmPull, Platform::Azure),
    ] {
        assert!(is_distribution_flow_available(
            flow,
            &test_config_with_platform(base_platform),
            TestApp::RuntimeLessMixed,
        ));
        assert!(!is_distribution_flow_available(
            flow,
            &empty_test_config(),
            TestApp::RuntimeLessMixed,
        ));
    }
}

#[test]
fn onprem_distribution_requires_a_cloud_registry_platform() {
    assert!(!is_distribution_flow_available(
        DistributionFlow::TerraformOnpremHelmPull,
        &empty_test_config(),
        TestApp::RuntimeLessMixed,
    ));
    assert!(is_distribution_flow_available(
        DistributionFlow::TerraformOnpremHelmPull,
        &test_config_with_platform(Platform::Gcp),
        TestApp::RuntimeLessMixed,
    ));
}

fn contains_resource_type(stack: &Stack, resource_type: &str) -> bool {
    stack
        .resources()
        .any(|(_, entry)| entry.config.resource_type().as_ref() == resource_type)
}

fn imported_resource<T: serde::Serialize>(
    resource_type: &'static str,
    data: &T,
) -> ImportedResource {
    ImportedResource {
        id: resource_type.to_string(),
        resource_type: alien_core::ResourceType::from_static(resource_type),
        import_data: serde_json::to_value(data).expect("import data should serialize"),
    }
}

#[test]
fn azure_management_probe_uses_service_bus_when_stack_emits_it() {
    let service_bus = AzureServiceBusNamespaceImportData {
        subscription_id: "subscription".to_string(),
        resource_group: "resource-group".to_string(),
        namespace_name: "namespace".to_string(),
        endpoint: "namespace.servicebus.windows.net".to_string(),
    };
    let resources = vec![imported_resource(
        "azure_service_bus_namespace",
        &service_bus,
    )];

    let probe =
        azure_management_permission_probe(&resources).expect("probe resource should be selected");

    assert_eq!(
        probe,
        AzureManagementPermissionProbe::ServiceBus(service_bus)
    );
}

#[test]
fn azure_management_probe_uses_resource_graph_when_stack_has_no_service_bus() {
    let resources = Vec::new();

    let probe =
        azure_management_permission_probe(&resources).expect("probe resource should be selected");

    assert_eq!(probe, AzureManagementPermissionProbe::ResourceGraph);
}

#[test]
fn azure_management_probe_rejects_malformed_service_bus_import_data() {
    let resources = vec![ImportedResource {
        id: "azure_service_bus_namespace".to_string(),
        resource_type: alien_core::ResourceType::from_static("azure_service_bus_namespace"),
        import_data: serde_json::json!({"resourceGroup": "resource-group"}),
    }];

    let error = azure_management_permission_probe(&resources)
        .expect_err("malformed Service Bus data must not silently fall back");

    assert!(
        error
            .to_string()
            .contains("Failed to parse azure_service_bus_namespace import data"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn distribution_wait_budget_accounts_for_slow_cloud_control_planes() {
    assert_eq!(
        deployment_running_timeout(Platform::Azure, TestApp::ComprehensiveRust),
        Duration::from_secs(1_800)
    );
    assert_eq!(
        deployment_running_timeout(Platform::Kubernetes, TestApp::ComprehensiveRust),
        Duration::from_secs(1_800)
    );
    assert_eq!(
        deployment_running_timeout(Platform::Kubernetes, TestApp::FullStackMicroservices),
        Duration::from_secs(3_600)
    );
    assert_eq!(
        deployment_running_timeout(Platform::Aws, TestApp::ComprehensiveRust),
        Duration::from_secs(600)
    );
    assert_eq!(
        deployment_running_timeout(Platform::Gcp, TestApp::ComprehensiveRust),
        Duration::from_secs(600)
    );
}

#[test]
fn existing_kubernetes_cluster_mode_marks_cluster_as_existing() {
    let mut settings = StackSettings {
        kubernetes: Some(KubernetesSettings {
            cluster: Some(KubernetesClusterSettings {
                ownership: KubernetesClusterOwnership::Managed,
                namespace: Some("alien-worker-runtime".to_string()),
                cloud: None,
            }),
            exposure: Some(KubernetesExposureSettings::Disabled),
        }),
        ..StackSettings::default()
    };

    set_kubernetes_cluster_ownership(&mut settings, KubernetesClusterOwnership::Existing);

    let kubernetes = settings.kubernetes.expect("kubernetes settings");
    let cluster = kubernetes.cluster.expect("cluster settings");
    assert_eq!(cluster.ownership, KubernetesClusterOwnership::Existing);
    assert_eq!(cluster.namespace.as_deref(), Some("alien-worker-runtime"));
    assert_eq!(
        kubernetes.exposure,
        Some(KubernetesExposureSettings::Disabled)
    );
}

#[tokio::test]
async fn gke_existing_cluster_render_does_not_add_cloud_network() {
    let source_stack = Stack::new("gke-existing-cluster-source".to_string())
        .permission(
            "execution",
            PermissionProfile::new().global(["worker/execute"]),
        )
        .add(
            Container::new("api".to_string())
                .permissions("execution".to_string())
                .code(ContainerCode::Image {
                    image: "manager.example.com/alien-e2e:tag".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "0.25".to_string(),
                    desired: "0.25".to_string(),
                })
                .memory(ResourceSpec {
                    min: "128Mi".to_string(),
                    desired: "128Mi".to_string(),
                })
                .port(8080)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let mut stack_settings = stack_settings_for_flow(DeploymentModel::Pull);
    set_kubernetes_cluster_ownership(&mut stack_settings, KubernetesClusterOwnership::Existing);

    let rendered_stack = terraform_kubernetes_stack_for_target(
        source_stack,
        alien_terraform::TerraformTarget::Gke,
        stack_settings,
    )
    .await
    .expect("GKE existing-cluster Terraform render preflights should pass");

    assert!(
        contains_resource_type(&rendered_stack, "kubernetes-cluster"),
        "GKE render should still add the KubernetesCluster handoff resource"
    );
    assert!(
        !contains_resource_type(&rendered_stack, "network"),
        "GKE existing-cluster render must not add a setup-owned GCP VPC for Kubernetes workloads"
    );
}

#[tokio::test]
async fn distribution_source_stack_remains_valid_after_setup_render_mutations() {
    let source_stack = Stack::new("distribution-source".to_string())
        .permission(
            "execution",
            PermissionProfile::new().global(["worker/execute"]),
        )
        .add(
            Worker::new("alien-rs-worker".to_string())
                .permissions("execution".to_string())
                .code(WorkerCode::Image {
                    image: "manager.example.com/alien-e2e:tag".to_string(),
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let stack_settings = stack_settings_for_flow(DeploymentModel::Push);

    assert!(
        !contains_resource_type(&source_stack, "remote-stack-management"),
        "release/source stack must not contain setup-authored resources"
    );

    let runner = alien_preflights::runner::PreflightRunner::new();
    runner
        .run_template_preflights(&source_stack, Platform::Aws)
        .await
        .expect("release/source stack should pass setup import preflights");

    let rendered_stack =
        apply_render_mutations(source_stack.clone(), Platform::Aws, &stack_settings)
            .await
            .expect("distribution render mutations should succeed");
    assert!(
        contains_resource_type(&rendered_stack, "remote-stack-management"),
        "rendered setup artifact stack should include remote management"
    );
    assert!(
        runner
            .run_template_preflights(&rendered_stack, Platform::Aws)
            .await
            .is_err(),
        "rendered setup stack is not a valid release/source stack"
    );
}

#[tokio::test]
async fn gcp_management_probe_skips_direct_target_without_remote_management() {
    wait_for_gcp_management_permissions(
        &empty_test_config(),
        &serde_json::json!({
            "deployment_management_config": {
                "value": "null"
            }
        }),
        false,
    )
    .await
    .expect("direct-target GCP setup should not require management config");
}

#[tokio::test]
async fn gcp_management_probe_skips_null_management_output() {
    wait_for_gcp_management_permissions(
        &empty_test_config(),
        &serde_json::json!({
            "deployment_management_config": {
                "value": "null"
            }
        }),
        true,
    )
    .await
    .expect("null Terraform management config should not require a GCP management probe");
}

#[tokio::test]
async fn terraform_output_kubeconfig_is_materialized_for_helm() {
    let workdir = tempfile::tempdir().expect("tempdir");
    let cleanup = DistributionArtifactCleanup::Terraform {
        workdir,
        env: Vec::new(),
    };
    let kubeconfig = r#""apiVersion": "v1"
"clusters": []
"contexts": []
"current-context": "test"
"kind": "Config"
"users": []
"#;
    let mut target = KubernetesHelmTarget {
        namespace: "alien-test".to_string(),
        runtime: KubernetesRuntimeConfig {
            kubeconfig: kubeconfig.to_string(),
            kube_context: Some("test".to_string()),
            namespace_prefix: "alien-test".to_string(),
        },
    };

    materialize_kubeconfig_for_helm(&mut target, &cleanup)
        .await
        .expect("kubeconfig should be written");

    assert_ne!(target.runtime.kubeconfig, kubeconfig);
    assert_eq!(
        std::fs::read_to_string(&target.runtime.kubeconfig).expect("kubeconfig file"),
        kubeconfig
    );
}

#[tokio::test]
async fn existing_kubeconfig_path_is_left_unchanged() {
    let workdir = tempfile::tempdir().expect("tempdir");
    let kubeconfig_path = workdir.path().join("existing.kubeconfig");
    std::fs::write(&kubeconfig_path, "apiVersion: v1\n").expect("write kubeconfig");
    let cleanup = DistributionArtifactCleanup::Terraform {
        workdir,
        env: Vec::new(),
    };
    let mut target = KubernetesHelmTarget {
        namespace: "alien-test".to_string(),
        runtime: KubernetesRuntimeConfig {
            kubeconfig: kubeconfig_path.to_string_lossy().into_owned(),
            kube_context: None,
            namespace_prefix: "alien-test".to_string(),
        },
    };

    materialize_kubeconfig_for_helm(&mut target, &cleanup)
        .await
        .expect("existing path should be accepted");

    assert_eq!(target.runtime.kubeconfig, kubeconfig_path.to_string_lossy());
}

#[tokio::test]
async fn eks_pull_distribution_values_include_manager_irsa() {
    let source_stack = Stack::new("distribution-eks-pull".to_string())
        .permission(
            "execution",
            PermissionProfile::new().global(["worker/execute"]),
        )
        .add(
            Worker::new("alien-rs-worker".to_string())
                .permissions("execution".to_string())
                .code(WorkerCode::Image {
                    image: "manager.example.com/alien-e2e:tag".to_string(),
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let stack_settings = stack_settings_for_flow(DeploymentModel::Pull);

    let rendered_stack = terraform_kubernetes_stack_for_target(
        source_stack,
        alien_terraform::TerraformTarget::Eks,
        stack_settings.clone(),
    )
    .await
    .expect("Kubernetes Terraform render mutations should succeed");
    assert!(
        contains_resource_type(&rendered_stack, "remote-stack-management"),
        "pull-mode Kubernetes setup needs a manager cloud identity"
    );

    let registry = alien_terraform::TfRegistry::built_in();
    let module = alien_terraform::generate_terraform_module(
        &rendered_stack,
        alien_terraform::TerraformTarget::Eks,
        alien_terraform::TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings,
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("Terraform generation should succeed");
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(
        rendered.contains("helm_manager_service_account = {"),
        "Terraform locals should include the manager service account values handed to Helm"
    );
    assert!(
        rendered.contains("\"eks.amazonaws.com/role-arn\" = aws_iam_role.management.arn"),
        "Helm values must annotate the manager service account with its IRSA role"
    );
    assert!(
        rendered.contains("id   = \"management\""),
        "rendered Terraform should include the management identity resource"
    );
    assert!(
        rendered.contains("eks:DescribeCluster"),
        "management role must be able to read the EKS cluster for cloud metadata heartbeat"
    );
    assert!(
        rendered.contains("arn:aws:eks:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:cluster/${local.kubernetes_cluster_name}"),
        "EKS cluster read must follow the Terraform-selected cluster name, not only the resource prefix"
    );
    assert!(
        rendered.contains("resource \"aws_iam_openid_connect_provider\" \"eks\"")
            && rendered.contains("data \"tls_certificate\" \"eks_oidc\""),
        "Terraform setup must create the EKS OIDC provider before Helm handoff"
    );
    assert!(
        rendered.contains("aws_iam_openid_connect_provider.eks[0].arn"),
        "IRSA trust must depend on the Terraform-managed OIDC provider"
    );
    assert!(
        !rendered.contains("iam:CreateOpenIDConnectProvider")
            && !rendered.contains("iam:UpdateAssumeRolePolicy"),
        "pull-agent management policy must not include workload identity bootstrap permissions"
    );
}

#[tokio::test]
async fn eks_pull_distribution_does_not_carry_cloud_compute_cluster() {
    let source_stack = Stack::new("distribution-eks-containers".to_string())
        .permission("execution", PermissionProfile::new())
        .add(
            Container::new("api".to_string())
                .permissions("execution".to_string())
                .code(ContainerCode::Image {
                    image: "manager.example.com/api:tag".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "0.25".to_string(),
                    desired: "0.25".to_string(),
                })
                .memory(ResourceSpec {
                    min: "128Mi".to_string(),
                    desired: "128Mi".to_string(),
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let stack_settings = stack_settings_for_flow(DeploymentModel::Pull);

    let rendered_stack = terraform_kubernetes_stack_for_target(
        source_stack,
        alien_terraform::TerraformTarget::Eks,
        stack_settings.clone(),
    )
    .await
    .expect("Kubernetes Terraform render mutations should succeed");

    assert!(
        contains_resource_type(&rendered_stack, "kubernetes-cluster"),
        "Kubernetes Terraform setup should include the cluster substrate"
    );
    assert!(
        contains_resource_type(&rendered_stack, "remote-stack-management"),
        "Kubernetes Terraform setup should include the cloud management identity"
    );
    assert!(
        !contains_resource_type(&rendered_stack, "compute-cluster"),
        "Kubernetes Terraform setup must not reuse the cloud VM compute substrate"
    );
    let secrets = rendered_stack
        .resources
        .get("secrets")
        .expect("Kubernetes setup should include the managed secrets vault");
    assert!(
        secrets
            .dependencies
            .iter()
            .all(|dependency| dependency.id() != "management-sa"),
        "cloud-backed Kubernetes setup uses remote-stack-management, not a stack-local management-sa"
    );

    let registry = alien_terraform::TfRegistry::built_in();
    alien_terraform::generate_terraform_module(
        &rendered_stack,
        alien_terraform::TerraformTarget::Eks,
        alien_terraform::TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings,
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("Terraform generation should not require a compute-cluster emitter");
}

fn azure_config(
    subscription_id: &str,
    tenant_id: &str,
    region: &str,
    oidc_issuer: Option<&str>,
    oidc_subject: Option<&str>,
) -> AzureConfig {
    AzureConfig {
        subscription_id: subscription_id.to_string(),
        tenant_id: tenant_id.to_string(),
        client_id: "client-id".to_string(),
        client_secret: "client-secret".to_string(),
        region: region.to_string(),
        principal_id: None,
        oidc_issuer: oidc_issuer.map(ToString::to_string),
        oidc_subject: oidc_subject.map(ToString::to_string),
    }
}

#[tokio::test]
async fn gcp_distribution_render_grants_live_worker_provision() {
    let stack = Stack::new("distribution-gcp".to_string())
        .permission(
            "execution",
            PermissionProfile::new().global(["worker/execute"]),
        )
        .add(
            Worker::new("alien-rs-fn".to_string())
                .permissions("execution".to_string())
                .code(WorkerCode::Image {
                    image: "us-central1-docker.pkg.dev/project/repo/alien-rs-fn:tag".to_string(),
                })
                // Cloud Run gen2 rejects < 512 MiB; the `WorkerMemoryCheck`
                // preflight enforces that at plan time. The default
                // `memory_mb` (256) is below the floor, so we set it
                // explicitly here to keep the fixture valid for GCP.
                .memory_mb(512)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let stack_settings = stack_settings_for_flow(DeploymentModel::Push);

    let rendered_stack = apply_render_mutations(stack, Platform::Gcp, &stack_settings)
        .await
        .expect("distribution render mutations should succeed");
    let registry = alien_terraform::TfRegistry::built_in();
    let module = alien_terraform::generate_terraform_module(
        &rendered_stack,
        alien_terraform::TerraformTarget::Gcp,
        alien_terraform::TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings,
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("Terraform generation should succeed");
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains(
        "resource \"google_project_iam_custom_role\" \"gcp_role_manage_cloud_run_services\""
    ));
    assert!(rendered.contains("run.services.update"));
    assert!(rendered.contains("roles/iam.serviceAccountUser"));
    assert!(rendered.contains("roles/artifactregistry.reader"));
}

#[tokio::test]
async fn gcp_distribution_render_scopes_vault_management_roles_per_vault() {
    let stack = Stack::new("distribution-gcp-vaults".to_string())
        .add(
            Vault::new("alien-vault".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let stack_settings = stack_settings_for_flow(DeploymentModel::Push);

    let rendered_stack = apply_render_mutations(stack, Platform::Gcp, &stack_settings)
        .await
        .expect("distribution render mutations should succeed");
    let registry = alien_terraform::TfRegistry::built_in();
    let module = alien_terraform::generate_terraform_module(
        &rendered_stack,
        alien_terraform::TerraformTarget::Gcp,
        alien_terraform::TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings,
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("Terraform generation should succeed");
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();
    let iam_member_declarations = rendered
        .lines()
        .filter(|line| line.contains("resource \"google_project_iam_member\""))
        .collect::<Vec<_>>();
    let unique_iam_member_declarations = iam_member_declarations
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(
        unique_iam_member_declarations.len(),
        iam_member_declarations.len(),
        "GCP IAM member declarations should be unique: {iam_member_declarations:?}"
    );
    let viewer_bindings = rendered
        .matches("role    = \"roles/secretmanager.viewer\"")
        .count();
    assert_eq!(
        viewer_bindings, 4,
        "GCP vault heartbeat/management bindings should be emitted once per target scope"
    );
    assert_eq!(
        rendered
            .matches("title       = \"ResourceVaultSecretsHeartbeat\"")
            .count(),
        2,
        "resource-scoped vault heartbeat conditions should be emitted once per generated vault"
    );
}

#[test]
fn azure_tfvars_include_oidc_federated_identity_inputs() {
    let azure_target = azure_config("target-sub", "target-tenant", "eastus", None, None);
    let azure_mgmt = azure_config(
        "mgmt-sub",
        "mgmt-tenant",
        "eastus2",
        Some("https://issuer.example.com"),
        Some("system:serviceaccount:alien:manager"),
    );
    let mut vars = serde_json::Map::new();

    insert_azure_tfvars(
        &mut vars,
        &azure_target,
        Some(&azure_mgmt),
        alien_terraform::TerraformTarget::Azure,
    );

    assert_eq!(
        vars.get("azure_subscription_id").and_then(Value::as_str),
        Some("target-sub")
    );
    assert!(vars.get("azure_tenant_id").is_none());
    assert_eq!(
        vars.get("azure_managing_tenant_id").and_then(Value::as_str),
        Some("mgmt-tenant")
    );
    assert_eq!(
        vars.get("azure_oidc_issuer").and_then(Value::as_str),
        Some("https://issuer.example.com")
    );
    assert_eq!(
        vars.get("azure_oidc_subject").and_then(Value::as_str),
        Some("system:serviceaccount:alien:manager")
    );
    assert!(vars.get("azure_management_principal_id").is_none());
}

#[test]
fn azure_tfvars_include_target_tenant_for_aks() {
    let azure_target = azure_config("target-sub", "target-tenant", "eastus", None, None);
    let mut vars = serde_json::Map::new();

    insert_azure_tfvars(
        &mut vars,
        &azure_target,
        None,
        alien_terraform::TerraformTarget::Aks,
    );

    assert_eq!(
        vars.get("azure_tenant_id").and_then(Value::as_str),
        Some("target-tenant")
    );
}

#[test]
fn azure_tfvars_omit_oidc_inputs_when_management_config_is_missing() {
    let azure_target = azure_config("target-sub", "target-tenant", "eastus", None, None);
    let mut vars = serde_json::Map::new();

    insert_azure_tfvars(
        &mut vars,
        &azure_target,
        None,
        alien_terraform::TerraformTarget::Azure,
    );

    assert!(vars.get("azure_management_principal_id").is_none());
    assert!(vars.get("azure_oidc_issuer").is_none());
    assert!(vars.get("azure_oidc_subject").is_none());
}

#[tokio::test]
async fn azure_direct_terraform_render_omits_remote_management_and_oidc() {
    let source_stack = Stack::new("distribution-azure-direct".to_string())
        .add(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let stack_settings = stack_settings_for_flow(DeploymentModel::Push);

    let rendered_stack = apply_render_mutations_with_management_config(
        source_stack,
        Platform::Azure,
        &stack_settings,
        None,
    )
    .await
    .expect("Azure direct-target render mutations should succeed");
    assert!(
        !contains_resource_type(&rendered_stack, "remote-stack-management"),
        "direct-target setup should not create remote stack management"
    );

    let registry = alien_terraform::TfRegistry::built_in();
    let module = alien_terraform::generate_terraform_module(
        &rendered_stack,
        alien_terraform::TerraformTarget::Azure,
        alien_terraform::TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings,
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("Terraform generation should succeed");
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("deployment_management_config = null"));
    assert!(!rendered.contains("variable \"azure_oidc_issuer\""));
    assert!(!rendered.contains("variable \"azure_oidc_subject\""));
    assert!(
        !rendered.contains("resource \"azurerm_federated_identity_credential\" \"management_fic\"")
    );
}

#[tokio::test]
async fn azure_push_distribution_render_grants_setup_heartbeats_to_management() {
    let source_stack = Stack::new("distribution-azure-rsm".to_string())
        .permission(
            "execution",
            PermissionProfile::new().global(["worker/execute"]),
        )
        .add(
            Worker::new("api".to_string())
                .permissions("execution".to_string())
                .code(WorkerCode::Image {
                    image: "manager.example.com/api:tag".to_string(),
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .add(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .add(
            Kv::new("state".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .add(
            Queue::new("commands".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .build();
    let stack_settings = stack_settings_for_flow(DeploymentModel::Push);

    let rendered_stack = apply_render_mutations(source_stack, Platform::Azure, &stack_settings)
        .await
        .expect("Azure push render mutations should succeed");

    let ManagementPermissions::Extend(management_profile) = rendered_stack.management() else {
        panic!("Azure push render should generate management permissions");
    };
    let global_permission_ids: Vec<_> = management_profile
        .0
        .get("*")
        .expect("management profile should include global permissions")
        .iter()
        .map(|permission| permission.id().to_string())
        .collect();

    for expected in [
        "azure-resource-group/heartbeat",
        "azure-storage-account/heartbeat",
        "azure-service-bus-namespace/heartbeat",
        "observe/observe",
        "service-account/heartbeat",
        "service-activation/heartbeat",
    ] {
        assert!(
            global_permission_ids.contains(&expected.to_string()),
            "Azure management profile should include {expected}"
        );
    }
    assert!(
        !global_permission_ids
            .iter()
            .any(|permission| permission.contains("azure_")
                || permission.contains("service_activation")),
        "Azure management profile must use permission-set IDs, not Rust resource type names"
    );
}
