mod controller;
pub use controller::*;

mod registry;
pub use registry::*;
mod executor;
pub use executor::{PlanResult, StackExecutor, StepResult};

mod service_provider;
pub use service_provider::*;

mod certificates;
pub use certificates::*;

pub mod state_utils;
pub use state_utils::*;

pub mod environment_variables;
pub use environment_variables::*;

pub mod k8s_secret_bindings;
pub use k8s_secret_bindings::*;

#[cfg(feature = "kubernetes")]
pub mod k8s_environment_secrets;
#[cfg(feature = "kubernetes")]
pub use k8s_environment_secrets::*;

#[cfg(all(test, feature = "kubernetes"))]
pub(crate) mod kubernetes_manifest_test_support;

mod azure_permissions_helper;
pub use azure_permissions_helper::*;

mod resource_permissions_helper;
pub use resource_permissions_helper::*;

use std::collections::BTreeMap;

use alien_core::{branded_standard_resource_tags, Platform, DEFAULT_ALIEN_LABEL_DOMAIN};

pub fn kubernetes_runtime_pod_labels(
    ctx: &ResourceControllerContext<'_>,
    labels: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    kubernetes_runtime_pod_labels_for_platform(
        ctx.platform,
        ctx.deployment_config.base_platform,
        labels,
    )
}

pub fn kubernetes_branded_resource_labels(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> BTreeMap<String, String> {
    let label_domain = ctx
        .deployment_config
        .label_domain
        .as_deref()
        .unwrap_or(DEFAULT_ALIEN_LABEL_DOMAIN);
    branded_standard_resource_tags(label_domain, ctx.resource_prefix, resource_id)
        .into_iter()
        .collect()
}

fn kubernetes_runtime_pod_labels_for_platform(
    platform: Platform,
    base_platform: Option<Platform>,
    mut labels: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    if platform == Platform::Kubernetes && base_platform == Some(Platform::Azure) {
        labels.insert(
            "azure.workload.identity/use".to_string(),
            "true".to_string(),
        );
    }
    labels
}

#[cfg(test)]
mod kubernetes_runtime_pod_label_tests {
    use super::*;

    #[test]
    fn azure_backed_kubernetes_pods_get_workload_identity_label() {
        let labels = kubernetes_runtime_pod_labels_for_platform(
            Platform::Kubernetes,
            Some(Platform::Azure),
            BTreeMap::from([("app".to_string(), "worker".to_string())]),
        );

        assert_eq!(
            labels
                .get("azure.workload.identity/use")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(labels.get("app").map(String::as_str), Some("worker"));
    }

    #[test]
    fn non_azure_kubernetes_pods_do_not_get_azure_workload_identity_label() {
        let labels = kubernetes_runtime_pod_labels_for_platform(
            Platform::Kubernetes,
            Some(Platform::Gcp),
            BTreeMap::new(),
        );

        assert!(!labels.contains_key("azure.workload.identity/use"));
    }
}

// Test utilities
#[cfg(any(feature = "test-utils", doc, test))]
pub mod controller_test;

#[cfg(test)]
mod executor_tests;
