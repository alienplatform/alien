//! The request sequences push and pull clients send reach the upstream unchanged: a push with a
//! self-mount probe, monolithic and chunked uploads on signed sessions, GAR upload sessions on
//! `/artifacts-uploads/` and on GAR's own package path, and pulls by tag, sha256 and sha512 digest.

use sha2::Digest;

use crate::harness::*;

/// Repository names under the harness's local registry route, in the shapes clients push.
fn repositories() -> Vec<(String, &'static str)> {
    vec![
        (OWN.to_string(), "pusher-for-prj_a"),
        ("artifacts/default".to_string(), "pusher-for-default"),
        (
            "artifacts/default-prj_0a1b2c3d4e5f6g7h8i9j0k1l2m3n".to_string(),
            "pusher-for-prj_0a1b2c3d4e5f6g7h8i9j0k1l2m3n",
        ),
        (format!("{OWN}/api/worker.v2"), "pusher-for-prj_a"),
        (format!("{OWN}/a__b/c-d/e---f"), "pusher-for-prj_a"),
    ]
}

/// The request as the upstream received it, and the response status.
async fn exchange(
    manager: &Manager,
    seen: &Seen,
    method: &str,
    target: &str,
    token: Option<&str>,
) -> (u16, Reached, Response) {
    let before = seen.lock().unwrap().len();
    let response = raw(manager.port, method, target, token, b"").await;
    let reached = reached_since(seen, before);
    assert_eq!(reached.len(), 1, "{method} {target}: {}", response.body);
    (response.status, reached[0].clone(), response)
}

#[tokio::test]
async fn push_and_pull_sequences_reach_the_upstream_unchanged() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let sha256 = digest();
    let sha512 = format!("sha512:{}", "b".repeat(128));
    for (repo, token) in repositories() {
        // The self-mount probe a client sends before each layer upload.
        let probe = format!(
            "/v2/{repo}/blobs/uploads/?mount={sha256}&from={}",
            enc(&repo)
        );
        let (status, reached, _) = exchange(&manager, &seen, "POST", &probe, Some(token)).await;
        assert_eq!(status, 201, "{probe}");
        assert_eq!(reached.path, format!("/v2/{repo}/blobs/uploads/"));
        let mut pairs = reached.pairs();
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("from".to_string(), repo.clone()),
                ("mount".to_string(), sha256.clone())
            ]
        );

        // Upload init, then a chunk and the closing PUT on the signed session, without Bearer.
        let init = format!("/v2/{repo}/blobs/uploads/");
        let (status, _, response) = exchange(&manager, &seen, "POST", &init, Some(token)).await;
        assert_eq!(status, 202, "{init}");
        let session = response.location(manager.port);
        let (session_path, _) = session.split_once('?').unwrap();
        assert_eq!(session_path, format!("/v2/{repo}/blobs/uploads/session-1"));
        let (status, reached, _) = exchange(&manager, &seen, "PATCH", &session, None).await;
        assert_eq!((status, reached.path.as_str()), (200, session_path));
        let close = format!("{session}&digest={sha256}");
        let (status, reached, _) = exchange(&manager, &seen, "PUT", &close, None).await;
        assert_eq!((status, reached.path.as_str()), (200, session_path));
        assert_eq!(
            reached.pairs(),
            vec![("digest".to_string(), sha256.clone())]
        );

        for (method, rest) in [
            ("PUT", "manifests/api-3k9x2m1q".to_string()),
            ("HEAD", "manifests/api-3k9x2m1q".to_string()),
            ("GET", "manifests/latest".to_string()),
            ("GET", format!("manifests/{sha256}")),
            ("GET", format!("manifests/{sha512}")),
            ("HEAD", format!("blobs/{sha256}")),
            ("GET", format!("blobs/{sha512}")),
            ("GET", "tags/list".to_string()),
            ("GET", format!("referrers/{sha256}")),
        ] {
            let target = format!("/v2/{repo}/{rest}");
            let (status, reached, _) =
                exchange(&manager, &seen, method, &target, Some(token)).await;
            assert_eq!(status, 200, "{method} {target}");
            assert_eq!(reached.path, target, "{method}");
        }
    }
}

/// A GAR upload session: the init's `/artifacts-uploads/` Location is signed, and a chunk and the
/// closing PUT with `digest` reach the upstream on that path.
#[tokio::test]
async fn gar_upload_session_sequences_reach_the_upstream_unchanged() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let sha256 = digest();
    let init = format!("/v2/{OWN}/blobs/uploads/?gar=1");
    let (status, _, response) =
        exchange(&manager, &seen, "POST", &init, Some("pusher-for-prj_a")).await;
    assert_eq!(status, 202);
    let session = response.location(manager.port);
    let (session_path, _) = session.split_once('?').unwrap();
    assert_eq!(
        session_path,
        "/artifacts-uploads/namespaces/artifacts/repositories/default-prj_a/uploads/AJ-x_y=="
    );
    let (status, reached, _) =
        exchange(&manager, &seen, "PATCH", &session, Some("pusher-for-prj_a")).await;
    assert_eq!((status, reached.path.as_str()), (200, session_path));
    let close = format!("{session}&digest={sha256}");
    let (status, reached, _) =
        exchange(&manager, &seen, "PUT", &close, Some("pusher-for-prj_a")).await;
    assert_eq!((status, reached.path.as_str()), (200, session_path));
    assert_eq!(reached.pairs(), vec![("digest".to_string(), sha256)]);
}

