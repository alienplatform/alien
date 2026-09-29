//! A request authorized for project A reaches the upstream on A's path exactly as parsed, or not
//! at all. Signed upload sessions are bound to the path and repo they were signed for.

use crate::harness::*;

/// Paths whose separators only appear after decoding or normalization, as the text after `/v2/`.
/// Names, references and session ids outside the grammar are covered in `names`.
fn paths_outside_the_grammar() -> Vec<(&'static str, String)> {
    vec![
        ("encoded slash", format!("{OWN}%2f..%2fx/manifests/v1")),
        ("encoded backslash", format!("{OWN}%5c..%5cx/manifests/v1")),
        ("semicolon", format!("{OWN}/..;/x/manifests/v1")),
        (
            "non-ASCII",
            format!("{OWN}/%ef%bc%8e%ef%bc%8e/x/manifests/v1"),
        ),
        ("no operation", "_catalog".to_string()),
    ]
}

const METHODS: [(&str, &str); 5] = [
    ("GET", "deploy-for-prj_a"),
    ("HEAD", "deploy-for-prj_a"),
    ("POST", "pusher-for-prj_a"),
    ("PUT", "pusher-for-prj_a"),
    ("PATCH", "pusher-for-prj_a"),
];

#[tokio::test]
async fn paths_outside_the_grammar_are_refused_and_clean_paths_reach_the_upstream() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;

    let clean = format!("/v2/{OWN}/manifests/v1");
    for (method, token) in METHODS {
        let before = seen.lock().unwrap().len();
        let response = send(manager.port, method, &clean, token).await;
        assert_eq!(response.status, 200, "{method} {clean}: {}", response.body);
        let reached = reached_since(&seen, before);
        assert_eq!(reached.len(), 1, "{method} {clean}");
        assert_eq!(reached[0].path, clean, "{method}");
    }

    for (rule, path) in paths_outside_the_grammar() {
        for (method, token) in METHODS {
            let before = seen.lock().unwrap().len();
            let response = send(manager.port, method, &format!("/v2/{path}"), token).await;
            assert_eq!(response.status, 400, "{rule}: {method} {path}");
            if method != "HEAD" {
                assert_eq!(
                    response.error_code(),
                    "NAME_INVALID",
                    "{rule}: {method} {path}"
                );
            }
            assert!(
                reached_since(&seen, before).is_empty(),
                "{rule}: {method} {path} reached the upstream"
            );
        }
    }
}

/// Signs a GAR-style `/artifacts-uploads/` session for A and returns its (path, query).
async fn signed_gar_session(manager: &Manager, variant: &str) -> (String, String) {
    let init = send(
        manager.port,
        "POST",
        &format!("/v2/{OWN}/blobs/uploads/?gar={variant}"),
        "pusher-for-prj_a",
    )
    .await;
    assert_eq!(init.status, 202, "{}", init.body);
    let signed = init.location(manager.port);
    assert!(signed.contains("_alien_sig="), "{signed}");
    let (path, query) = signed.split_once('?').unwrap();
    (path.to_string(), query.to_string())
}

/// `/artifacts-uploads/` forwards the raw path, so a segment that is a dot segment or holds an
/// encoded separator is refused with BLOB_UPLOAD_INVALID before the signature is read.
#[tokio::test]
async fn artifacts_upload_paths_with_dot_or_separator_segments_are_refused() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let (signed_path, signed_query) = signed_gar_session(&manager, "1").await;

    let before = seen.lock().unwrap().len();
    let signed = format!("{signed_path}?{signed_query}");
    let response = send(manager.port, "PATCH", &signed, "pusher-for-prj_a").await;
    assert_eq!(response.status, 200, "{}", response.body);
    let reached = reached_since(&seen, before);
    assert_eq!(reached.len(), 1);
    assert_eq!(reached[0].path, signed_path);

    let base = "/artifacts-uploads/namespaces/artifacts/repositories";
    for path in [
        format!("{base}/default-prj_a/../x/uploads/AJ-x_y=="),
        format!("{base}/default-prj_a/%2e%2e/x/uploads/AJ-x_y=="),
        format!("{base}/default-prj_a/.%2E/x/uploads/AJ-x_y=="),
        format!("{base}/default-prj_a%2f..%2fx/uploads/AJ-x_y=="),
        format!("{base}/default-prj_a%5c..%5cx/uploads/AJ-x_y=="),
        format!("{base}//x/uploads/AJ-x_y=="),
        format!("{base}/default-prj_a/uploads/AJ-x_y==/.."),
        format!("{base}/default-prj_a/uploads/AJ-x_y==%3fx"),
        "/artifacts-uploads/%2e%2e/v2/x/blobs/uploads/".to_string(),
    ] {
        for method in ["PUT", "PATCH", "POST"] {
            let before = seen.lock().unwrap().len();
            let target = format!("{path}?{signed_query}");
            let response = send(manager.port, method, &target, "pusher-for-prj_a").await;
            assert_eq!(response.status, 400, "{method} {path}: {}", response.body);
            assert_eq!(
                response.error_code(),
                "BLOB_UPLOAD_INVALID",
                "{method} {path}"
            );
            assert!(
                reached_since(&seen, before).is_empty(),
                "{method} {path} reached the upstream"
            );
        }
    }

    // The signed path with an altered repo in its query.
    for query in [
        signed_query.replace("default-prj_a", "default-prj_b"),
        format!("{signed_query}&_alien_repo={}", enc(OTHER)),
    ] {
        let before = seen.lock().unwrap().len();
        let response = send(
            manager.port,
            "PATCH",
            &format!("{signed_path}?{query}"),
            "pusher-for-prj_a",
        )
        .await;
        assert!(
            (400..500).contains(&response.status),
            "{query}: {}",
            response.status
        );
        assert!(reached_since(&seen, before).is_empty(), "{query}");
    }
}

