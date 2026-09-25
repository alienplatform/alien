//! Application release identity read from the observed environment.
//!
//! Remote Operator deployments run an application Alien did not ship, so the
//! Operator reports what it finds: a version, the Helm chart that installed
//! the workloads, and the images they run.

use std::collections::BTreeSet;

use alien_core::{
    sync::{ObservedApplicationImage, ObservedApplicationReport, ObservedApplicationSource},
    ObservedInventoryBatch, Platform,
};

/// Label Helm's standard chart helper sets to `<chart name>-<chart version>`.
const HELM_CHART_LABEL: &str = "helm.sh/chart";

/// Resolve a single app version from the observed inventory — the distinct,
/// non-empty `version` read off the workloads (e.g. `app.kubernetes.io/version`).
/// Returns it only when exactly one version is present; a multi-version environment
/// reports none rather than guessing, so the deployment shows "no release" until a
/// vendor pins one via `OPERATOR_RELEASE_VERSION` or `alien release`.
pub(super) fn single_observed_version(batches: &[ObservedInventoryBatch]) -> Option<String> {
    let mut versions = BTreeSet::new();
    for batch in batches {
        for sample in &batch.resources {
            if let Some(version) = sample.version.as_deref() {
                let version = version.trim();
                if !version.is_empty() {
                    versions.insert(version.to_string());
                }
            }
        }
    }
    if versions.len() == 1 {
        versions.into_iter().next()
    } else {
        None
    }
}

/// Application identity from observed Kubernetes workloads: the Helm chart
/// when every labelled workload names the same one, and every distinct
/// container image. `None` when the workloads carry neither.
pub(super) fn observed_application(
    batches: &[ObservedInventoryBatch],
) -> Option<ObservedApplicationReport> {
    let batches = batches
        .iter()
        .filter(|batch| batch.controller_platform == Platform::Kubernetes)
        .collect::<Vec<_>>();
    let observed_at = batches.iter().map(|batch| batch.observed_at).max()?;
    let complete = batches.iter().all(|batch| batch.complete);

    let mut charts = BTreeSet::new();
    let mut images = BTreeSet::new();
    for sample in batches.iter().flat_map(|batch| &batch.resources) {
        if let Some(chart) = sample
            .labels
            .get(HELM_CHART_LABEL)
            .and_then(|label| parse_helm_chart_label(label))
        {
            charts.insert(chart);
        }
        for image in &sample.images {
            images.insert(ObservedApplicationImage {
                workload: sample.raw_identity.clone(),
                container: image.name.clone(),
                image: image.image.clone(),
                digest: image.digest.clone(),
            });
        }
    }

    let (chart_name, chart_version) = if charts.len() == 1 {
        let (name, version) = charts.into_iter().next()?;
        (Some(name), Some(version))
    } else {
        (None, None)
    };
    if chart_name.is_none() && images.is_empty() {
        return None;
    }

    Some(ObservedApplicationReport {
        source: ObservedApplicationSource::Kubernetes,
        chart_name,
        chart_version,
        images: images.into_iter().collect(),
        complete,
        observed_at,
    })
}

/// Split a `helm.sh/chart` label into chart name and version. Chart names may
/// contain hyphens, so the version starts at the first hyphen followed by a
/// `MAJOR.MINOR.PATCH` version, which Helm requires of every chart. Helm
/// writes `+` as `_` in the label; the version is returned with `+` restored.
fn parse_helm_chart_label(label: &str) -> Option<(String, String)> {
    let label = label.trim();
    label
        .match_indices('-')
        .map(|(index, _)| (&label[..index], &label[index + 1..]))
        .find(|(name, version)| !name.is_empty() && is_chart_version(version))
        .map(|(name, version)| (name.to_string(), version.replace('_', "+")))
}

