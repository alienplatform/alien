//! Helm charts served from the manager's OCI registry.
//!
//! Every release with a Kubernetes stack has an installable chart at
//! `oci://<manager>/charts/<stack-id>`. The chart comes pre-wired to this
//! manager: its management URL, the Operator image, and pod log collection
//! are defaults, so a customer installs it with a deployment-group token and
//! their own infrastructure values only.
//!
//! A chart is rendered the first time its version is served, and the manager
//! keeps its manifest, config and archive bytes in the release store. From
//! then on it serves the kept bytes, so a version keeps its digest across
//! manager upgrades. The chart version names the release and the Operator
//! image, `1.<n>.<operator>` for the n-th release, so GitOps tools that pin
//! or compare versions see an Operator upgrade as a new, higher version:
//!
//! - `<n>` counts every release, oldest first, so a newer release sorts
//!   above an older one under any Operator image.
//! - `<operator>` is the Operator image's release version as one number,
//!   `MAJOR * 1_000_000 + MINOR * 1_000 + PATCH` (`v3.3.59` is `3003059`).
//!   An image tag that isn't a release version (`dev`, `latest`) is `0`:
//!   such a tag can move to other content, so its charts can't be immutable
//!   either.
//!
//! A manager renders only versions for its own Operator image, and serves
//! versions of other Operator images only from its store (they were served by
//! an earlier manager). The `1.0.<n>` versions earlier managers served were
//! never kept, so they are unknown: serving another chart under them is what
//! made versions mutable.

use std::sync::Arc;

use alien_core::{DeploymentModel, Platform, Stack, StackSettings};
use alien_helm::{
    apply_package_defaults, generate_helm_chart, package_chart, HelmOptions, HelmRegistry,
    LogCollectorDefault, OperatorImageDefault, PackageDefaults, SetupDefault,
};
use axum::{
    body::Body,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};

use super::AppState;
use crate::{
    auth::{Scope, Subject},
    traits::{
        deploy_cli_downloads, BundleSource, BundleSources, ReleaseRecord, SourceCredentials,
        StoredChart,
    },
};

/// Repository namespace for charts in the manager's registry.
pub const CHART_NAMESPACE: &str = "charts/";

const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const CONFIG_MEDIA_TYPE: &str = "application/vnd.cncf.helm.config.v1+json";
const CHART_MEDIA_TYPE: &str = "application/vnd.cncf.helm.chart.content.v1.tar+gzip";

/// Version of the setup fingerprint served charts carry: the release's
/// [`alien_core::Stack::setup_update_digest`]. A deployment's first report
/// records it, and later reports with another version are refused, so it
/// changes only with what the fingerprint means.
const SETUP_FINGERPRINT_VERSION: u32 = 1;

/// Chart settings for a manager that serves charts.
#[derive(Clone)]
pub struct ChartSettings {
    /// Operator image as configured, `repository:tag`.
    pub operator_image: String,
    /// When set, deployments pull the Operator image through this manager
    /// (`<manager>/alien-operator:<tag>`) and the manager passes pulls through
    /// to this upstream. Unset for an image without a registry (a local
    /// image), which charts reference as is.
    pub operator_upstream: Option<super::operator_image::UpstreamImage>,
    /// Checks the enrollment code an operator pulls its own image with
    /// before it enrolls. Set by the builder from the identity verifier.
    pub enrollment_codes: Option<Arc<dyn crate::auth::identity_exchange::RuntimeIdentityVerifier>>,
}

impl std::fmt::Debug for ChartSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartSettings")
            .field("operator_image", &self.operator_image)
            .field("operator_upstream", &self.operator_upstream)
            .field("enrollment_codes", &self.enrollment_codes.is_some())
            .finish()
    }
}

