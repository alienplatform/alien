//! Helm charts served from the manager's OCI registry.
//!
//! Every release with a Kubernetes stack has an installable chart at
//! `oci://<manager>/charts/<stack-id>`. The chart comes pre-wired to this
//! manager: its management URL, the Operator image, and pod log collection
//! are defaults, so a customer installs it with a deployment-group token and
//! their own infrastructure values only.
//!
//! Charts are rendered on request from the release; rendering is
//! deterministic, so a release and an Operator image always yield the same
//! digests. The chart version names both, `1.<n>.<operator>` for the n-th
//! release, so a version never changes content and GitOps tools that pin or
//! compare versions see an Operator upgrade as a new, higher version:
//!
//! - `<n>` counts every release, oldest first, so a newer release sorts
//!   above an older one under any Operator image.
//! - `<operator>` is the Operator image's release version as one number,
//!   `MAJOR * 1_000_000 + MINOR * 1_000 + PATCH` (`v3.3.59` is `3003059`).
//!   An image tag that isn't a release version (`dev`, `latest`) is `0`:
//!   such a tag can move to other content, so its charts can't be immutable
//!   either.
//!
//! A manager serves only the versions for its own Operator image. Other
//! versions, including the `1.0.<n>` versions earlier managers served, are
//! unknown: their content can't be reproduced here, and serving another
//! chart under them is what made versions mutable.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

use alien_core::{DeploymentModel, Platform, StackSettings};
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
    traits::{deploy_cli_downloads, BundleSource, BundleSources, ReleaseRecord, SourceCredentials},
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
#[derive(Debug, Clone)]
pub struct ChartSettings {
    /// Operator image as configured, `repository:tag`.
    pub operator_image: String,
    /// When set, deployments pull the Operator image through this manager
    /// (`<manager>/alien-operator:<tag>`) and the manager passes pulls through
    /// to this upstream. Unset for an image without a registry (a local
    /// image), which charts reference as is.
    pub operator_upstream: Option<super::operator_image::UpstreamImage>,
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
        }
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

struct Blob {
    media_type: &'static str,
    bytes: Arc<Vec<u8>>,
}

/// Rendered blobs by digest (manifests, configs and chart archives). Content
/// addressed, so entries never go stale.
static BLOBS: LazyLock<Mutex<HashMap<String, Blob>>> = LazyLock::new(Default::default);

/// Digests of each rendered chart, by release ID and chart version.
static RENDERED: LazyLock<Mutex<HashMap<(String, String), ChartDigests>>> =
    LazyLock::new(Default::default);

/// The three blobs that make up one chart version.
#[derive(Clone)]
struct ChartDigests {
    manifest: String,
    config: String,
    chart: String,
}

impl ChartDigests {
    fn contains(&self, digest: &str) -> bool {
        [&self.manifest, &self.config, &self.chart]
            .iter()
            .any(|known| known.as_str() == digest)
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
            return visible_blob(state, &settings, &charts, method, reference);
        }
        let Some((version, release)) = charts.iter().find(|(version, _)| version == reference)
        else {
            return oci_error(
                StatusCode::NOT_FOUND,
                "MANIFEST_UNKNOWN",
                format!(
                    "chart '{name}' has no version '{reference}'; this manager serves \
                     versions 1.<release>.{} for its Operator image {}",
                    operator_number(settings.operator_tag()),
                    settings.operator_image
                ),
            );
        };
        return match render(state, &settings, release, version) {
            Ok(digests) => blob_response(method, &digests.manifest),
            Err(response) => response,
        };
    }

    if let Some(digest) = operation.strip_prefix("blobs/") {
        return visible_blob(state, &settings, &charts, method, digest);
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

/// Serve `digest` only if it belongs to one of `charts`, the charts this
/// caller may read. The blob cache is shared across callers, so a digest
/// alone must not grant access.
fn visible_blob(
    state: &AppState,
    settings: &ChartSettings,
    charts: &[(String, ReleaseRecord)],
    method: &Method,
    digest: &str,
) -> Response {
    for (version, release) in charts {
        match render(state, settings, release, version) {
            Ok(digests) if digests.contains(digest) => return blob_response(method, digest),
            Ok(_) => {}
            Err(response) => return response,
        }
    }
    oci_error(
        StatusCode::NOT_FOUND,
        "BLOB_UNKNOWN",
        format!("unknown digest {digest}"),
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

/// Render a release's chart once, cache its blobs, and return their digests.
fn render(
    state: &AppState,
    settings: &ChartSettings,
    release: &ReleaseRecord,
    version: &str,
) -> Result<ChartDigests, Response> {
    let key = (release.id.clone(), version.to_string());
    if let Some(digests) = rendered().get(&key) {
        return Ok(digests.clone());
    }
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

    let config_digest = store(CONFIG_MEDIA_TYPE, config.clone());
    let chart_digest = store(CHART_MEDIA_TYPE, archive.clone());
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
    let digests = ChartDigests {
        manifest: store(MANIFEST_MEDIA_TYPE, manifest),
        config: config_digest,
        chart: chart_digest,
    };
    rendered().insert(key, digests.clone());
    Ok(digests)
}

fn rendered() -> std::sync::MutexGuard<'static, HashMap<(String, String), ChartDigests>> {
    RENDERED
        .lock()
        .expect("rendered chart index is never held across a panic")
}

fn store(media_type: &'static str, bytes: Vec<u8>) -> String {
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
    blobs().entry(digest.clone()).or_insert(Blob {
        media_type,
        bytes: Arc::new(bytes),
    });
    digest
}

fn blob_response(method: &Method, digest: &str) -> Response {
    let Some((media_type, bytes)) = blobs()
        .get(digest)
        .map(|blob| (blob.media_type, blob.bytes.clone()))
    else {
        return oci_error(
            StatusCode::NOT_FOUND,
            "BLOB_UNKNOWN",
            format!("unknown digest {digest}"),
        );
    };
    let mut response = if *method == Method::HEAD {
        Response::new(Body::empty())
    } else {
        Response::new(Body::from(bytes.as_ref().clone()))
    };
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(media_type));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(bytes.len()));
    if let Ok(value) = HeaderValue::from_str(digest) {
        headers.insert("docker-content-digest", value);
    }
    response
}

fn blobs() -> std::sync::MutexGuard<'static, HashMap<String, Blob>> {
    BLOBS
        .lock()
        .expect("chart blob cache is never held across a panic")
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
