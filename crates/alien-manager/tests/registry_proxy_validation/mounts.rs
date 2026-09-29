//! A cross-repository blob mount reaches the upstream only on the upload-init POST, only with a
//! valid digest, and only when its source is on the pushed repository's registry route and
//! pullable by the caller. Any other mount becomes a plain upload.

use std::process::Command;

use sha2::Digest;

use crate::harness::*;

/// Mount queries the proxy refuses: a source outside the pushed project, or no valid digest, in
/// each encoding. None may carry a mount parameter upstream.
fn mount_queries_outside_the_project(digest: &str, port: u16) -> Vec<String> {
    vec![
        format!("mount={digest}&from={}", enc(OTHER)),
        format!("mount={digest}&from={OTHER}"),
        format!("mount={digest}&mount={digest}&from={}", enc(OTHER)),
        format!("from={}&mount={digest}&from={}", enc(OTHER), enc(OTHER)),
        format!("mount={digest}&from={}", enc(&format!("{OWN}/../x"))),
        format!("mount={digest}&from={}", enc("artifacts/default-prj_a.b")),
        format!("mount={digest}&from={}", enc("artifacts/default-prj_a-b")),
        format!("mount={digest}&from={}", enc("artifacts/DEFAULT-prj_b")),
        format!("mount={digest}&from={}", enc("Artifacts/default-prj_a")),
        format!("mount={digest}&from="),
        format!("mount={digest}&from"),
        format!("mount={digest}"),
        "mount=&from=".to_string(),
        format!("mount={digest}&from=artifacts%252Fdefault-prj_b"),
        format!("mount={digest}&from=artifacts%5Cdefault-prj_b"),
        format!(
            "mount={digest}&from={}",
            enc(&format!("127.0.0.1:{port}/{OTHER}"))
        ),
        format!("mount={digest}&from={}", enc(&format!("localhost/{OTHER}"))),
        format!("mount={digest}&from={}", enc(&format!("/{OTHER}"))),
        format!("mount={digest}&from={}", enc(&format!("{OTHER}/"))),
        format!("m%6Funt={digest}&fr%6Fm={}", enc(OTHER)),
        format!("x=1;mount={digest};from={}", enc(OTHER)),
        format!("x=1%26mount%3D{digest}%26from%3D{}", enc(OTHER)),
        format!("from={}", enc(OTHER)),
        format!("Mount={digest}&From={}", enc(OTHER)),
        format!("MOUNT={digest}&FROM={}", enc(OTHER)),
        format!("mount={digest}&from={}&origin=registry.example", enc(OTHER)),
        format!("mount=not-a-digest&from={}", enc(OWN)),
    ]
}

#[tokio::test]
async fn mount_sources_outside_the_pushed_project_become_plain_uploads() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    for token in ["deploy-for-prj_a", "group-for-prj_a"] {
        for init in [
            format!("/v2/{OWN}/blobs/uploads/"),
            format!("/v2/{OWN}/blobs/uploads"),
        ] {
            for query in mount_queries_outside_the_project(&digest(), manager.port) {
                let before = seen.lock().unwrap().len();
                let response = send(manager.port, "POST", &format!("{init}?{query}"), token).await;
                let reached = reached_since(&seen, before);
                assert_eq!(response.status, 202, "{token} {query}: {}", response.body);
                assert_eq!(reached.len(), 1, "{token} {query}");
                assert!(
                    !reached[0].has_mount_params(),
                    "{token} {query}: {reached:?}"
                );
                // A separator inside a value stays inside that value upstream.
                if let Some(rest) = query.strip_prefix("x=1") {
                    let x = reached[0]
                        .pairs()
                        .into_iter()
                        .find(|(k, _)| k == "x")
                        .map(|(_, v)| v);
                    let rest = urlencoding::decode(rest).unwrap();
                    assert_eq!(x.as_deref(), Some(format!("1{rest}").as_str()), "{query}");
                }
            }
        }
    }
}