impl ChartSettings {
    /// Settings for `operator_image` (the published Operator image matching
    /// this manager's version when `None`).
    pub fn new(operator_image: Option<String>, insecure_registry: bool) -> Self {
        let operator_image = operator_image.unwrap_or_else(|| {
            format!(
                "ghcr.io/alienplatform/alien-operator:v{}",
                env!("CARGO_PKG_VERSION")
            )
        });
        let has_registry = operator_image.split_once('/').is_some_and(|(host, _)| {
            host.contains('.') || host.contains(':') || host == "localhost"
        });
        let operator_upstream = has_registry
            .then(|| {
                super::operator_image::UpstreamImage::parse(&operator_image, insecure_registry)
            })
            .flatten();
        Self {
            operator_image,
            operator_upstream,
            enrollment_codes: None,
        }
    }

    /// What to tell an Operator that is too old for this manager: the Operator
    /// this manager serves and the chart versions that carry it, with how to
    /// move to one. `None` for the Operator version in use when unknown.
    pub fn operator_upgrade_message(&self, current: Option<&str>) -> String {
        let current = current.map_or("this Operator".to_string(), |v| format!("Operator {v}"));
        format!(
            "{current} is too old for this deployment. Operator {tag} accepts it, and chart \
             versions 1.<release>.{number} carry it: run `helm upgrade <release> <chart> \
             --version 1.<release>.{number}`, or raise the chart version in your GitOps source \
             (Flux `semver: \"1.x\"` and Argo CD `targetRevision: \"1.*\"` pick it up by themselves).",
            tag = self.operator_tag(),
            number = operator_number(self.operator_tag()),
        )
    }

    /// Whether an Operator at `reported` (`v3.3.40` or `3.3.40`) is older than
    /// the Operator image this manager's charts install. `None` when either
    /// isn't a release version (`dev`, `latest`), which can't be compared.
    pub fn operator_behind_target(&self, reported: &str) -> Option<bool> {
        let reported = operator_number(reported);
        let target = operator_number(self.operator_tag());
        (reported != 0 && target != 0).then_some(reported < target)
    }

    /// Tag of the Operator image charts install.
    fn operator_tag(&self) -> &str {
        match &self.operator_upstream {
            Some(upstream) => &upstream.tag,
            None => split_image(&self.operator_image).1,
        }
    }

    /// The Operator image reference deployments run, given this manager's
    /// public URL: through the manager when an upstream is set.
    pub fn deployed_operator_image(&self, manager_url: &str) -> String {
        match &self.operator_upstream {
            Some(upstream) => format!(
                "{}/{}:{}",
                alien_core::image_rewrite::strip_url_scheme(manager_url),
                super::operator_image::OPERATOR_REPOSITORY,
                upstream.tag
            ),
            None => self.operator_image.clone(),
        }
    }
}

/// Serve an OCI read for a path under [`CHART_NAMESPACE`].
pub async fn serve(state: &AppState, subject: &Subject, method: &Method, path: &str) -> Response {
    if matches!(
        subject.scope,
        Scope::Commands { .. } | Scope::RemoteBindings { .. } | Scope::Telemetry { .. }
    ) {
        return oci_error(StatusCode::FORBIDDEN, "DENIED", "token cannot pull charts");
    }
    let Some(settings) = state.charts.clone() else {
        return oci_error(
            StatusCode::NOT_FOUND,
            "NAME_UNKNOWN",
            "charts are not served here",
        );
    };

    let rest = &path[CHART_NAMESPACE.len()..];
    let (name, operation) = match rest.split_once('/') {
        Some(parts) => parts,
        None => return oci_error(StatusCode::NOT_FOUND, "NAME_UNKNOWN", "unknown chart path"),
    };

    let charts = match chart_releases(state, &settings, subject, name).await {
        Ok(charts) => charts,
        Err(response) => return response,
    };
    if charts.is_empty() {
        return oci_error(
            StatusCode::NOT_FOUND,
            "NAME_UNKNOWN",
            format!("no release has a Kubernetes stack named '{name}'"),
        );
    }

    if operation == "tags/list" {
        let tags: Vec<_> = charts.iter().map(|(version, _)| version.clone()).collect();
        return axum::Json(serde_json::json!({ "name": format!("charts/{name}"), "tags": tags }))
            .into_response();
    }

    if let Some(reference) = operation.strip_prefix("manifests/") {
        if reference.starts_with("sha256:") {
            return visible_blob(state, subject, &charts, method, reference).await;
        }
        let Some((version, release)) = charts.iter().find(|(version, _)| version == reference)
        else {
            // A version of an earlier Operator image is served from the store.
            return match kept_version(state, subject, &charts, reference).await {
                Ok(Some(chart)) => blob_response(method, MANIFEST_MEDIA_TYPE, &chart.manifest),
                Ok(None) => unknown_version(&settings, name, reference),
                Err(response) => response,
            };
        };
        return match chart_version(state, &settings, subject, release, version).await {
            Ok(chart) => blob_response(method, MANIFEST_MEDIA_TYPE, &chart.manifest),
            Err(response) => response,
        };
    }
    if let Some(digest) = operation.strip_prefix("blobs/") {
        return visible_blob(state, subject, &charts, method, digest).await;
    }

    oci_error(StatusCode::NOT_FOUND, "NAME_UNKNOWN", "unknown chart path")
}

