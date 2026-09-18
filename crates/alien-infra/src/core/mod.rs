mod controller;
pub use controller::*;

mod registry;
pub use registry::*;
mod executor;
pub use executor::{PlanResult, RunningResourcePolicy, StackExecutor, StepResult};

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

#[cfg(feature = "kubernetes")]
pub(crate) mod kubernetes_errors;

mod azure_permissions_helper;
pub use azure_permissions_helper::*;

mod resource_permissions_helper;
pub use resource_permissions_helper::*;

use std::collections::BTreeMap;

use alien_core::{branded_standard_resource_tags, branded_tag_key, Platform, ALIEN_STACK_TAG_KEY};

const LEGACY_LABEL_DOMAIN_MARKER_PREFIX: &str = "alien.dev/legacy-label-domain-";
const CURRENT_LABEL_DOMAIN_MARKER: &str = "alien.dev/label-domain";
const KUBERNETES_LABEL_VALUE_MAX_LEN: usize = 63;

fn current_kubernetes_label_domain(configured: &str) -> String {
    alien_core::access_request_crd::current_kubernetes_label_domain(configured)
}

fn explicit_legacy_label_domain<'a>(configured: &'a str, current: &str) -> Option<&'a str> {
    (configured != current)
        .then(|| {
            alien_core::access_request_crd::explicit_legacy_kubernetes_label_domain(configured)
        })
        .flatten()
}

fn is_valid_kubernetes_dns_subdomain(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        })
}

fn insert_legacy_label_domain_markers(
    labels: &mut BTreeMap<String, String>,
    legacy_label_domain: &str,
) {
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    for segment in legacy_label_domain.split('.') {
        let candidate_len = chunk.len() + usize::from(!chunk.is_empty()) + segment.len();
        if candidate_len > KUBERNETES_LABEL_VALUE_MAX_LEN {
            chunks.push(std::mem::take(&mut chunk));
        }
        if !chunk.is_empty() {
            chunk.push('.');
        }
        chunk.push_str(segment);
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }

    for (index, chunk) in chunks.into_iter().enumerate() {
        labels.insert(format!("{LEGACY_LABEL_DOMAIN_MARKER_PREFIX}{index}"), chunk);
    }
}

fn legacy_label_domain(labels: &BTreeMap<String, String>) -> Option<String> {
    let mut chunks = labels
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix(LEGACY_LABEL_DOMAIN_MARKER_PREFIX)
                .and_then(|index| index.parse::<usize>().ok())
                .map(|index| (index, value))
        })
        .collect::<Vec<_>>();
    if chunks.is_empty() {
        return None;
    }
    chunks.sort_by_key(|(index, _)| *index);
    if chunks
        .iter()
        .enumerate()
        .any(|(expected, (actual, value))| {
            expected != *actual || value.is_empty() || value.len() > KUBERNETES_LABEL_VALUE_MAX_LEN
        })
    {
        return None;
    }
    let domain = chunks
        .into_iter()
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>()
        .join(".");
    is_valid_kubernetes_dns_subdomain(&domain).then_some(domain)
}

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
    let current_label_domain = current_kubernetes_label_domain(label_domain);
    let mut labels: BTreeMap<_, _> =
        branded_standard_resource_tags(&current_label_domain, ctx.resource_prefix, resource_id)
            .into_iter()
            .collect();
    labels.insert(
        CURRENT_LABEL_DOMAIN_MARKER.to_string(),
        current_label_domain.clone(),
    );
    if let Some(legacy_label_domain) =
        explicit_legacy_label_domain(label_domain, &current_label_domain)
    {
        insert_legacy_label_domain_markers(&mut labels, legacy_label_domain);
    }
    labels
}

