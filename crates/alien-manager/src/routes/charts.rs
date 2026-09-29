//! Helm charts served from the manager's OCI registry.
//!
//! Every release with a Kubernetes stack has an installable chart at
//! `oci://<manager>/charts/<stack-id>`, version `1.0.<n>` for the n-th
//! release. The chart comes pre-wired to this manager: its management URL,
//! the Operator image, and pod log collection are defaults, so a customer
//! installs it with a deployment-group token and their own infrastructure
//! values only.
//!
//! Charts are rendered on request from the release; rendering is
//! deterministic, so the same release always yields the same digests.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

use alien_core::{DeploymentModel, Platform, StackSettings};
use alien_helm::{
    apply_package_defaults, generate_helm_chart, package_chart, HelmOptions, HelmRegistry,
    LogCollectorDefault, OperatorImageDefault, PackageDefaults,
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
    traits::ReleaseRecord,
};

/// Repository namespace for charts in the manager's registry.
pub const CHART_NAMESPACE: &str = "charts/";

const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const CONFIG_MEDIA_TYPE: &str = "application/vnd.cncf.helm.config.v1+json";
const CHART_MEDIA_TYPE: &str = "application/vnd.cncf.helm.chart.content.v1.tar+gzip";

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

    let charts = match chart_releases(state, name).await {
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
            if !cached(reference) {
                // A digest this process hasn't rendered yet: render the
                // releases' charts so the digest can be found.
                for (version, release) in &charts {
                    if let Err(response) = render(state, &settings, release, version) {
                        return response;
                    }
                }
            }
            return blob_response(method, reference);
        }
        let Some((version, release)) = charts.iter().find(|(version, _)| version == reference)
        else {
            return oci_error(
                StatusCode::NOT_FOUND,
                "MANIFEST_UNKNOWN",
                format!("chart '{name}' has no version '{reference}'"),
            );
        };
        return match render(state, &settings, release, version) {
            Ok(manifest_digest) => blob_response(method, &manifest_digest),
            Err(response) => response,
        };
    }

    if let Some(digest) = operation.strip_prefix("blobs/") {
        if !cached(digest) {
            for (version, release) in &charts {
                if let Err(response) = render(state, &settings, release, version) {
                    return response;
                }
            }
        }
        return blob_response(method, digest);
    }

    oci_error(StatusCode::NOT_FOUND, "NAME_UNKNOWN", "unknown chart path")
}

/// Releases whose Kubernetes stack is named `name`, oldest first, with the
/// chart version each one publishes.
async fn chart_releases(
    state: &AppState,
    name: &str,
) -> Result<Vec<(String, ReleaseRecord)>, Response> {
    let mut releases = state
        .release_store
        .list_releases(&Subject::system())
        .await
        .map_err(|e| {
            oci_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                format!("failed to list releases: {e}"),
            )
        })?;
    releases.sort_by_key(|release| release.created_at);
    Ok(releases
        .into_iter()
        .enumerate()
        .filter(|(_, release)| {
            release
                .stacks
                .get(&Platform::Kubernetes)
                .is_some_and(|stack| stack.id() == name)
        })
        .map(|(index, release)| (format!("1.0.{}", index + 1), release))
        .collect())
}

/// Render a release's chart, cache its blobs, and return the manifest digest.
fn render(
    state: &AppState,
    settings: &ChartSettings,
    release: &ReleaseRecord,
    version: &str,
) -> Result<String, Response> {
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
    let (repository, tag) = match &settings.operator_upstream {
        Some(upstream) => (proxied_repository.as_str(), upstream.tag.as_str()),
        None => split_image(&settings.operator_image),
    };
    let chart = apply_package_defaults(
        chart,
        &PackageDefaults {
            version: Some(version),
            management_url: Some(&management_url),
            operator_image: Some(OperatorImageDefault { repository, tag }),
            log_collector: Some(LogCollectorDefault::PodApi),
            // Pulls through the manager authenticate with the install token.
            registry_pull_secret: settings.operator_upstream.is_some(),
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
    Ok(store(MANIFEST_MEDIA_TYPE, manifest))
}

fn store(media_type: &'static str, bytes: Vec<u8>) -> String {
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
    blobs().entry(digest.clone()).or_insert(Blob {
        media_type,
        bytes: Arc::new(bytes),
    });
    digest
}

fn cached(digest: &str) -> bool {
    blobs().contains_key(digest)
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
    use super::split_image;

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