/// Bundle sources for `release` from this manager's own registry: the chart
/// version it serves for the release. `None` when this manager doesn't serve
/// charts.
pub(crate) async fn served_bundle_sources(
    state: &AppState,
    subject: &Subject,
    release: &ReleaseRecord,
) -> Result<Option<BundleSources>, Response> {
    let Some(settings) = state.charts.as_deref() else {
        return Ok(None);
    };
    let Some(stack) = release.stacks.get(&Platform::Kubernetes) else {
        return Err(oci_error(
            StatusCode::BAD_REQUEST,
            "NAME_UNKNOWN",
            format!("release {} has no Kubernetes stack", release.id),
        ));
    };
    let name = stack.id().to_string();
    let charts = chart_releases(state, settings, subject, &name).await?;
    let Some((version, _)) = charts.iter().find(|(_, r)| r.id == release.id) else {
        return Err(oci_error(
            StatusCode::NOT_FOUND,
            "MANIFEST_UNKNOWN",
            format!("no chart for release {}", release.id),
        ));
    };
    let base_url = state.config.base_url();
    let host = alien_core::image_rewrite::strip_url_scheme(&base_url);
    Ok(Some(BundleSources {
        chart: BundleSource {
            reference: format!("{host}/{CHART_NAMESPACE}{name}:{version}"),
            credentials: SourceCredentials::Caller,
        },
        deploy_cli: deploy_cli_downloads(&state.config.releases_url()),
    }))
}

/// The chart version this manager serves for `release_id`, when it serves
/// charts and the release has one.
pub(crate) async fn served_chart_version(
    state: &AppState,
    subject: &Subject,
    release: &ReleaseRecord,
) -> Option<String> {
    let settings = state.charts.clone()?;
    let name = release.stacks.get(&Platform::Kubernetes)?.id().to_string();
    let charts = chart_releases(state, &settings, subject, &name)
        .await
        .ok()?;
    charts
        .into_iter()
        .find(|(_, r)| r.id == release.id)
        .map(|(version, _)| version)
}

/// Serve `digest` only if it belongs to a chart kept for one of `charts`'
/// releases, the charts this caller may read. A digest alone must not grant
/// access. A digest comes from a manifest, which is kept when first served.
async fn visible_blob(
    state: &AppState,
    subject: &Subject,
    charts: &[(String, ReleaseRecord)],
    method: &Method,
    digest: &str,
) -> Response {
    let release_ids: Vec<String> = charts.iter().map(|(_, r)| r.id.clone()).collect();
    let kept = match state
        .release_store
        .stored_charts(subject, &release_ids)
        .await
    {
        Ok(kept) => kept,
        Err(e) => return store_failed("read kept charts", e),
    };
    for chart in &kept {
        for (media_type, bytes) in [
            (MANIFEST_MEDIA_TYPE, &chart.manifest),
            (CONFIG_MEDIA_TYPE, &chart.config),
            (CHART_MEDIA_TYPE, &chart.archive),
        ] {
            if sha256_digest(bytes) == digest {
                return blob_response(method, media_type, bytes);
            }
        }
    }
    oci_error(
        StatusCode::NOT_FOUND,
        "BLOB_UNKNOWN",
        format!("unknown digest {digest}"),
    )
}