/// Mounts within the pushed project are forwarded for a caller that may pull the project, and
/// become plain uploads for a push-only caller.
#[tokio::test]
async fn mounts_within_the_pushed_project_follow_the_callers_pull_access() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let digest = digest();
    for (token, pullable) in [
        ("deploy-for-prj_a", true),
        ("pusher-for-prj_a", true),
        ("group-for-prj_a", false),
    ] {
        for (target, from) in [
            (OWN.to_string(), OWN.to_string()),
            (format!("{OWN}/copy"), OWN.to_string()),
            (OWN.to_string(), format!("{OWN}/nested")),
        ] {
            let response = send(
                manager.port,
                "POST",
                &format!(
                    "/v2/{target}/blobs/uploads/?mount={digest}&from={}",
                    enc(&from)
                ),
                token,
            )
            .await;
            let mut pairs = seen.lock().unwrap().last().unwrap().pairs();
            pairs.sort();
            if pullable {
                assert_eq!(
                    response.status, 201,
                    "{token} {from} -> {target}: {}",
                    response.body
                );
                assert_eq!(
                    pairs,
                    vec![
                        ("from".to_string(), from.clone()),
                        ("mount".to_string(), digest.clone())
                    ],
                    "{token} {from} -> {target}"
                );
            } else {
                assert_eq!(
                    response.status, 202,
                    "{token} {from} -> {target}: {}",
                    response.body
                );
                assert!(pairs.is_empty(), "{token} {from} -> {target}: {pairs:?}");
            }
        }
    }
}

/// The upstream resolves `from` in its own registry, so `origin` never reaches it: not on an
/// allowed mount, and not on a refused one.
#[tokio::test]
async fn origin_is_never_forwarded() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let digest = digest();
    let init = format!("/v2/{OWN}/blobs/uploads/");
    for origin in ["origin", "Origin", "ORIGIN"] {
        let allowed = send(
            manager.port,
            "POST",
            &format!(
                "{init}?mount={digest}&from={}&{origin}=registry.example",
                enc(OWN)
            ),
            "deploy-for-prj_a",
        )
        .await;
        assert_eq!(allowed.status, 201, "{origin}: {}", allowed.body);
        let mut pairs = seen.lock().unwrap().last().unwrap().pairs();
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("from".to_string(), OWN.to_string()),
                ("mount".to_string(), digest.clone())
            ],
            "{origin}"
        );

        for query in [
            format!(
                "mount={digest}&from={}&{origin}=registry.example",
                enc(OTHER)
            ),
            format!("{origin}=registry.example"),
        ] {
            let refused = send(
                manager.port,
                "POST",
                &format!("{init}?{query}"),
                "deploy-for-prj_a",
            )
            .await;
            assert_eq!(refused.status, 202, "{query}: {}", refused.body);
            let reached = seen.lock().unwrap().last().cloned().unwrap();
            assert!(!reached.has_mount_params(), "{query}: {reached:?}");
        }
    }
}

/// Only the lowercase `mount` and `from` keys make a mount request: the upstream receives either
/// exactly those two keys, naming the pushed project, or no mount keys at all.
#[tokio::test]
async fn only_the_lowercase_mount_keys_are_forwarded() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let digest = digest();
    for (query, forwarded) in [
        // A repeated key keeps its last value.
        (
            format!("mount={digest}&from={}&from={}", enc(OWN), enc(OTHER)),
            false,
        ),
        (
            format!("mount={digest}&from={}&from={}", enc(OTHER), enc(OWN)),
            true,
        ),
        (
            format!("mount={digest}&from={}&FROM={}", enc(OWN), enc(OTHER)),
            true,
        ),
        (
            format!("mount={digest}&From={}&from={}", enc(OTHER), enc(OWN)),
            true,
        ),
        (format!("mount={digest}&From={}", enc(OWN)), false),
        (format!("Mount={digest}&from={}", enc(OWN)), false),
        (
            format!("MOUNT={digest}&mount={digest}&from={}", enc(OWN)),
            true,
        ),
    ] {
        let response = send(
            manager.port,
            "POST",
            &format!("/v2/{OWN}/blobs/uploads/?{query}"),
            "deploy-for-prj_a",
        )
        .await;
        let expected = if forwarded { 201 } else { 202 };
        assert_eq!(response.status, expected, "{query}: {}", response.body);
        let pairs = seen.lock().unwrap().last().unwrap().pairs();
        let mut mount_keys: Vec<(String, String)> = pairs
            .into_iter()
            .filter(|(k, _)| {
                ["mount", "from", "origin"]
                    .iter()
                    .any(|p| k.eq_ignore_ascii_case(p))
            })
            .collect();
        mount_keys.sort();
        let want = if forwarded {
            vec![
                ("from".to_string(), OWN.to_string()),
                ("mount".to_string(), digest.clone()),
            ]
        } else {
            vec![]
        };
        assert_eq!(mount_keys, want, "{query}");
    }
}