/// A session Location the upstream issues with an encoded separator in a segment is signed like
/// any other, and still refused when the client follows it.
#[tokio::test]
async fn signed_artifacts_upload_sessions_with_encoded_separators_are_refused() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    for variant in ["encoded-slash", "encoded-backslash"] {
        let (path, query) = signed_gar_session(&manager, variant).await;
        for method in ["PUT", "PATCH"] {
            let before = seen.lock().unwrap().len();
            let response = send(
                manager.port,
                method,
                &format!("{path}?{query}"),
                "pusher-for-prj_a",
            )
            .await;
            assert_eq!(
                response.status, 400,
                "{variant} {method}: {}",
                response.body
            );
            assert_eq!(
                response.error_code(),
                "BLOB_UPLOAD_INVALID",
                "{variant} {method}"
            );
            assert!(
                reached_since(&seen, before).is_empty(),
                "{variant} {method} reached the upstream"
            );
        }
    }
}

/// A signed `/v2/.../blobs/uploads/<id>` session is bound to its path and repo: any altered path or
/// repo is refused, and the unaltered session reaches the signed path.
#[tokio::test]
async fn signed_v2_sessions_accept_only_the_signed_path_and_repo() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let init = send(
        manager.port,
        "POST",
        &format!("/v2/{OWN}/blobs/uploads/"),
        "pusher-for-prj_a",
    )
    .await;
    assert_eq!(init.status, 202, "{}", init.body);
    let signed = init.location(manager.port);
    let (signed_path, signed_query) = signed.split_once('?').unwrap();

    let before = seen.lock().unwrap().len();
    let response = raw(manager.port, "PUT", &signed, None, b"").await;
    assert_eq!(response.status, 200, "{}", response.body);
    let reached = reached_since(&seen, before);
    assert_eq!(reached.len(), 1);
    assert_eq!(reached[0].path, signed_path);

    let altered = [
        (
            signed_path.to_string(),
            signed_query.replace("default-prj_a", "default-prj_b"),
        ),
        (
            signed_path.to_string(),
            signed_query.replace(
                "_alien_repo=artifacts%2Fdefault-prj_a",
                "_alien_repo=artifacts%2Fdefault-prj_a%2F..%2Fx",
            ),
        ),
        (
            signed_path.to_string(),
            format!("{signed_query}&_alien_repo={}", enc(OTHER)),
        ),
        (
            signed_path.to_string(),
            signed_query.replace(
                "_alien_repo=artifacts%2Fdefault-prj_a",
                "_alien_repo=ARTIFACTS%2FDEFAULT-PRJ_A",
            ),
        ),
        (
            signed_path.replace("default-prj_a", "default-prj_b"),
            signed_query.to_string(),
        ),
        (
            signed_path.replace("default-prj_a/", "default-prj_a/../x/"),
            signed_query.to_string(),
        ),
        (
            signed_path.replace("default-prj_a/", "default-prj_a/%2e%2e/x/"),
            signed_query.to_string(),
        ),
        (format!("{signed_path}/.."), signed_query.to_string()),
        (format!("{signed_path}%2f..%2fx"), signed_query.to_string()),
    ];
    for (path, query) in altered {
        for method in ["PUT", "PATCH", "POST"] {
            let before = seen.lock().unwrap().len();
            let target = format!("{path}?{query}");
            let response = raw(manager.port, method, &target, None, b"").await;
            assert!(
                (400..500).contains(&response.status),
                "{method} {target}: {}",
                response.status
            );
            assert!(
                reached_since(&seen, before).is_empty(),
                "{method} {target} reached the upstream"
            );
        }
    }
}

/// A session Location the upstream issues on another registry route than the init was authorized
/// for is signed, and refused when the client follows it.
#[tokio::test]
async fn signed_sessions_are_refused_off_the_signed_route() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    for (issued, token) in [
        ("/v2/my-gcp/alien-repo/prj_a/blobs/uploads/AJ-x_y==", None),
        (
            "/artifacts-uploads/namespaces/elsewhere/repositories/x/uploads/AJ-x_y==",
            Some("pusher-for-prj_a"),
        ),
    ] {
        let init = send(
            manager.port,
            "POST",
            &format!("/v2/{OWN}/blobs/uploads/?location={}", enc(issued)),
            "pusher-for-prj_a",
        )
        .await;
        assert_eq!(init.status, 202, "{issued}: {}", init.body);
        let signed = init.location(manager.port);
        assert!(signed.starts_with(&format!("{issued}?")), "{signed}");
        assert!(signed.contains("_alien_sig="), "{signed}");

        for method in ["PUT", "PATCH"] {
            let before = seen.lock().unwrap().len();
            let response = raw(manager.port, method, &signed, token, b"").await;
            assert_eq!(response.status, 403, "{method} {issued}: {}", response.body);
            assert_eq!(response.error_code(), "DENIED", "{method} {issued}");
            assert!(
                reached_since(&seen, before).is_empty(),
                "{method} {issued} reached the upstream"
            );
        }
    }
}