/// The kept chart for `reference`, a version of an Operator image this
/// manager doesn't serve now: it belongs to the release with the same number
/// among `charts`.
async fn kept_version(
    state: &AppState,
    subject: &Subject,
    charts: &[(String, ReleaseRecord)],
    reference: &str,
) -> Result<Option<StoredChart>, Response> {
    let release_part = |version: &str| {
        version
            .rsplit_once('.')
            .map(|(release, _)| release.to_string())
    };
    let Some(wanted) = release_part(reference) else {
        return Ok(None);
    };
    let Some((_, release)) = charts
        .iter()
        .find(|(version, _)| release_part(version).as_ref() == Some(&wanted))
    else {
        return Ok(None);
    };
    let kept = state
        .release_store
        .stored_charts(subject, std::slice::from_ref(&release.id))
        .await
        .map_err(|e| store_failed("read kept charts", e))?;
    Ok(kept.into_iter().find(|chart| chart.version == reference))
}

fn unknown_version(settings: &ChartSettings, name: &str, reference: &str) -> Response {
    oci_error(
        StatusCode::NOT_FOUND,
        "MANIFEST_UNKNOWN",
        format!(
            "chart '{name}' has no version '{reference}'; this manager serves \
             versions 1.<release>.{} for its Operator image {}, and versions it served \
             before. Versions 1.0.<n> from earlier managers are no longer served: pin \
             1.<release>.<operator> instead, or a range such as 1.x",
            operator_number(settings.operator_tag()),
            settings.operator_image
        ),
    )
}

fn store_failed(action: &str, error: alien_error::AlienError) -> Response {
    oci_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "UNKNOWN",
        format!("failed to {action}: {error}"),
    )
}

/// Releases the caller may read whose Kubernetes stack is named `name`,
/// oldest first, with the chart version each one publishes.
async fn chart_releases(
    state: &AppState,
    settings: &ChartSettings,
    subject: &Subject,
    name: &str,
) -> Result<Vec<(String, ReleaseRecord)>, Response> {
    let mut releases = state
        .release_store
        .list_releases(subject)
        .await
        .map_err(|e| {
            oci_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                format!("failed to list releases: {e}"),
            )
        })?;
    releases.retain(|release| state.authz.can_read_release(subject, release));
    releases.sort_by_key(|release| release.created_at);
    let operator = operator_number(settings.operator_tag());
    Ok(releases
        .into_iter()
        .enumerate()
        .filter(|(_, release)| {
            release
                .stacks
                .get(&Platform::Kubernetes)
                .is_some_and(|stack| stack.id() == name)
        })
        .map(|(index, release)| (format!("1.{}.{operator}", index + 1), release))
        .collect())
}

/// The chart of `release` at `version`: the kept copy, or, the first time
/// the version is served, a render that is kept before it is returned. The
/// store keeps the first copy when managers race, and every one returns it.
async fn chart_version(
    state: &AppState,
    settings: &ChartSettings,
    subject: &Subject,
    release: &ReleaseRecord,
    version: &str,
) -> Result<StoredChart, Response> {
    let kept = state
        .release_store
        .stored_charts(subject, std::slice::from_ref(&release.id))
        .await
        .map_err(|e| store_failed("read kept charts", e))?;
    if let Some(chart) = kept.into_iter().find(|chart| chart.version == version) {
        return Ok(chart);
    }
    let rendered = render(state, settings, release, version).await?;
    state
        .release_store
        .store_chart_if_absent(subject, rendered)
        .await
        .map_err(|e| store_failed("keep the chart", e))
}

/// The stack after the preparation every Kubernetes deployment applies, with no cloud queries.
async fn prepare_stack(
    stack: &Stack,
    stack_settings: &StackSettings,
) -> alien_preflights::error::Result<Stack> {
    let config = alien_core::DeploymentConfig::builder()
        .stack_settings(stack_settings.clone())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(alien_core::ExternalBindings::default())
        .build();
    alien_preflights::runner::PreflightRunner::new()
        .apply_mutations(
            stack.clone(),
            &alien_core::StackState::new(Platform::Kubernetes),
            &config,
        )
        .await
}