/// Labels every controller-owned object that uninstall cleanup may delete.
/// The unbranded marker is the cleanup selector's stable ownership boundary;
/// the branded labels scope that ownership to one deployment and resource.
pub fn kubernetes_cleanup_resource_labels(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> BTreeMap<String, String> {
    let mut labels = kubernetes_branded_resource_labels(ctx, resource_id);
    labels.insert("managed-by".to_string(), "runtime".to_string());
    labels
}

pub fn kubernetes_deployment_scope_label(ctx: &ResourceControllerContext<'_>) -> (String, String) {
    kubernetes_deployment_scope_label_for(
        ctx.deployment_config.label_domain.as_deref(),
        ctx.resource_prefix,
    )
}

pub fn kubernetes_deployment_scope_label_for(
    label_domain: Option<&str>,
    resource_prefix: &str,
) -> (String, String) {
    let label_domain = label_domain.unwrap_or(DEFAULT_ALIEN_LABEL_DOMAIN);
    (
        branded_tag_key(
            current_kubernetes_label_domain(label_domain),
            ALIEN_STACK_TAG_KEY,
        ),
        resource_prefix.to_string(),
    )
}

/// Whether an existing Kubernetes object may be migrated to the desired
/// deployment scope. Objects without a deployment label are legacy and may be
/// adopted only after the caller proves their old resource-specific identity.
/// An object already scoped to any deployment must match this one exactly.
pub(crate) fn kubernetes_labels_have_compatible_scope(
    existing: Option<&BTreeMap<String, String>>,
    desired: &BTreeMap<String, String>,
) -> bool {
    let Some(current_domain) = desired.get(CURRENT_LABEL_DOMAIN_MARKER) else {
        return false;
    };
    let current_key = branded_tag_key(current_domain, ALIEN_STACK_TAG_KEY);
    let current_resource_key = format!("{current_domain}/resource");
    let legacy_domain = legacy_label_domain(desired);
    let legacy_key = legacy_domain
        .as_deref()
        .map(|domain| branded_tag_key(domain, ALIEN_STACK_TAG_KEY));
    let legacy_resource_key = legacy_domain
        .as_deref()
        .map(|domain| format!("{domain}/resource"));
    let Some(expected_value) = desired.get(&current_key) else {
        return false;
    };
    let Some(labels) = existing else {
        return true;
    };
    let expected_resource = desired.get(&current_resource_key);
    let deployment_labels = labels
        .iter()
        .filter(|(key, _)| key.ends_with("/deployment"))
        .collect::<Vec<_>>();
    if deployment_labels.iter().any(|(key, value)| {
        if *key == &current_key || legacy_key.as_ref() == Some(*key) {
            *value != expected_value
        } else {
            true
        }
    }) {
        return false;
    }

    if labels.contains_key(&current_key) {
        return expected_resource
            .is_none_or(|resource| labels.get(&current_resource_key) == Some(resource));
    }
    if let Some(legacy_key) = legacy_key.as_ref().filter(|key| labels.contains_key(*key)) {
        debug_assert_eq!(labels.get(legacy_key), Some(expected_value));
        return legacy_resource_key.as_ref().is_some_and(|resource_key| {
            expected_resource.is_none_or(|resource| labels.get(resource_key) == Some(resource))
        });
    }

    deployment_labels.is_empty()
}

/// Whether an object carries one of the deployment-scope labels explicitly
/// emitted for the current configuration. This is stricter than compatibility:
/// an unlabeled legacy dependent can be compatible after its root proves
/// ownership, but it is not independently recognizable as deployment-owned.
pub(crate) fn kubernetes_labels_have_recognized_scope(
    existing: Option<&BTreeMap<String, String>>,
    desired: &BTreeMap<String, String>,
) -> bool {
    let Some(current_domain) = desired.get(CURRENT_LABEL_DOMAIN_MARKER) else {
        return false;
    };
    let current_key = branded_tag_key(current_domain, ALIEN_STACK_TAG_KEY);
    let legacy_key =
        legacy_label_domain(desired).map(|domain| branded_tag_key(domain, ALIEN_STACK_TAG_KEY));

    existing.is_some_and(|labels| {
        labels.contains_key(&current_key)
            || legacy_key
                .as_ref()
                .is_some_and(|key| labels.contains_key(key))
    })
}

pub(crate) fn kubernetes_labels_match_identity(
    existing: Option<&BTreeMap<String, String>>,
    desired: &BTreeMap<String, String>,
    identity_keys: &[&str],
) -> bool {
    existing.is_some_and(|existing| {
        identity_keys.iter().all(|key| {
            desired
                .get(*key)
                .is_some_and(|expected| existing.get(*key) == Some(expected))
        })
    })
}

pub(crate) fn kubernetes_labels_match_current_scope(
    existing: Option<&BTreeMap<String, String>>,
    desired: &BTreeMap<String, String>,
) -> bool {
    let Some(current_domain) = desired.get(CURRENT_LABEL_DOMAIN_MARKER) else {
        return false;
    };
    let expected_key = branded_tag_key(current_domain, ALIEN_STACK_TAG_KEY);
    let Some(expected_value) = desired.get(&expected_key) else {
        return false;
    };
    let expected_resource_key = format!("{current_domain}/resource");
    existing.is_some_and(|labels| {
        labels.get(&expected_key) == Some(expected_value)
            && desired
                .get(&expected_resource_key)
                .is_none_or(|value| labels.get(&expected_resource_key) == Some(value))
    })
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

    #[test]
    fn valid_multi_segment_legacy_label_domains_may_exceed_sixty_three_characters() {
        let domain = "customer-release-identity-with-a-long-but-valid-label.platform.example.com";
        assert!(domain.len() > 63);
        assert_eq!(
            explicit_legacy_label_domain(domain, "customerreleaseidentitywithalongbutvalidlabel"),
            Some(domain)
        );
        let mut labels = BTreeMap::new();
        insert_legacy_label_domain_markers(&mut labels, domain);
        assert!(labels
            .values()
            .all(|value| value.len() <= 63 && is_valid_kubernetes_dns_subdomain(value)));
        assert_eq!(legacy_label_domain(&labels).as_deref(), Some(domain));
    }

    #[test]
    fn invalid_legacy_label_domains_are_not_emitted() {
        for domain in [
            "Upper.example",
            "-bad.example",
            "bad-.example",
            "bad_.example",
        ] {
            assert_eq!(explicit_legacy_label_domain(domain, "different"), None);
        }
    }

    fn desired_scoped_labels() -> BTreeMap<String, String> {
        let mut labels = BTreeMap::from([
            ("alien.dev/label-domain".to_string(), "acme".to_string()),
            ("acme/deployment".to_string(), "release-a".to_string()),
            ("acme/resource".to_string(), "worker".to_string()),
        ]);
        insert_legacy_label_domain_markers(&mut labels, "acme.example");
        labels
    }

    #[test]
    fn deployment_scope_rejects_conflicting_current_and_legacy_labels() {
        let desired = desired_scoped_labels();
        let current_conflict = BTreeMap::from([
            ("acme/deployment".to_string(), "release-b".to_string()),
            ("acme/resource".to_string(), "worker".to_string()),
            (
                "acme.example/deployment".to_string(),
                "release-a".to_string(),
            ),
            ("acme.example/resource".to_string(), "worker".to_string()),
        ]);
        let legacy_conflict = BTreeMap::from([
            ("acme/deployment".to_string(), "release-a".to_string()),
            ("acme/resource".to_string(), "worker".to_string()),
            (
                "acme.example/deployment".to_string(),
                "release-b".to_string(),
            ),
            ("acme.example/resource".to_string(), "worker".to_string()),
        ]);

        assert!(!kubernetes_labels_have_compatible_scope(
            Some(&current_conflict),
            &desired
        ));
        assert!(!kubernetes_labels_have_compatible_scope(
            Some(&legacy_conflict),
            &desired
        ));
    }

    #[test]
    fn deployment_scope_requires_the_matching_resource_identity() {
        let desired = desired_scoped_labels();
        let wrong_current_resource = BTreeMap::from([
            ("acme/deployment".to_string(), "release-a".to_string()),
            ("acme/resource".to_string(), "other".to_string()),
        ]);
        let matching_legacy = BTreeMap::from([
            (
                "acme.example/deployment".to_string(),
                "release-a".to_string(),
            ),
            ("acme.example/resource".to_string(), "worker".to_string()),
        ]);

        assert!(!kubernetes_labels_have_compatible_scope(
            Some(&wrong_current_resource),
            &desired
        ));
        assert!(kubernetes_labels_have_compatible_scope(
            Some(&matching_legacy),
            &desired
        ));
        assert!(kubernetes_labels_have_compatible_scope(
            Some(&BTreeMap::new()),
            &desired
        ));
    }
}

// Test utilities
#[cfg(any(feature = "test-utils", doc, test))]
pub mod controller_test;

#[cfg(test)]
mod executor_tests;