fn is_chart_version(value: &str) -> bool {
    let value = value.strip_prefix('v').unwrap_or(value);
    let core = value.split(['-', '_']).next().unwrap_or_default();
    let parts = core.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use alien_core::{
        ContainerImageIdentity, HeartbeatBackend, ObservedHealth, ObservedResourceSample,
        ProviderLifecycleState,
    };
    use chrono::{DateTime, Utc};

    use super::*;

    fn digest(fill: char) -> String {
        format!("sha256:{}", fill.to_string().repeat(64))
    }

    fn workload(
        raw_identity: &str,
        chart: Option<&str>,
        images: Vec<ContainerImageIdentity>,
    ) -> ObservedResourceSample {
        ObservedResourceSample {
            deployment_id: Some("dep_1".to_string()),
            raw_identity: raw_identity.to_string(),
            provider_kind: "apps/v1/Deployment".to_string(),
            display_name: raw_identity.rsplit(':').next().unwrap().to_string(),
            namespace: Some("shop".to_string()),
            region: None,
            scope: None,
            resource_type_hint: None,
            version: None,
            alien_resource_id: None,
            health: ObservedHealth::Healthy,
            lifecycle: ProviderLifecycleState::Running,
            message: None,
            partial: false,
            provider_stale: false,
            counts: None,
            collection_issues: vec![],
            labels: chart
                .map(|chart| BTreeMap::from([(HELM_CHART_LABEL.to_string(), chart.to_string())]))
                .unwrap_or_default(),
            attributes: BTreeMap::new(),
            raw: vec![],
            images,
        }
    }

    fn image(name: &str, image: &str, digest: Option<String>) -> ContainerImageIdentity {
        ContainerImageIdentity {
            name: name.to_string(),
            image: image.to_string(),
            digest,
        }
    }

    fn batch(
        platform: Platform,
        observed_at: &str,
        resources: Vec<ObservedResourceSample>,
    ) -> ObservedInventoryBatch {
        ObservedInventoryBatch {
            source_kind: "operator".to_string(),
            inventory_scope: "kubernetes:apps/v1:Deployment:shop".to_string(),
            controller_platform: platform,
            backend: HeartbeatBackend::Kubernetes,
            observed_at: observed_at.parse::<DateTime<Utc>>().unwrap(),
            complete: true,
            resources,
        }
    }

    #[test]
    fn parses_chart_names_with_hyphens_and_prerelease_versions() {
        assert_eq!(
            parse_helm_chart_label("shop-api-1.4.0"),
            Some(("shop-api".to_string(), "1.4.0".to_string()))
        );
        assert_eq!(
            parse_helm_chart_label("k8s-2fa-gateway-2.0.0-rc.1_build.7"),
            Some((
                "k8s-2fa-gateway".to_string(),
                "2.0.0-rc.1+build.7".to_string()
            ))
        );
        assert_eq!(
            parse_helm_chart_label("shop-v3.1.2"),
            Some(("shop".to_string(), "v3.1.2".to_string()))
        );
        assert_eq!(
            parse_helm_chart_label("shop-1.2-extra-3.0.0"),
            Some(("shop-1.2-extra".to_string(), "3.0.0".to_string()))
        );
        assert_eq!(parse_helm_chart_label("shop"), None);
        assert_eq!(parse_helm_chart_label("shop-latest"), None);
        assert_eq!(parse_helm_chart_label("-1.0.0"), None);
    }

    #[test]
    fn reports_the_shared_chart_and_every_distinct_image_by_workload() {
        let batches = vec![
            batch(
                Platform::Kubernetes,
                "2026-09-24T10:00:00Z",
                vec![
                    workload(
                        "apps/v1:Deployment:shop:api",
                        Some("shop-1.4.0"),
                        vec![
                            image(
                                "api",
                                "registry.example.com/shop/api:1.4.0",
                                Some(digest('a')),
                            ),
                            image("proxy", "envoyproxy/envoy:v1.31", None),
                        ],
                    ),
                    workload(
                        "apps/v1:Deployment:shop:worker",
                        Some("shop-1.4.0"),
                        vec![image(
                            "worker",
                            "registry.example.com/shop/worker:1.4.0",
                            Some(digest('b')),
                        )],
                    ),
                ],
            ),
            batch(
                Platform::Kubernetes,
                "2026-09-24T10:00:05Z",
                vec![workload(
                    "apps/v1:StatefulSet:shop:db",
                    None,
                    vec![image("db", "postgres:16", Some(digest('c')))],
                )],
            ),
        ];

        let report = observed_application(&batches).expect("application identity");

        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::json!({
                "source": "kubernetes",
                "chartName": "shop",
                "chartVersion": "1.4.0",
                "images": [
                    {
                        "workload": "apps/v1:Deployment:shop:api",
                        "container": "api",
                        "image": "registry.example.com/shop/api:1.4.0",
                        "digest": digest('a'),
                    },
                    {
                        "workload": "apps/v1:Deployment:shop:api",
                        "container": "proxy",
                        "image": "envoyproxy/envoy:v1.31",
                    },
                    {
                        "workload": "apps/v1:Deployment:shop:worker",
                        "container": "worker",
                        "image": "registry.example.com/shop/worker:1.4.0",
                        "digest": digest('b'),
                    },
                    {
                        "workload": "apps/v1:StatefulSet:shop:db",
                        "container": "db",
                        "image": "postgres:16",
                        "digest": digest('c'),
                    },
                ],
                "complete": true,
                "observedAt": "2026-09-24T10:00:05Z",
            })
        );
    }

    #[test]
    fn marks_the_report_incomplete_when_a_workload_kind_could_not_be_listed() {
        let mut unlisted_statefulsets = batch(Platform::Kubernetes, "2026-09-24T10:00:00Z", vec![]);
        unlisted_statefulsets.complete = false;
        let batches = vec![
            batch(
                Platform::Kubernetes,
                "2026-09-24T10:00:00Z",
                vec![workload(
                    "apps/v1:Deployment:shop:api",
                    Some("shop-1.4.0"),
                    vec![image("api", "shop/api:1.4.0", None)],
                )],
            ),
            unlisted_statefulsets,
        ];

        let report = observed_application(&batches).expect("listed workloads still report");

        assert!(!report.complete);
        assert_eq!(report.chart_name.as_deref(), Some("shop"));
        assert_eq!(report.images.len(), 1);
    }

    #[test]
    fn omits_the_chart_when_workloads_name_different_charts() {
        let batches = vec![batch(
            Platform::Kubernetes,
            "2026-09-24T10:00:00Z",
            vec![
                workload(
                    "apps/v1:Deployment:shop:api",
                    Some("shop-1.4.0"),
                    vec![image("api", "shop/api:1.4.0", None)],
                ),
                workload(
                    "apps/v1:StatefulSet:shop:redis",
                    Some("redis-19.0.1"),
                    vec![],
                ),
            ],
        )];

        let report = observed_application(&batches).expect("images still identify the app");

        assert_eq!(report.chart_name, None);
        assert_eq!(report.chart_version, None);
        assert_eq!(report.images.len(), 1);
    }

    #[test]
    fn omits_the_report_when_workloads_carry_no_chart_or_images() {
        let unlabelled = vec![batch(
            Platform::Kubernetes,
            "2026-09-24T10:00:00Z",
            vec![workload("apps/v1:Deployment:shop:api", None, vec![])],
        )];
        let cloud_only = vec![batch(
            Platform::Aws,
            "2026-09-24T10:00:00Z",
            vec![workload(
                "arn:aws:s3:::shop-assets",
                Some("shop-1.4.0"),
                vec![image("api", "shop/api:1.4.0", None)],
            )],
        )];

        assert_eq!(observed_application(&unlabelled), None);
        assert_eq!(observed_application(&cloud_only), None);
        assert_eq!(observed_application(&[]), None);
    }
}