/// Render a release's chart.
async fn render(
    state: &AppState,
    settings: &ChartSettings,
    release: &ReleaseRecord,
    version: &str,
) -> Result<StoredChart, Response> {
    let render_failed =
        |message: String| oci_error(StatusCode::INTERNAL_SERVER_ERROR, "UNKNOWN", message);
    let stack = release
        .stacks
        .get(&Platform::Kubernetes)
        .expect("chart_releases only returns releases with a Kubernetes stack");
    let stack_settings = StackSettings {
        // Charts install the Operator, which always pulls from the manager.
        deployment_model: DeploymentModel::Pull,
        ..Default::default()
    };
    // The chart carries what the Operator cannot create itself, such as the service accounts
    // of resources Alien adds to the stack (the sandbox egress gateway), so it is generated
    // from the prepared stack, the same one `alien render` prints.
    let prepared = prepare_stack(stack, &stack_settings)
        .await
        .map_err(|e| render_failed(format!("failed to prepare the stack: {e}")))?;
    let stack = &prepared;
    let chart = generate_helm_chart(
        stack,
        HelmOptions {
            registry: &HelmRegistry::built_in(),
            stack_settings,
            chart_name: stack.id().to_string(),
        },
    )
    .map_err(|e| render_failed(format!("failed to generate chart: {e}")))?;

    let management_url = state.config.base_url();
    let registry_host = alien_core::image_rewrite::strip_url_scheme(&management_url).to_string();
    let proxied_repository = format!(
        "{registry_host}/{}",
        super::operator_image::OPERATOR_REPOSITORY
    );
    let repository = match &settings.operator_upstream {
        Some(_) => proxied_repository.as_str(),
        None => split_image(&settings.operator_image).0,
    };
    let tag = settings.operator_tag();
    // The setup the chart carries lets a `helm upgrade` to it complete a
    // pending setup update: the Operator reports it and the manager authorizes
    // the change. A setup change also changes the Operator's pod template, so
    // the Operator restarts with it.
    let setup_fingerprint = stack.setup_update_digest();
    let chart = apply_package_defaults(
        chart,
        &PackageDefaults {
            version: Some(version),
            management_url: Some(&management_url),
            operator_image: Some(OperatorImageDefault { repository, tag }),
            log_collector: Some(LogCollectorDefault::PodApi),
            // Pulls through the manager authenticate with the install token.
            registry_pull_secret: settings.operator_upstream.is_some(),
            setup: Some(SetupDefault {
                release_id: &release.id,
                target: Platform::Kubernetes.as_str(),
                fingerprint: &setup_fingerprint,
                fingerprint_version: SETUP_FINGERPRINT_VERSION,
            }),
        },
    )
    .map_err(|e| render_failed(format!("failed to apply chart defaults: {e}")))?;
    let archive = package_chart(&chart)
        .map_err(|e| render_failed(format!("failed to package chart: {e}")))?;

    let config = serde_json::json!({
        "apiVersion": "v2",
        "name": chart.name,
        "version": version,
        "appVersion": version,
        "type": "application",
        "description": format!("Deployment chart for {}", stack.id()),
    })
    .to_string()
    .into_bytes();

    let config_digest = sha256_digest(&config);
    let chart_digest = sha256_digest(&archive);
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": MANIFEST_MEDIA_TYPE,
        "config": {
            "mediaType": CONFIG_MEDIA_TYPE,
            "digest": config_digest,
            "size": config.len(),
        },
        "layers": [{
            "mediaType": CHART_MEDIA_TYPE,
            "digest": chart_digest,
            "size": archive.len(),
            "annotations": { "org.opencontainers.image.title": format!("{}-{version}.tgz", chart.name) },
        }],
        "annotations": {
            "org.opencontainers.image.version": version,
            "org.opencontainers.image.revision": release.id,
        },
    })
    .to_string()
    .into_bytes();
    Ok(StoredChart {
        release_id: release.id.clone(),
        version: version.to_string(),
        manifest,
        config,
        archive,
    })
}

fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn blob_response(method: &Method, media_type: &'static str, bytes: &[u8]) -> Response {
    let mut response = if *method == Method::HEAD {
        Response::new(Body::empty())
    } else {
        Response::new(Body::from(bytes.to_vec()))
    };
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(media_type));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(bytes.len()));
    if let Ok(value) = HeaderValue::from_str(&sha256_digest(bytes)) {
        headers.insert("docker-content-digest", value);
    }
    response
}

/// The Operator release version in `tag` (`v3.3.59` or `3.3.59`) as one
/// number that sorts like the version: `3003059`. `0` for a tag that isn't a
/// release version, or whose minor or patch doesn't fit in three digits.
fn operator_number(tag: &str) -> u64 {
    let numbers: Option<Vec<u64>> = tag
        .strip_prefix('v')
        .unwrap_or(tag)
        .split('.')
        .map(|part| {
            let digits =
                !part.is_empty() && part.len() <= 6 && part.bytes().all(|b| b.is_ascii_digit());
            digits.then(|| part.parse().ok()).flatten()
        })
        .collect();
    match numbers.as_deref() {
        Some(&[major, minor, patch]) if minor < 1_000 && patch < 1_000 => {
            major * 1_000_000 + minor * 1_000 + patch
        }
        _ => 0,
    }
}

/// Split `repository:tag` (the tag after the last `:` that follows the last `/`).
fn split_image(image: &str) -> (&str, &str) {
    let name_start = image.rfind('/').map_or(0, |i| i + 1);
    match image[name_start..].rfind(':') {
        Some(i) => (&image[..name_start + i], &image[name_start + i + 1..]),
        None => (image, "latest"),
    }
}

fn oci_error(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    let body = serde_json::json!({ "errors": [{ "code": code, "message": message.into() }] });
    (status, axum::Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::{operator_number, split_image};

    #[test]
    fn operator_numbers_sort_like_operator_versions() {
        assert_eq!(operator_number("v3.3.59"), 3_003_059);
        assert_eq!(operator_number("3.3.59"), 3_003_059);
        assert_eq!(operator_number("v0.0.1"), 1);
        let ordered = [
            "v3.3.57", "v3.3.59", "v3.3.999", "v3.4.0", "v3.10.2", "v4.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(
                operator_number(pair[0]) < operator_number(pair[1]),
                "{} must sort below {}",
                pair[0],
                pair[1]
            );
        }
        for tag in [
            "dev",
            "latest",
            "sha-0123abc",
            "v3.3",
            "v3.3.59.1",
            "v3.3.59-rc.1",
            "v3.3.1000",
            "v3.1000.0",
            "v+3.3.59",
            "v3..59",
            "v1234567.0.0",
        ] {
            assert_eq!(operator_number(tag), 0, "{tag}");
        }
    }

    #[test]
    fn splits_image_references_with_registry_ports() {
        assert_eq!(
            split_image("host.example.com:5050/alienplatform/alien-operator:1.2.3"),
            (
                "host.example.com:5050/alienplatform/alien-operator",
                "1.2.3"
            )
        );
        assert_eq!(
            split_image("host.example.com:5050/alien-operator"),
            ("host.example.com:5050/alien-operator", "latest")
        );
        assert_eq!(split_image("alien-operator:dev"), ("alien-operator", "dev"));
    }
}

#[cfg(test)]
mod upgrade_message_tests {
    use super::ChartSettings;

    #[test]
    fn the_message_names_the_operator_and_the_chart_versions_that_carry_it() {
        let settings = ChartSettings::new(
            Some("ghcr.io/alienplatform/alien-operator:v3.3.59".to_string()),
            false,
        );
        let message = settings.operator_upgrade_message(Some("v3.3.40"));
        assert!(message.contains("Operator v3.3.40"), "{message}");
        assert!(message.contains("Operator v3.3.59 accepts it"), "{message}");
        assert!(message.contains("1.<release>.3003059"), "{message}");
        assert!(
            message.contains("--version 1.<release>.3003059"),
            "{message}"
        );
        assert!(settings
            .operator_upgrade_message(None)
            .starts_with("this Operator is too old"));
    }
}