/// A mount follows the pull rule: a caller that may pull the source may mount from it. A
/// deployment token pulls only its own project and a deployment-group token pulls nothing, so
/// their mounts from B become plain uploads; the project-scoped token's mount matches its pull.
#[tokio::test]
async fn mounts_follow_the_pull_rule() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let digest = digest();
    for token in ["pusher-for-prj_a", "deploy-for-prj_a", "group-for-prj_a"] {
        let pull = send(
            manager.port,
            "GET",
            &format!("/v2/{OTHER}/blobs/{digest}"),
            token,
        )
        .await;
        let may_pull = pull.status == 200;
        if token != "pusher-for-prj_a" {
            assert!(!may_pull, "{token} pulled {OTHER}: {}", pull.status);
        }
        let mount = send(
            manager.port,
            "POST",
            &format!(
                "/v2/{OWN}/blobs/uploads/?mount={digest}&from={}",
                enc(OTHER)
            ),
            token,
        )
        .await;
        let forwarded = seen.lock().unwrap().last().unwrap().has_mount_params();
        assert_eq!(
            (mount.status, forwarded),
            if may_pull { (201, true) } else { (202, false) },
            "{token} (pull status {})",
            pull.status
        );
    }
}

/// A mount source must resolve to the pushed repository's registry route, whatever the caller may
/// pull, since the upstream reads `from` as one of its own repositories.
#[tokio::test]
async fn mount_sources_outside_the_pushed_route_are_refused() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let digest = digest();
    for token in ["pusher-for-prj_a", "deploy-for-prj_a"] {
        for from in ["other/default-prj_a", "my-gcp/alien-repo/prj_a"] {
            let response = send(
                manager.port,
                "POST",
                &format!("/v2/{OWN}/blobs/uploads/?mount={digest}&from={}", enc(from)),
                token,
            )
            .await;
            assert_eq!(response.status, 202, "{token} {from}: {}", response.body);
            let reached = seen.lock().unwrap().last().cloned().unwrap();
            assert!(!reached.has_mount_params(), "{token} {from}: {reached:?}");
        }
    }
}

/// Outside the upload-init POST the mount parameters carry no meaning and are dropped.
#[tokio::test]
async fn mount_parameters_on_a_manifest_put_are_dropped() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let response = send(
        manager.port,
        "PUT",
        &format!(
            "/v2/{OWN}/manifests/v1?mount={}&from={}&origin=registry.example",
            digest(),
            enc(OWN)
        ),
        "deploy-for-prj_a",
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.body);
    let reached = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(reached.path, format!("/v2/{OWN}/manifests/v1"));
    assert!(!reached.has_mount_params(), "{reached:?}");
}

/// Signed upload sessions, on `/v2/` and on `/artifacts-uploads/`, forward no mount parameters and
/// no signature parameters.
#[tokio::test]
async fn upload_sessions_forward_no_mount_parameters() {
    let (upstream, seen) = start_upstream().await;
    let manager = start_manager(upstream).await;
    let digest = digest();
    for init_query in ["", "gar=1"] {
        let init = send(
            manager.port,
            "POST",
            &format!("/v2/{OWN}/blobs/uploads/?{init_query}"),
            "deploy-for-prj_a",
        )
        .await;
        assert_eq!(init.status, 202, "{}", init.body);
        let signed = init.location(manager.port);
        assert!(signed.contains("_alien_sig="), "{signed}");
        for extra in [
            format!("mount={digest}&from={}", enc(OTHER)),
            format!("mount={digest}&from={}", enc(OWN)),
            format!("Mount={digest}&FROM={}&origin=registry.example", enc(OWN)),
            format!("from={}&digest={digest}", enc(OTHER)),
        ] {
            for method in ["POST", "PUT", "PATCH"] {
                for token in ["deploy-for-prj_a", "pusher-for-prj_a"] {
                    let before = seen.lock().unwrap().len();
                    let target = format!("{signed}&{extra}");
                    let response = send(manager.port, method, &target, token).await;
                    assert_eq!(response.status, 200, "{method} {target}: {}", response.body);
                    let reached = reached_since(&seen, before);
                    assert_eq!(reached.len(), 1, "{method} {target}");
                    assert!(!reached[0].has_mount_params(), "{reached:?}");
                    assert!(!reached[0].query.contains("_alien_"), "{reached:?}");
                    if extra.contains("digest=") {
                        assert!(
                            reached[0]
                                .pairs()
                                .contains(&("digest".to_string(), digest.clone())),
                            "{reached:?}"
                        );
                    }
                }
            }
        }
    }
}

