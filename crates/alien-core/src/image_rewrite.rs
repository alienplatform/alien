//! Image URI utilities for the registry proxy.
//!
//! The manager IS the container registry — its `/v2/` endpoint serves images.
//! Releases store proxy URIs (e.g., `manager.alien.dev/my-app:v1`).
//! Only Lambda and Cloud Run need native registry URIs (ECR/GAR).

/// Strip the registry hostname from an image URI, returning just the repo path.
///
/// Input: `us-central1-docker.pkg.dev/project/repo:tag`
/// Output: `project/repo:tag`
///
/// Input: `manager.alien.dev/my-app:v1`
/// Output: `my-app:v1`
pub fn strip_registry_host(image_uri: &str) -> Option<String> {
    // Skip local paths
    if image_uri.starts_with('/') || image_uri.starts_with("./") {
        return None;
    }

    // OCI image reference format: [host[:port]/]path[:tag|@digest]
    // The host always contains a dot or colon (to distinguish from a path component).
    let parts: Vec<&str> = image_uri.splitn(2, '/').collect();
    if parts.len() == 2
        && (parts[0].contains('.') || parts[0].contains(':') || parts[0] == "localhost")
    {
        Some(parts[1].to_string())
    } else {
        // No registry host prefix — return as-is
        Some(image_uri.to_string())
    }
}

/// Registry host and repository an image with repository path `repository`
/// is stored at under a site registry `registry_prefix` (`host[/path]`).
///
/// One rule for every path that moves images to a site registry: `alien-deploy
/// sync --registry` pushes with it and the Operator rewrites workload images
/// with it, so both always agree on where an image lives.
pub fn site_repository(registry_prefix: &str, repository: &str) -> (String, String) {
    match registry_prefix.trim_end_matches('/').split_once('/') {
        Some((host, path)) => (host.to_string(), format!("{path}/{repository}")),
        None => (
            registry_prefix.trim_end_matches('/').to_string(),
            repository.to_string(),
        ),
    }
}

/// The registry-less repository path of an image reference: no host, no tag,
/// no digest. Docker Hub official images gain the `library/` namespace, as
/// registries store them.
fn repository_path(image: &str) -> Option<(String, &str)> {
    let path = strip_registry_host(image)?;
    let had_host = path.len() != image.len();
    let (repository, reference) = match path.find('@') {
        Some(at) => (
            path[..at].to_string(),
            &image[image.len() - (path.len() - at)..],
        ),
        None => match path.rfind(':') {
            Some(colon) if !path[colon..].contains('/') => (
                path[..colon].to_string(),
                &image[image.len() - (path.len() - colon)..],
            ),
            _ => (path.clone(), ""),
        },
    };
    if repository.is_empty() {
        return None;
    }
    let repository = if !had_host && !repository.contains('/') {
        format!("library/{repository}")
    } else {
        repository
    };
    Some((repository, reference))
}

/// `image` moved to the site registry `registry_prefix`, keeping its tag or
/// digest. Local paths come back unchanged.
pub fn rewrite_to_site_registry(image: &str, registry_prefix: &str) -> String {
    let prefix = registry_prefix.trim_end_matches('/');
    // Already stored under the site registry: rewriting again would nest the prefix.
    if image
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('/'))
    {
        return image.to_string();
    }
    let Some((repository, reference)) = repository_path(image) else {
        return image.to_string();
    };
    let (host, repository) = site_repository(registry_prefix, &repository);
    format!("{host}/{repository}{reference}")
}

/// Strip URL scheme and trailing slash from a URL, returning just host[:port].
pub fn strip_url_scheme(url: &str) -> &str {
    let trimmed = url.trim_end_matches('/');
    trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed)
}

