//! Repository names, references and upload session ids outside the OCI distribution grammar are
//! refused with 400 before anything reaches the upstream: a path that is not literal with
//! NAME_INVALID, and a literal path with the code of the part that failed.

use crate::harness::*;

/// Repository names outside the OCI grammar.
fn invalid_repositories() -> Vec<String> {
    vec![
        format!("{OWN}/../x"),
        format!("{OWN}/%2e%2e/x"),
        format!("{OWN}/%2E%2e/x"),
        format!("{OWN}/./x"),
        format!("{OWN}/%2e/x"),
        format!("{OWN}//x"),
        format!("{OWN}/%2e%2e%2fx"),
        "artifacts/Default-prj_a".to_string(),
    ]
}

async fn assert_refused(
    manager: &Manager,
    seen: &Seen,
    method: &str,
    target: &str,
    token: &str,
    code: &str,
) {
    let before = seen.lock().unwrap().len();
    let response = send(manager.port, method, target, token).await;
    assert_eq!(response.status, 400, "{method} {target}: {}", response.body);
    // A HEAD response has no body to read the code from.
    if method != "HEAD" {
        assert_eq!(response.error_code(), code, "{method} {target}");
    }
    assert!(
        reached_since(seen, before).is_empty(),
        "{method} {target} reached the upstream"
    );
}

#[tokio::test]
async fn pushes_to_names_outside_the_grammar_are_refused_before_upstream() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    for repo in invalid_repositories() {
        for (method, suffix) in [
            ("POST", "blobs/uploads/"),
            ("POST", "blobs/uploads"),
            ("PATCH", "blobs/uploads/session-1"),
            ("PUT", "blobs/uploads/session-1"),
            ("PUT", "manifests/v1"),
            ("PATCH", "uploads/session-1"),
        ] {
            let target = format!("/v2/{repo}/{suffix}");
            assert_refused(
                &manager,
                &seen,
                method,
                &target,
                "pusher-for-prj_a",
                "NAME_INVALID",
            )
            .await;
        }
    }
}

#[tokio::test]
async fn pulls_of_names_outside_the_grammar_are_refused_before_upstream() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    for repo in invalid_repositories() {
        for method in ["GET", "HEAD"] {
            for suffix in ["manifests/v1".to_string(), format!("blobs/{}", digest())] {
                let target = format!("/v2/{repo}/{suffix}");
                assert_refused(
                    &manager,
                    &seen,
                    method,
                    &target,
                    "deploy-for-prj_a",
                    "NAME_INVALID",
                )
                .await;
            }
        }
    }
}

#[tokio::test]
async fn references_outside_the_grammar_are_refused_with_the_code_of_the_part_that_failed() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    for (suffix, code) in [
        ("manifests/v1/../../x/manifests/v1", "NAME_INVALID"),
        ("manifests/..%2f..%2fx%2fmanifests%2fv1", "NAME_INVALID"),
        ("manifests/%2e%2e", "NAME_INVALID"),
        ("blobs/uploads/..", "NAME_INVALID"),
        ("blobs/uploads/%2e%2e", "NAME_INVALID"),
        ("uploads/%2e%2e", "NAME_INVALID"),
        ("manifests/-v1", "MANIFEST_INVALID"),
        ("manifests/.v1", "MANIFEST_INVALID"),
        ("manifests/sha256:", "DIGEST_INVALID"),
        ("blobs/not-a-digest", "DIGEST_INVALID"),
        ("blobs/SHA256:abc", "DIGEST_INVALID"),
    ] {
        let target = format!("/v2/{OWN}/{suffix}");
        assert_refused(&manager, &seen, "GET", &target, "deploy-for-prj_a", code).await;
        assert_refused(&manager, &seen, "PUT", &target, "pusher-for-prj_a", code).await;
    }
}

#[tokio::test]
async fn valid_names_reach_the_upstream_unchanged() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let digest = digest();
    for (method, target, token, expected) in [
        (
            "POST",
            format!("/v2/{OWN}/blobs/uploads/"),
            "pusher-for-prj_a",
            202,
        ),
        (
            "PUT",
            format!("/v2/{OWN}/manifests/v1.2_rc-1"),
            "pusher-for-prj_a",
            200,
        ),
        (
            "GET",
            format!("/v2/{OWN}/manifests/v1"),
            "deploy-for-prj_a",
            200,
        ),
        (
            "GET",
            format!("/v2/{OWN}/manifests/{digest}"),
            "deploy-for-prj_a",
            200,
        ),
        (
            "GET",
            format!("/v2/{OWN}/blobs/{digest}"),
            "deploy-for-prj_a",
            200,
        ),
        (
            "GET",
            format!("/v2/{OWN}/nested/image/manifests/v1"),
            "deploy-for-prj_a",
            200,
        ),
        (
            "PUT",
            format!("/v2/{OWN}/tags/manifests/v1"),
            "pusher-for-prj_a",
            200,
        ),
        (
            "GET",
            format!("/v2/{OWN}/manifests/referrers/manifests/v1"),
            "deploy-for-prj_a",
            200,
        ),
        (
            "POST",
            format!("/v2/{OWN}/uploads/blobs/uploads/"),
            "pusher-for-prj_a",
            202,
        ),
    ] {
        let before = seen.lock().unwrap().len();
        let response = send(manager.port, method, &target, token).await;
        assert_eq!(
            response.status, expected,
            "{method} {target}: {}",
            response.body
        );
        let reached = reached_since(&seen, before);
        assert_eq!(reached.len(), 1, "{method} {target}");
        assert_eq!(
            (reached[0].method.as_str(), reached[0].path.as_str()),
            (method, target.as_str())
        );
    }

    // The upload-init form without the trailing slash reaches upstream with it restored.
    let response = send(
        manager.port,
        "POST",
        &format!("/v2/{OWN}/blobs/uploads"),
        "pusher-for-prj_a",
    )
    .await;
    assert_eq!(response.status, 202, "{}", response.body);
    let reached = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(reached.path, format!("/v2/{OWN}/blobs/uploads/"));
}