/// GAR issues `/v2/` upload sessions under its own package path, not the repository the init
/// named. The signed session is followed on that path.
#[tokio::test]
async fn signed_sessions_on_the_registrys_own_repository_path_reach_the_upstream() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let issued = format!("/v2/{OWN}/pkg/blobs/uploads/Gq7sessionId0001");
    let init = format!("/v2/{OWN}/blobs/uploads/?location={}", enc(&issued));
    let (status, _, response) =
        exchange(&manager, &seen, "POST", &init, Some("pusher-for-prj_a")).await;
    assert_eq!(status, 202);
    let session = response.location(manager.port);
    assert!(session.starts_with(&format!("{issued}?")), "{session}");
    for method in ["PATCH", "PUT"] {
        let (status, reached, _) = exchange(&manager, &seen, method, &session, None).await;
        assert_eq!(
            (status, reached.path.as_str()),
            (200, issued.as_str()),
            "{method}"
        );
    }
}

fn sha256_of(content: &[u8]) -> String {
    format!("sha256:{}", hex::encode(sha2::Sha256::digest(content)))
}

/// Against a real distribution registry (`REGISTRY_UPSTREAM=localhost:<port>`, `registry:2`): a
/// chunked upload whose session Location carries the registry's `_state` query completes through
/// the proxy, a manifest referencing it is pushed, and both are pulled by tag and digest.
#[tokio::test]
#[ignore]
async fn distribution_registry_completes_chunked_uploads_and_pulls_by_tag_and_digest() {
    let upstream = std::env::var("REGISTRY_UPSTREAM").expect("set REGISTRY_UPSTREAM");
    let manager = start_manager(upstream).await;
    let client = reqwest::Client::new();
    let repo = format!("{OWN}/api/worker.v2");
    let pusher = "pusher-for-prj_a";

    let absolute = |location: &str| {
        if location.starts_with('/') {
            format!("{}{location}", manager.url)
        } else {
            location.to_string()
        }
    };
    let location =
        |response: &reqwest::Response| absolute(response.headers()["location"].to_str().unwrap());

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let layer: Vec<u8> = format!("layer {nonce} ").repeat(4096).into_bytes();
    let layer_digest = sha256_of(&layer);

    let init = client
        .post(format!("{}/v2/{repo}/blobs/uploads/", manager.url))
        .bearer_auth(pusher)
        .send()
        .await
        .unwrap();
    assert_eq!(init.status(), 202);
    let mut session = location(&init);
    assert!(session.contains("_state="), "{session}");
    assert!(session.contains("_alien_sig="), "{session}");

    let (first, second) = layer.split_at(layer.len() / 2);
    for (offset, chunk) in [(0, first), (first.len(), second)] {
        let patch = client
            .patch(&session)
            .header("content-type", "application/octet-stream")
            .header(
                "content-range",
                format!("{offset}-{}", offset + chunk.len() - 1),
            )
            .body(chunk.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(patch.status(), 202, "{session}");
        session = location(&patch);
    }
    let close = client
        .put(format!("{session}&digest={layer_digest}"))
        .send()
        .await
        .unwrap();
    assert_eq!(close.status(), 201, "{:?}", close.text().await);

    let config =
        br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#;
    let config_digest = sha256_of(config);
    let init = client
        .post(format!("{}/v2/{repo}/blobs/uploads/", manager.url))
        .bearer_auth(pusher)
        .send()
        .await
        .unwrap();
    assert_eq!(init.status(), 202);
    let put = client
        .put(format!("{}&digest={config_digest}", location(&init)))
        .body(config.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), 201);

    let manifest = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": config_digest,
            "size": config.len(),
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": layer_digest,
            "size": layer.len(),
        }],
    }))
    .unwrap();
    let manifest_digest = sha256_of(&manifest);
    let put = client
        .put(format!("{}/v2/{repo}/manifests/api-3k9x2m1q", manager.url))
        .bearer_auth(pusher)
        .header("content-type", "application/vnd.oci.image.manifest.v1+json")
        .body(manifest.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), 201, "{:?}", put.text().await);

    let pull = |path: String| {
        let client = client.clone();
        let url = format!("{}/v2/{repo}/{path}", manager.url);
        async move {
            client
                .get(url)
                .bearer_auth("deploy-for-prj_a")
                .header("accept", "application/vnd.oci.image.manifest.v1+json")
                .send()
                .await
                .unwrap()
        }
    };
    for reference in ["api-3k9x2m1q", manifest_digest.as_str()] {
        let response = pull(format!("manifests/{reference}")).await;
        assert_eq!(response.status(), 200, "{reference}");
        assert_eq!(
            response.bytes().await.unwrap().as_ref(),
            manifest.as_slice()
        );
    }
    let response = pull(format!("blobs/{layer_digest}")).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.bytes().await.unwrap().as_ref(), layer.as_slice());
    let head = client
        .head(format!("{}/v2/{repo}/blobs/{layer_digest}", manager.url))
        .bearer_auth("deploy-for-prj_a")
        .send()
        .await
        .unwrap();
    assert_eq!(head.status(), 200);
}
