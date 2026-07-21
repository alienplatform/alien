use super::*;
use alien_core::image_rewrite::strip_registry_host;
use alien_core::{Daemon, DaemonCode, ResourceLifecycle, Stack};
use std::sync::atomic::{AtomicUsize, Ordering};

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
fn test_strip_registry_host_localhost() {
    assert_eq!(
        strip_registry_host("localhost:5000/repo:tag"),
        Some("repo:tag".to_string())
    );
}

#[test]
fn test_extract_repo_name_flat() {
    assert_eq!(extract_repo_name("alien-e2e/manifests/v1"), "alien-e2e");
}

#[test]
fn test_extract_repo_name_gar_multi_segment() {
    assert_eq!(
        extract_repo_name("my-project/alien-repo/alien-prj-123/manifests/v1"),
        "my-project/alien-repo/alien-prj-123"
    );
}

#[test]
fn test_extract_repo_name_blobs() {
    assert_eq!(
        extract_repo_name("alien-e2e/blobs/sha256:abc123"),
        "alien-e2e"
    );
}

#[test]
fn test_extract_repo_name_uploads() {
    assert_eq!(
        extract_repo_name("alien-e2e/blobs/uploads/uuid-123"),
        "alien-e2e"
    );
}

#[test]
fn extract_repo_names_includes_daemon_image_resources() {
    let daemon = Daemon::new("host-loader".to_string())
        .code(DaemonCode::Image {
            image: "manager.example.com/artifacts/prj_test:host-loader-v1".to_string(),
        })
        .permissions("execution".to_string())
        .build();
    let stack = Stack::new("test-stack".to_string())
        .add(daemon, ResourceLifecycle::Live)
        .build();

    assert_eq!(
        extract_repo_names(&stack),
        vec!["artifacts/prj_test".to_string()]
    );
}

#[test]
fn proxy_base_url_prefers_forwarded_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "127.0.0.1:8080".parse().unwrap());
    headers.insert("x-forwarded-host", "manager.example.com".parse().unwrap());
    headers.insert("x-forwarded-proto", "https".parse().unwrap());

    assert_eq!(
        proxy_base_url(&headers, "http://localhost:8080"),
        "https://manager.example.com"
    );
}

#[test]
fn proxy_base_url_uses_request_host_for_public_requests() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "alien-manager.example.com".parse().unwrap());

    assert_eq!(
        proxy_base_url(&headers, "http://localhost:8080"),
        "https://alien-manager.example.com"
    );
}

#[test]
fn proxy_base_url_keeps_localhost_http() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "localhost:8080".parse().unwrap());

    assert_eq!(
        proxy_base_url(&headers, "http://localhost:8080"),
        "http://localhost:8080"
    );
}

#[tokio::test]
async fn credential_cache_serializes_generation_for_same_key() {
    let cache = Arc::new(CredentialCache::new());
    let generation_count = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();

    for _ in 0..16 {
        let cache = cache.clone();
        let generation_count = generation_count.clone();
        tasks.push(tokio::spawn(async move {
            let key = "https://ecr.example.test:alien-e2e:PushPull";
            if cache.get(key).is_none() {
                let generation_lock = cache.generation_lock(key);
                let _guard = generation_lock.lock().await;
                if cache.get(key).is_none() {
                    generation_count.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    cache.insert(
                        key.to_string(),
                        ArtifactRegistryCredentials {
                            auth_method: alien_bindings::traits::RegistryAuthMethod::Basic,
                            username: "AWS".to_string(),
                            password: "token".to_string(),
                            expires_at: None,
                        },
                        Duration::from_secs(300),
                    );
                }
            }
        }));
    }

    for task in tasks {
        task.await.expect("credential cache task should complete");
    }

    assert_eq!(generation_count.load(Ordering::SeqCst), 1);
    assert!(cache
        .get("https://ecr.example.test:alien-e2e:PushPull")
        .is_some());
}

// -----------------------------------------------------------------------
// RegistryRoutingTable
// -----------------------------------------------------------------------

#[derive(Debug)]
struct RegistryRouteTestProvider;

fn registry_route(prefix: &str, platform: Platform, binding_name: &str) -> RegistryRoute {
    RegistryRoute {
        prefix: prefix.to_string(),
        platform,
        provider: Arc::new(RegistryRouteTestProvider),
        binding_name: binding_name.to_string(),
    }
}