/// Against a real distribution registry (`REGISTRY_UPSTREAM=localhost:<port>`, `registry:2`):
/// mounts from a source outside the pushed project become plain uploads, and mounts inside A land.
#[tokio::test]
#[ignore]
async fn distribution_registry_forwards_only_mounts_within_the_pushed_project() {
    let upstream = std::env::var("REGISTRY_UPSTREAM").expect("set REGISTRY_UPSTREAM");
    let direct_base = format!("http://{upstream}");
    let manager = start_manager(upstream).await;
    let port = manager.port;
    let direct = reqwest::Client::new();

    let upload = |repo: String, content: Vec<u8>| {
        let direct = direct.clone();
        let base = direct_base.clone();
        async move {
            let digest = format!("sha256:{}", hex::encode(sha2::Sha256::digest(&content)));
            let init = direct
                .post(format!("{base}/v2/{repo}/blobs/uploads/"))
                .send()
                .await
                .unwrap();
            assert_eq!(init.status(), 202);
            let loc = init.headers()["location"].to_str().unwrap().to_string();
            let loc = if loc.starts_with('/') {
                format!("{base}{loc}")
            } else {
                loc
            };
            let sep = if loc.contains('?') { '&' } else { '?' };
            let put = direct
                .put(format!("{loc}{sep}digest={digest}"))
                .body(content)
                .send()
                .await
                .unwrap();
            assert!(put.status().is_success(), "{}", put.status());
            digest
        }
    };
    let exists = |repo: String, digest: String| {
        let direct = direct.clone();
        let base = direct_base.clone();
        async move {
            direct
                .head(format!("{base}/v2/{repo}/blobs/{digest}"))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }
    };

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let held_by_b = upload(OTHER.to_string(), format!("b-only {nonce}").into_bytes()).await;
    assert_eq!(exists(OTHER.to_string(), held_by_b.clone()).await, 200);
    assert_eq!(exists(OWN.to_string(), held_by_b.clone()).await, 404);

    // Control: the registry completes a direct mount, so the plain uploads below are the proxy's.
    let control = direct
        .post(format!(
            "{direct_base}/v2/artifacts/control/blobs/uploads/?mount={held_by_b}&from={OTHER}"
        ))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(control, 201);

    for token in ["deploy-for-prj_a", "group-for-prj_a"] {
        for init in [
            format!("/v2/{OWN}/blobs/uploads/"),
            format!("/v2/{OWN}/blobs/uploads"),
        ] {
            for query in mount_queries_outside_the_project(&held_by_b, port) {
                let response = send(port, "POST", &format!("{init}?{query}"), token).await;
                assert_eq!(response.status, 202, "{token} {query}: {}", response.body);
            }
        }
        assert_eq!(
            exists(OWN.to_string(), held_by_b.clone()).await,
            404,
            "{token}"
        );
        for method in ["HEAD", "GET"] {
            let response = send(
                port,
                method,
                &format!("/v2/{OWN}/blobs/{held_by_b}"),
                "deploy-for-prj_a",
            )
            .await;
            assert_eq!(response.status, 404, "{method} after {token} mounts");
        }
    }

    // A mount inside A lands for a caller that may pull A: from A into A (a repeated push) and
    // from A into A/copy. A push-only caller's mount becomes a plain upload.
    let own = {
        let init = reqwest::Client::new()
            .post(format!("{}/v2/{OWN}/blobs/uploads/", manager.url))
            .bearer_auth("deploy-for-prj_a")
            .send()
            .await
            .unwrap();
        assert_eq!(init.status(), 202);
        let loc = init.headers()["location"].to_str().unwrap().to_string();
        let content = format!("a-own {nonce}").into_bytes();
        let digest = format!("sha256:{}", hex::encode(sha2::Sha256::digest(&content)));
        let sep = if loc.contains('?') { '&' } else { '?' };
        let put = reqwest::Client::new()
            .put(format!("{loc}{sep}digest={digest}"))
            .body(content)
            .send()
            .await
            .unwrap();
        assert!(put.status().is_success(), "{}", put.status());
        digest
    };
    for (target, token, landed) in [
        (OWN.to_string(), "deploy-for-prj_a", true),
        (format!("{OWN}/copy"), "deploy-for-prj_a", true),
        (format!("{OWN}/copy2"), "group-for-prj_a", false),
    ] {
        let response = send(
            port,
            "POST",
            &format!("/v2/{target}/blobs/uploads/?mount={own}&from={}", enc(OWN)),
            token,
        )
        .await;
        let (status, present) = if landed { (201, 200) } else { (202, 404) };
        assert_eq!(response.status, status, "{target}: {}", response.body);
        assert_eq!(
            exists(target.clone(), own.clone()).await,
            present,
            "{target}"
        );
    }

    // A mount follows the pull rule: it lands exactly when the caller may pull the source.
    let pull = send(
        port,
        "HEAD",
        &format!("/v2/{OTHER}/blobs/{held_by_b}"),
        "pusher-for-prj_a",
    )
    .await;
    let response = send(
        port,
        "POST",
        &format!(
            "/v2/{OWN}/scoped/blobs/uploads/?mount={held_by_b}&from={}",
            enc(OTHER)
        ),
        "pusher-for-prj_a",
    )
    .await;
    let landed = exists(format!("{OWN}/scoped"), held_by_b.clone()).await;
    if pull.status == 200 {
        assert_eq!((response.status, landed), (201, 200));
    } else {
        assert_eq!((response.status, landed), (202, 404));
    }
    assert_eq!(exists(OWN.to_string(), held_by_b.clone()).await, 404);
}

