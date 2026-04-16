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
    if parts.len() == 2 && (parts[0].contains('.') || parts[0].contains(':')) {
        Some(parts[1].to_string())
    } else {
        // No registry host prefix — return as-is
        Some(image_uri.to_string())
    }
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
