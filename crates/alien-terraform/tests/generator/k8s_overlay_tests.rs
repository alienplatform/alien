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
    AzureResourceGroup, Worker, WorkerCode, Ingress, ResourceLifecycle, ServiceAccount, Stack,
    StackSettings, Storage,
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