/// A real Docker client mounts a layer it already pushed to A when pushing to another repository
/// of A, and uploads instead of mounting when the only other holder of the layer is B.
/// Needs `REGISTRY_UPSTREAM` and a Docker daemon.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn docker_cli_mounts_within_the_project_and_uploads_across_projects() {
    let upstream = std::env::var("REGISTRY_UPSTREAM").expect("set REGISTRY_UPSTREAM");
    let manager = start_manager(upstream).await;
    let host = format!("localhost:{}", manager.port);
    let first = format!("{host}/{OWN}:v1");
    let second = format!("{host}/{OWN}/second:v1");
    let in_b = format!("{host}/{OTHER}:v1");

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let ctx = tempfile::tempdir().unwrap();
    std::fs::write(ctx.path().join("hello.txt"), format!("hello {nonce}\n")).unwrap();
    std::fs::write(
        ctx.path().join("Dockerfile"),
        b"FROM scratch\nCOPY hello.txt /hello.txt\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        let out =
            tokio::task::block_in_place(|| Command::new("docker").args(args).output().unwrap());
        let text = format!(
            "docker {}: {}\n{}{}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), text)
    };
    let must = |args: &[&str]| {
        let (ok, text) = run(args);
        assert!(ok, "{text}");
        text
    };
    must(&["build", "-q", "-t", &first, ctx.path().to_str().unwrap()]);
    must(&["tag", &first, &second]);
    must(&["tag", &first, &in_b]);

    // B's push token seeds the layer in B first, so Docker knows B as a mount source.
    must(&["login", &host, "-u", "x", "-p", "pusher-for-prj_b"]);
    must(&["push", &in_b]);
    // A deployment token of A pushes: Docker offers B as the source and must upload instead.
    must(&["login", &host, "-u", "x", "-p", "deploy-for-prj_a"]);
    let text = must(&["push", &first]);
    assert!(!text.contains("Mounted from"), "{text}");
    // Pushing A's second repository mounts from A.
    let text = must(&["push", &second]);
    assert!(text.contains(&format!("Mounted from {OWN}")), "{text}");
    for image in [&first, &second, &in_b] {
        let _ = run(&["rmi", image]);
    }
    must(&["pull", &second]);
    let _ = run(&["rmi", &second]);
    let _ = run(&["logout", &host]);
}