/// Resolve a proxy image URI to a native registry URI.
///
/// Used by Lambda (ECR) and Cloud Run (GAR) which require native registries.
/// Strips the proxy host from the URI and prepends the native registry host,
/// preserving the full repo path and tag/digest.
///
/// Input:  `manager.alien.dev/alien-e2e:fn-abc123`, `123456.dkr.ecr.us-east-1.amazonaws.com`
/// Output: `123456.dkr.ecr.us-east-1.amazonaws.com/alien-e2e:fn-abc123`
///
/// Input:  `manager.alien.dev/project/repo/default:fn-abc123`, `us-central1-docker.pkg.dev`
/// Output: `us-central1-docker.pkg.dev/project/repo/default:fn-abc123`
pub fn resolve_native_image_uri(
    proxy_image_uri: &str,
    native_registry_host: &str,
) -> Option<String> {
    // Strip the proxy host to get /repo-path:tag or /repo-path@sha256:...
    let repo_and_ref = strip_registry_host(proxy_image_uri)?;
    if repo_and_ref.is_empty() {
        return None;
    }
    Some(format!(
        "{}/{}",
        native_registry_host.trim_end_matches('/'),
        repo_and_ref
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_registry_host_gar() {
        assert_eq!(
            strip_registry_host("us-central1-docker.pkg.dev/project/repo:tag"),
            Some("project/repo:tag".to_string())
        );
    }

    #[test]
    fn test_strip_registry_host_ecr() {
        assert_eq!(
            strip_registry_host("123456.dkr.ecr.us-east-1.amazonaws.com/repo:tag"),
            Some("repo:tag".to_string())
        );
    }

    #[test]
    fn test_strip_registry_host_proxy() {
        assert_eq!(
            strip_registry_host("manager.alien.dev/my-app:v1"),
            Some("my-app:v1".to_string())
        );
    }

    #[test]
    fn test_strip_registry_host_localhost() {
        assert_eq!(
            strip_registry_host("localhost:5000/my-app:v1"),
            Some("my-app:v1".to_string())
        );
    }

    #[test]
    fn test_strip_registry_host_no_host() {
        assert_eq!(
            strip_registry_host("simple-image:latest"),
            Some("simple-image:latest".to_string())
        );
    }

    #[test]
    fn test_strip_registry_host_local_path() {
        assert_eq!(strip_registry_host("/local/path/to/image.tar"), None);
    }

    #[test]
    fn test_strip_url_scheme() {
        assert_eq!(strip_url_scheme("https://example.com"), "example.com");
        assert_eq!(strip_url_scheme("http://localhost:8080"), "localhost:8080");
        assert_eq!(strip_url_scheme("https://example.com/"), "example.com");
        assert_eq!(strip_url_scheme("manager:8080"), "manager:8080");
    }

    #[test]
    fn test_resolve_native_image_uri_ecr() {
        // ECR: proxy host stripped, repo path preserved, native host prepended
        assert_eq!(
            resolve_native_image_uri(
                "manager.alien.dev/alien-e2e:fn-abc123",
                "123456.dkr.ecr.us-east-1.amazonaws.com"
            ),
            Some("123456.dkr.ecr.us-east-1.amazonaws.com/alien-e2e:fn-abc123".to_string())
        );
    }

    #[test]
    fn test_resolve_native_image_uri_gar() {
        // GAR: 3-segment repo path preserved from proxy URI
        assert_eq!(
            resolve_native_image_uri(
                "manager.alien.dev/project/repo/default:my-fn-abc123",
                "us-central1-docker.pkg.dev"
            ),
            Some("us-central1-docker.pkg.dev/project/repo/default:my-fn-abc123".to_string())
        );
    }

    #[test]
    fn test_resolve_native_image_uri_digest() {
        assert_eq!(
            resolve_native_image_uri(
                "manager.alien.dev/alien-e2e@sha256:abcdef123456",
                "123456.dkr.ecr.us-east-1.amazonaws.com"
            ),
            Some(
                "123456.dkr.ecr.us-east-1.amazonaws.com/alien-e2e@sha256:abcdef123456".to_string()
            )
        );
    }
}

#[cfg(test)]
mod site_registry_tests {
    use super::*;

    #[test]
    fn rewrites_the_registry_and_keeps_the_digest() {
        assert_eq!(
            rewrite_to_site_registry(
                "manager.example.com/team/app@sha256:abc",
                "mirror.local:5000"
            ),
            "mirror.local:5000/team/app@sha256:abc"
        );
    }

    #[test]
    fn a_mirror_path_prefixes_the_repository() {
        assert_eq!(
            rewrite_to_site_registry("ghcr.io/org/app:1.2", "mirror.local/alien"),
            "mirror.local/alien/org/app:1.2"
        );
    }

    #[test]
    fn docker_hub_names_follow_the_registry_layout() {
        assert_eq!(
            rewrite_to_site_registry("alpine/k8s:1.32.0", "mirror.local"),
            "mirror.local/alpine/k8s:1.32.0"
        );
        assert_eq!(
            rewrite_to_site_registry("nginx:1.27", "mirror.local"),
            "mirror.local/library/nginx:1.27"
        );
    }

    #[test]
    fn a_registry_port_is_not_a_tag() {
        assert_eq!(
            rewrite_to_site_registry("localhost:5000/app", "mirror.local"),
            "mirror.local/app"
        );
    }

    #[test]
    fn bare_localhost_is_a_registry_host() {
        assert_eq!(
            rewrite_to_site_registry("localhost/team/app:1", "mirror.local"),
            "mirror.local/team/app:1"
        );
    }

    #[test]
    fn rewriting_twice_does_not_nest_the_prefix() {
        let once = rewrite_to_site_registry("ghcr.io/org/app:1", "mirror.local/alien");
        assert_eq!(rewrite_to_site_registry(&once, "mirror.local/alien"), once);
    }

    #[test]
    fn a_tag_only_image_from_an_older_release_keeps_its_tag() {
        assert_eq!(
            rewrite_to_site_registry("manager.example.com/team/app:v3", "mirror.local"),
            "mirror.local/team/app:v3"
        );
    }

    #[test]
    fn without_a_site_registry_the_release_image_is_used_as_is() {
        let config = crate::DeploymentConfig::builder()
            .stack_settings(crate::StackSettings::default())
            .environment_variables(crate::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(crate::ExternalBindings::default())
            .build();
        assert_eq!(
            config.workload_image("manager.example.com/app:v3"),
            "manager.example.com/app:v3"
        );
    }

    #[test]
    fn local_paths_are_left_alone() {
        assert_eq!(
            rewrite_to_site_registry("/tmp/build/app", "mirror.local"),
            "/tmp/build/app"
        );
    }
}