fn route_test_error(binding_name: &str) -> alien_bindings::error::Error {
    alien_error::AlienError::new(alien_bindings::error::ErrorData::config_invalid(
        binding_name,
        "test provider has no bindings",
    ))
}

#[async_trait::async_trait]
impl BindingsProviderApi for RegistryRouteTestProvider {
    async fn load_storage(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Storage>> {
        Err(route_test_error(binding_name))
    }

    async fn load_build(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Build>> {
        Err(route_test_error(binding_name))
    }

    async fn load_artifact_registry(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn ArtifactRegistry>> {
        Err(route_test_error(binding_name))
    }

    async fn load_vault(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Vault>> {
        Err(route_test_error(binding_name))
    }

    async fn load_kv(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Kv>> {
        Err(route_test_error(binding_name))
    }

    async fn load_postgres(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Postgres>> {
        Err(route_test_error(binding_name))
    }

    async fn load_queue(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Queue>> {
        Err(route_test_error(binding_name))
    }

    async fn load_worker(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Worker>> {
        Err(route_test_error(binding_name))
    }

    async fn load_container(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Container>> {
        Err(route_test_error(binding_name))
    }

    async fn load_service_account(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::ServiceAccount>> {
        Err(route_test_error(binding_name))
    }
}

#[test]
fn registry_routing_table_specific_prefix_beats_catch_all_regardless_registration_order() {
    for routes in [
        vec![
            registry_route("", Platform::Local, "local"),
            registry_route("artifacts", Platform::Aws, "aws"),
        ],
        vec![
            registry_route("artifacts", Platform::Aws, "aws"),
            registry_route("", Platform::Local, "local"),
        ],
    ] {
        let table = RegistryRoutingTable::new(routes).expect("routing table should be unambiguous");
        let route = table
            .resolve("artifacts/prj_test")
            .expect("specific route should match");

        assert_eq!(route.platform, Platform::Aws);
    }
}

#[test]
fn registry_routing_table_nested_prefixes_pick_longest_match() {
    let table = RegistryRoutingTable::new(vec![
        registry_route("artifacts", Platform::Aws, "aws"),
        registry_route("artifacts/team-a", Platform::Gcp, "gcp"),
        registry_route("", Platform::Local, "local"),
    ])
    .expect("nested prefixes should be valid");
    let route = table
        .resolve("artifacts/team-a/prj_test")
        .expect("nested route should match");

    assert_eq!(route.platform, Platform::Gcp);
}

#[test]
fn registry_routing_table_rejects_duplicate_prefixes_at_construction() {
    let result = RegistryRoutingTable::new(vec![
        registry_route("", Platform::Local, "local"),
        registry_route("", Platform::Azure, "azure"),
    ]);
    let Err(error) = result else {
        panic!("duplicate catch-all prefixes should fail");
    };

    assert!(error.contains("Duplicate artifact registry prefix '<empty>'"));
}

#[test]
fn registry_routing_table_rejects_duplicate_non_empty_prefixes_at_construction() {
    let result = RegistryRoutingTable::new(vec![
        registry_route("artifacts", Platform::Aws, "aws"),
        registry_route("artifacts", Platform::Gcp, "gcp"),
    ]);
    let Err(error) = result else {
        panic!("duplicate non-empty prefixes should fail");
    };

    assert!(error.contains("Duplicate artifact registry prefix 'artifacts'"));
}

// -----------------------------------------------------------------------
// project_id_after_prefix — the algorithm behind
// RegistryRoutingTable::project_id_for_repo. Tests target the free
// helper so separator handling is tested independently from routing.
// -----------------------------------------------------------------------

#[test]
fn project_id_aws_ecr_dash_separator() {
    assert_eq!(
        project_id_after_prefix("alien-artifacts-prj_xxx", "alien-artifacts"),
        Some("prj_xxx")
    );
    assert_eq!(
        project_id_after_prefix("alien-artifacts-prj_xxx/sub", "alien-artifacts"),
        Some("prj_xxx")
    );
}

#[test]
fn project_id_gar_slash_separator() {
    assert_eq!(
        project_id_after_prefix(
            "alien-dev-1/alien-artifacts/prj_xxx",
            "alien-dev-1/alien-artifacts",
        ),
        Some("prj_xxx")
    );
}

#[test]
fn project_id_local_slash_separator() {
    assert_eq!(
        project_id_after_prefix("artifacts/default/prj_xxx", "artifacts/default"),
        Some("prj_xxx")
    );
    assert_eq!(
        project_id_after_prefix("artifacts/default/prj_xxx/release-v1", "artifacts/default",),
        Some("prj_xxx")
    );
}

#[test]
fn project_id_acr_empty_prefix() {
    assert_eq!(project_id_after_prefix("prj_xxx", ""), Some("prj_xxx"));
    assert_eq!(project_id_after_prefix("prj_xxx/sub", ""), Some("prj_xxx"));
}

#[test]
fn project_id_rejects_malformed_separator() {
    // No `-` or `/` after the prefix — defense against repos that didn't
    // go through `make_full_repo_name`.
    assert_eq!(
        project_id_after_prefix("alien-artifactsXprj_xxx", "alien-artifacts"),
        None
    );
}

#[test]
fn project_id_rejects_empty_id() {
    assert_eq!(
        project_id_after_prefix("alien-artifacts-", "alien-artifacts"),
        None
    );
}

#[test]
fn project_id_rejects_bare_prefix() {
    // No suffix at all after the prefix.
    assert_eq!(
        project_id_after_prefix("alien-artifacts", "alien-artifacts"),
        None
    );
}

#[test]
fn project_id_rejects_unrelated_prefix() {
    // The prefix isn't actually a prefix of repo_name — `strip_prefix` returns None.
    assert_eq!(
        project_id_after_prefix("unknown/path/prj_xxx", "alien-artifacts"),
        None
    );
}

#[test]
fn gar_upload_session_location_gets_signed_repo_context() {
    let signing_key = b"test-registry-upload-session-key";
    let repo_name = "cloud-project/artifacts/prj_123";
    let location = "https://manager.example.com/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/session-1?digest=sha256:abc";

    let rewritten =
        rewrite_location_with_upload_session_auth(location, Some(repo_name), signing_key)
            .expect("GAR upload session location should be signed");
    let url = Url::parse(&rewritten).expect("rewritten location should be a URL");
    let query = url
        .query_pairs()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect::<HashMap<_, _>>();

    assert_eq!(query.get("digest").map(String::as_str), Some("sha256:abc"));
    assert_eq!(
        query.get(UPLOAD_SESSION_REPO_PARAM).map(String::as_str),
        Some(repo_name)
    );
    assert!(verify_upload_session_auth(signing_key, url.path(), &query).is_ok());

    let upstream_query = strip_upload_session_auth_params(&query);
    assert_eq!(upstream_query.len(), 1);
    assert_eq!(
        upstream_query.get("digest").map(String::as_str),
        Some("sha256:abc")
    );
}

#[test]
fn gar_upload_session_auth_rejects_tampering() {
    let signing_key = b"test-registry-upload-session-key";
    let path =
        "/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/session-1";
    let repo_name = "cloud-project/artifacts/prj_123";
    let expires_at = chrono::Utc::now().timestamp() + UPLOAD_SESSION_TTL_SECONDS;
    let signature = sign_upload_session(signing_key, path, repo_name, expires_at);

    let mut query = HashMap::from([
        (
            UPLOAD_SESSION_VERSION_PARAM.to_string(),
            UPLOAD_SESSION_VERSION.to_string(),
        ),
        (UPLOAD_SESSION_REPO_PARAM.to_string(), repo_name.to_string()),
        (
            UPLOAD_SESSION_EXPIRES_PARAM.to_string(),
            expires_at.to_string(),
        ),
        (UPLOAD_SESSION_SIGNATURE_PARAM.to_string(), signature),
    ]);

    assert!(verify_upload_session_auth(signing_key, path, &query).is_ok());

    query.insert(
        UPLOAD_SESSION_REPO_PARAM.to_string(),
        "cloud-project/artifacts/prj_other".to_string(),
    );
    assert!(verify_upload_session_auth(signing_key, path, &query).is_err());

    query.insert(UPLOAD_SESSION_REPO_PARAM.to_string(), repo_name.to_string());
    assert!(verify_upload_session_auth(
        signing_key,
        "/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/other-session",
        &query,
    )
    .is_err());
}

#[test]
fn gar_upload_session_auth_rejects_expired_token() {
    let signing_key = b"test-registry-upload-session-key";
    let path =
        "/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/session-1";
    let repo_name = "cloud-project/artifacts/prj_123";
    let expires_at = chrono::Utc::now().timestamp() - 1;
    let signature = sign_upload_session(signing_key, path, repo_name, expires_at);
    let query = HashMap::from([
        (
            UPLOAD_SESSION_VERSION_PARAM.to_string(),
            UPLOAD_SESSION_VERSION.to_string(),
        ),
        (UPLOAD_SESSION_REPO_PARAM.to_string(), repo_name.to_string()),
        (
            UPLOAD_SESSION_EXPIRES_PARAM.to_string(),
            expires_at.to_string(),
        ),
        (UPLOAD_SESSION_SIGNATURE_PARAM.to_string(), signature),
    ]);

    assert!(verify_upload_session_auth(signing_key, path, &query).is_err());
}

#[test]
fn oci_upload_session_location_gets_signed_for_dockdash_compat() {
    // Cloud OCI registries (ECR, GCR, …) return `/v2/{repo}/blobs/
    // uploads/{session-id}` Location URLs that push clients treat as
    // self-authenticating (no further Bearer token sent). The proxy
    // has to sign these URLs the same way it signs GAR's
    // `/artifacts-uploads/` URLs, otherwise the subsequent PUT/PATCH
    // arrives at the proxy with no Authorization and fails with 401.
    let location = "https://manager.example.com/v2/repo/blobs/uploads/session-1";

    let signed = rewrite_location_with_upload_session_auth(
        location,
        Some("cloud-project/artifacts/prj_123"),
        b"test-key",
    )
    .expect("OCI upload-session location should be signed");

    assert_ne!(signed, location, "URL should have been signed");
    assert!(signed.contains(UPLOAD_SESSION_VERSION_PARAM));
    assert!(signed.contains(UPLOAD_SESSION_SIGNATURE_PARAM));
    assert!(signed.contains(UPLOAD_SESSION_EXPIRES_PARAM));
}

#[test]
fn non_session_location_is_not_signed() {
    // Locations that aren't upload-session URLs (e.g. a manifest URL
    // returned on push, or any other generic OCI path) must NOT get
    // signed — they go through Bearer auth like the rest of the API.
    let location = "https://manager.example.com/v2/repo/manifests/latest";

    assert_eq!(
        rewrite_location_with_upload_session_auth(
            location,
            Some("cloud-project/artifacts/prj_123"),
            b"test-key",
        )
        .expect("non-session location should be unchanged"),
        location
    );
}

#[test]
fn is_oci_upload_session_path_matches_session_urls_only() {
    // Initial upload POST has no session-id suffix — must NOT be
    // recognized as a session URL, so Bearer auth still kicks in.
    assert!(!is_oci_upload_session_path("/v2/repo/blobs/uploads/"));
    assert!(!is_oci_upload_session_path("v2/repo/blobs/uploads/"));

    // Real session URLs.
    assert!(is_oci_upload_session_path(
        "/v2/repo/blobs/uploads/3403bc14-cbcd-3760-a4b1-c678a3c6ea61"
    ));
    assert!(is_oci_upload_session_path(
        "v2/alien-artifacts/host-loader/blobs/uploads/abc-123"
    ));

    // Other OCI paths.
    assert!(!is_oci_upload_session_path("/v2/repo/manifests/latest"));
    assert!(!is_oci_upload_session_path("/v2/repo/blobs/sha256:abc"));
    assert!(!is_oci_upload_session_path("/artifacts-uploads/something"));
}

#[test]
fn raw_gar_upload_session_path_does_not_identify_project_repo() {
    assert_eq!(
        project_id_after_prefix(
            "/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/session-1",
            "cloud-project/artifacts",
        ),
        None
    );
    assert_eq!(
        project_id_after_prefix("cloud-project/artifacts/prj_123", "cloud-project/artifacts",),
        Some("prj_123")
    );
}
