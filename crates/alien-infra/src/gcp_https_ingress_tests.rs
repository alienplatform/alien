use super::*;
use alien_gcp_clients::gcp::compute::{Backend, MockComputeApi, OperationStatus};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct Provider {
    pub(crate) objects: BTreeMap<String, Value>,
    pub(crate) mutations: usize,
    pub(crate) lose_backend_response: bool,
    pub(crate) deny_backend_recovery_reads: bool,
    fail_certificate_delete: bool,
    lose_tcp_proxy_response: bool,
    lose_health_update_response: bool,
}

pub(crate) type Store = Arc<Mutex<Provider>>;

fn missing(name: &str) -> alien_client_core::Error {
    AlienError::new(ClientErrorData::RemoteResourceNotFound {
        resource_type: "ingress child".to_string(),
        resource_name: name.to_string(),
    })
}

fn read<T: DeserializeOwned>(
    store: &Store,
    kind: &str,
    name: &str,
) -> alien_client_core::Result<T> {
    let store = store.lock().unwrap();
    if kind == "backend"
        && store.deny_backend_recovery_reads
        && !store.lose_backend_response
        && store.objects.contains_key(&format!("{kind}/{name}"))
    {
        return Err(AlienError::new(ClientErrorData::AuthenticationError {
            message: "backend recovery read denied until explicit retry".to_string(),
        }));
    }
    let value = store
        .objects
        .get(&format!("{kind}/{name}"))
        .ok_or_else(|| missing(name))?;
    Ok(serde_json::from_value(value.clone()).unwrap())
}

fn done() -> Operation {
    Operation {
        name: Some("mutation".to_string()),
        region: Some("regions/us-central1".to_string()),
        status: Some(OperationStatus::Done),
        ..Default::default()
    }
}

fn insert(
    store: &Store,
    kind: &str,
    value: &impl Serialize,
) -> alien_client_core::Result<Operation> {
    let mut value = serde_json::to_value(value).unwrap();
    let name = value["name"].as_str().unwrap().to_string();
    if kind == "address" {
        value["address"] = if name.starts_with("private-") {
            json!("10.20.0.10")
        } else {
            json!("203.0.113.10")
        };
    }
    if kind == "backend" {
        value["fingerprint"] = json!("fresh");
    }
    let mut store = store.lock().unwrap();
    let key = format!("{kind}/{name}");
    assert!(!store.objects.contains_key(&key), "duplicate create {key}");
    value["id"] = json!((store.mutations + 1).to_string());
    store.objects.insert(key, value);
    store.mutations += 1;
    if kind == "backend" && store.lose_backend_response {
        store.lose_backend_response = false;
        return Err(AlienError::new(ClientErrorData::AuthenticationError {
            message: "lost create response".to_string(),
        }));
    }
    if kind == "tcpProxy" && store.lose_tcp_proxy_response {
        store.lose_tcp_proxy_response = false;
        return Err(AlienError::new(ClientErrorData::Timeout {
            message: "lost TCP proxy create response".to_string(),
        }));
    }
    Ok(done())
}

fn update_health(
    store: &Store,
    name: &str,
    desired: HealthCheck,
) -> alien_client_core::Result<Operation> {
    let mut state = store.lock().unwrap();
    let key = format!("health/{name}");
    let old = state
        .objects
        .get(&key)
        .expect("update requires existing health check");
    let mut replacement = serde_json::to_value(desired).unwrap();
    replacement["id"] = old["id"].clone();
    state.objects.insert(key, replacement);
    state.mutations += 1;
    if state.lose_health_update_response {
        state.lose_health_update_response = false;
        return Err(AlienError::new(ClientErrorData::Timeout {
            message: "lost health update reply".to_string(),
        }));
    }
    Ok(done())
}

fn remove(store: &Store, kind: &str, name: &str) -> alien_client_core::Result<Operation> {
    let mut store = store.lock().unwrap();
    if kind == "certificate" && store.fail_certificate_delete {
        store.fail_certificate_delete = false;
        return Err(AlienError::new(ClientErrorData::AuthenticationError {
            message: "temporary delete failure".to_string(),
        }));
    }
    assert!(store.objects.remove(&format!("{kind}/{name}")).is_some());
    store.mutations += 1;
    Ok(done())
}

pub(crate) fn provider(store: &Store) -> MockComputeApi {
    let mut client = MockComputeApi::new();
    macro_rules! regional {
        ($kind:literal, $get:ident, $create:ident, $delete:ident) => {
            let state = store.clone();
            client
                .$get()
                .returning(move |_, name| read(&state, $kind, &name));
            let state = store.clone();
            client
                .$create()
                .returning(move |_, object| insert(&state, $kind, &object));
            let state = store.clone();
            client
                .$delete()
                .returning(move |_, name| remove(&state, $kind, &name));
        };
    }
    regional!(
        "health",
        expect_get_region_health_check,
        expect_insert_region_health_check,
        expect_delete_region_health_check
    );
    regional!(
        "backend",
        expect_get_region_backend_service,
        expect_insert_region_backend_service,
        expect_delete_region_backend_service
    );
    regional!(
        "url",
        expect_get_region_url_map,
        expect_insert_region_url_map,
        expect_delete_region_url_map
    );
    regional!(
        "certificate",
        expect_get_region_ssl_certificate,
        expect_insert_region_ssl_certificate,
        expect_delete_region_ssl_certificate
    );
    regional!(
        "proxy",
        expect_get_region_target_https_proxy,
        expect_insert_region_target_https_proxy,
        expect_delete_region_target_https_proxy
    );
    regional!(
        "tcpProxy",
        expect_get_region_target_tcp_proxy,
        expect_insert_region_target_tcp_proxy,
        expect_delete_region_target_tcp_proxy
    );
    regional!(
        "address",
        expect_get_address,
        expect_insert_address,
        expect_delete_address
    );
    regional!(
        "frontend",
        expect_get_forwarding_rule,
        expect_insert_forwarding_rule,
        expect_delete_forwarding_rule
    );
    macro_rules! global {
        ($kind:literal, $get:ident, $create:ident, $delete:ident) => {
            let state = store.clone();
            client
                .$get()
                .returning(move |name| read(&state, $kind, &name));
            let state = store.clone();
            client
                .$create()
                .returning(move |object| insert(&state, $kind, &object));
            let state = store.clone();
            client
                .$delete()
                .returning(move |name| remove(&state, $kind, &name));
        };
    }
    global!(
        "health",
        expect_get_health_check,
        expect_insert_health_check,
        expect_delete_health_check
    );
    global!(
        "backend",
        expect_get_backend_service,
        expect_insert_backend_service,
        expect_delete_backend_service
    );
    global!(
        "url",
        expect_get_url_map,
        expect_insert_url_map,
        expect_delete_url_map
    );
    global!(
        "certificate",
        expect_get_ssl_certificate,
        expect_insert_ssl_certificate,
        expect_delete_ssl_certificate
    );
    global!(
        "proxy",
        expect_get_target_https_proxy,
        expect_insert_target_https_proxy,
        expect_delete_target_https_proxy
    );
    global!(
        "tcpProxy",
        expect_get_target_tcp_proxy,
        expect_insert_target_tcp_proxy,
        expect_delete_target_tcp_proxy
    );
    global!(
        "address",
        expect_get_global_address,
        expect_insert_global_address,
        expect_delete_global_address
    );
    global!(
        "frontend",
        expect_get_global_forwarding_rule,
        expect_insert_global_forwarding_rule,
        expect_delete_global_forwarding_rule
    );
    let state = store.clone();
    client
        .expect_update_health_check()
        .returning(move |name, desired| update_health(&state, &name, desired));
    let state = store.clone();
    client
        .expect_update_region_health_check()
        .returning(move |_, name, desired| update_health(&state, &name, desired));
    client
        .expect_get_global_operation()
        .returning(|_| Ok(done()));
    let state = store.clone();
    client
        .expect_set_target_https_proxy_ssl_certificates()
        .returning(move |name, certificates| {
            let mut state = state.lock().unwrap();
            state.objects.get_mut(&format!("proxy/{name}")).unwrap()["sslCertificates"] =
                json!(certificates);
            state.mutations += 1;
            Ok(done())
        });
    client
        .expect_get_region_operation()
        .returning(|_, _| Ok(done()));
    let state = store.clone();
    client
        .expect_set_region_target_https_proxy_ssl_certificates()
        .returning(move |_, name, certificates| {
            let mut state = state.lock().unwrap();
            state.objects.get_mut(&format!("proxy/{name}")).unwrap()["sslCertificates"] =
                json!(certificates);
            state.mutations += 1;
            Ok(done())
        });
    let state = store.clone();
    client
        .expect_patch_region_backend_service()
        .returning(move |_, name, desired| {
            assert_eq!(desired.fingerprint.as_deref(), Some("fresh"));
            let mut state = state.lock().unwrap();
            let live = state
                .objects
                .get_mut(&format!("backend/{name}"))
                .unwrap()
                .as_object_mut()
                .unwrap();
            live.extend(
                serde_json::to_value(desired)
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            state.mutations += 1;
            Ok(done())
        });
    client
}

fn plan(certificate: &str) -> GcpHttpsIngressPlan<'_> {
    GcpHttpsIngressPlan {
        scope: GcpHttpsIngressScope::RegionalPrivate,
        resource_id: "api",
        project: "example",
        region: "us-central1",
        network: "projects/example/global/networks/test",
        subnetwork: "projects/example/regions/us-central1/subnetworks/test",
        firewall: None,
        health_check: None,
        backend: BackendService {
            backends: vec![Backend {
                group: Some(
                    "projects/example/regions/us-central1/networkEndpointGroups/test".to_string(),
                ),
                ..Default::default()
            }],
            ..Default::default()
        },
        frontend: crate::GcpIngressFrontend::Https(GcpIngressCertificate::Managed {
            certificate_pem: certificate,
            private_key_pem: "runtime-only-private-key",
        }),
    }
}

fn checkpoint(state: &GcpHttpsIngressState) -> GcpHttpsIngressState {
    let json = serde_json::to_string(state).unwrap();
    assert!(!json.contains("runtime-only-private-key"));
    serde_json::from_str(&json).unwrap()
}

async fn ready(
    state: &mut GcpHttpsIngressState,
    client: &MockComputeApi,
    store: &Store,
    certificate: &str,
) {
    for _ in 0..60 {
        *state = checkpoint(state);
        let before = store.lock().unwrap().mutations;
        let mut desired = plan(certificate);
        desired.scope = state.scope;
        let complete = state.reconcile(client, desired).await.unwrap();
        assert!(store.lock().unwrap().mutations - before <= 1);
        if complete {
            return;
        }
    }
    panic!("private ingress did not become ready");
}

#[tokio::test]
async fn lost_response_checkpoint_rotation_and_failed_cleanup_preserve_frontend() {
    let store = Arc::new(Mutex::new(Provider {
        lose_backend_response: true,
        ..Default::default()
    }));
    let client = provider(&store);
    let mut state = GcpHttpsIngressState::new(&plan("certificate-one"));
    assert!(state
        .reconcile(&client, plan("certificate-one"))
        .await
        .is_err());
    assert!(state.children.contains(&Child::Backend(state.name())));
    ready(&mut state, &client, &store, "certificate-one").await;
    let frontend = store.lock().unwrap().objects[&format!("frontend/{}", state.name())].clone();
    let mutations = store.lock().unwrap().mutations;
    ready(&mut state, &client, &store, "certificate-one").await;
    assert_eq!(
        store.lock().unwrap().mutations,
        mutations,
        "no-op update mutated provider"
    );
    ready(&mut state, &client, &store, "certificate-two").await;
    assert_eq!(
        store.lock().unwrap().objects[&format!("frontend/{}", state.name())],
        frontend
    );
    assert_eq!(
        store
            .lock()
            .unwrap()
            .objects
            .keys()
            .filter(|key| key.starts_with("certificate/"))
            .count(),
        1
    );

    store.lock().unwrap().fail_certificate_delete = true;
    let mut saw_failure = false;
    for _ in 0..60 {
        state = checkpoint(&state);
        match state.delete(&client, "api").await {
            Ok(true) => break,
            Ok(false) => {}
            Err(_) => {
                assert!(!saw_failure);
                saw_failure = true;
                assert!(state
                    .children
                    .iter()
                    .any(|child| matches!(child, Child::Certificate(_))));
            }
        }
    }
    assert!(saw_failure);
    assert!(state.children.is_empty());
    assert!(store.lock().unwrap().objects.is_empty());
    assert_eq!(state.address, None);
}

#[tokio::test]
async fn replaced_or_missing_confirmed_frontend_is_not_recreated_or_deleted() {
    for scope in [
        GcpHttpsIngressScope::RegionalPrivate,
        GcpHttpsIngressScope::GlobalPublic,
    ] {
        let store = Arc::new(Mutex::new(Provider::default()));
        let client = provider(&store);
        let mut desired = plan("certificate");
        desired.scope = scope;
        let mut state = GcpHttpsIngressState::new(&desired);
        ready(&mut state, &client, &store, "certificate").await;
        state = checkpoint(&state);
        let frontend = format!("frontend/{}", state.name());
        let before = store.lock().unwrap().mutations;
        // Even a copied ownership description cannot authorize a replacement
        // with a different immutable provider ID.
        store.lock().unwrap().objects.get_mut(&frontend).unwrap()["id"] = json!("replacement");
        let mut desired = plan("certificate");
        desired.scope = scope;
        assert!(state
            .reconcile(&client, desired)
            .await
            .unwrap_err()
            .message
            .contains("replaced"));
        assert!(state
            .delete(&client, "api")
            .await
            .unwrap_err()
            .message
            .contains("replaced"));
        assert_eq!(store.lock().unwrap().mutations, before);
        store.lock().unwrap().objects.remove(&frontend).unwrap();
        let mut desired = plan("certificate");
        desired.scope = scope;
        assert!(state
            .reconcile(&client, desired)
            .await
            .unwrap_err()
            .message
            .contains("missing"));
        assert_eq!(store.lock().unwrap().mutations, before);
        // A confirmed 404 is still successful best-effort teardown, including
        // after restoring the checkpoint which refused the replacement.
        for _ in 0..60 {
            state = checkpoint(&state);
            if state.delete(&client, "api").await.unwrap() {
                break;
            }
        }
        assert!(state.children.is_empty());
        assert!(state.provider_ids.is_empty());
        assert!(store.lock().unwrap().objects.is_empty());
    }
}

#[tokio::test]
async fn foreign_child_is_refused_on_retry_and_delete() {
    let store = Arc::new(Mutex::new(Provider::default()));
    let client = provider(&store);
    let mut state = GcpHttpsIngressState::new(&plan("certificate"));
    store.lock().unwrap().objects.insert(
        format!("backend/{}", state.name()),
        json!({"name": state.name(), "description": "someone else"}),
    );
    assert!(state.reconcile(&client, plan("certificate")).await.is_err());
    state = checkpoint(&state);
    assert!(state.delete(&client, "api").await.is_err());
    assert_eq!(store.lock().unwrap().mutations, 0);
    assert_eq!(state.children.len(), 1);
}

#[tokio::test]
async fn backend_patch_preserves_unmanaged_fields_and_does_not_replace_frontend() {
    let store = Arc::new(Mutex::new(Provider::default()));
    let client = provider(&store);
    let mut state = GcpHttpsIngressState::new(&plan("certificate"));
    ready(&mut state, &client, &store, "certificate").await;
    let key = format!("backend/{}", state.name());
    store.lock().unwrap().objects.get_mut(&key).unwrap()["timeoutSec"] = json!(20);
    store.lock().unwrap().objects.get_mut(&key).unwrap()["unknownProviderField"] =
        json!("preserve");
    let mut desired = plan("certificate");
    desired.backend.timeout_sec = Some(60);
    assert!(!state.reconcile(&client, desired).await.unwrap());
    let provider = store.lock().unwrap();
    assert_eq!(provider.objects[&key]["timeoutSec"], json!(60));
    assert_eq!(
        provider.objects[&key]["unknownProviderField"],
        json!("preserve")
    );
    assert_eq!(
        provider
            .objects
            .keys()
            .filter(|key| key.starts_with("frontend/"))
            .count(),
        1
    );
}

#[tokio::test]
async fn denied_delete_retains_children_without_signaling_deleted_to_executor() {
    let mut state = GcpHttpsIngressState::new(&plan("certificate"));
    state.children.insert(Child::Backend(state.name()));
    let mut client = MockComputeApi::new();
    client
        .expect_get_region_backend_service()
        .times(1)
        .returning(|_, name| {
            Err(AlienError::new(ClientErrorData::RemoteAccessDenied {
                resource_type: "backendService".to_string(),
                resource_name: name,
            }))
        });
    let error = state.delete(&client, "api").await.unwrap_err();
    assert!(!matches!(error.http_status_code, Some(401 | 403 | 404)));
    assert!(error.source.is_none());
    assert_eq!(state.children.len(), 1);
}

#[tokio::test]
async fn switching_to_custom_certificate_retires_only_owned_imports() {
    let store = Arc::new(Mutex::new(Provider::default()));
    let client = provider(&store);
    let mut state = GcpHttpsIngressState::new(&plan("certificate"));
    ready(&mut state, &client, &store, "certificate").await;
    let references =
        vec!["projects/example/regions/us-central1/sslCertificates/customer".to_string()];
    let mut complete = false;
    for _ in 0..30 {
        state = checkpoint(&state);
        let mut desired = plan("unused");
        desired.frontend = crate::GcpIngressFrontend::Https(GcpIngressCertificate::Custom {
            references: &references,
        });
        if state.reconcile(&client, desired).await.unwrap() {
            complete = true;
            break;
        }
    }
    assert!(complete);
    assert!(!state
        .children
        .iter()
        .any(|child| matches!(child, Child::Certificate(_))));
    assert!(store
        .lock()
        .unwrap()
        .objects
        .keys()
        .all(|key| !key.starts_with("certificate/")));
    let proxy = store.lock().unwrap().objects[&format!("proxy/{}", state.name())].clone();
    assert_eq!(proxy["sslCertificates"], json!(references));
    for _ in 0..60 {
        if state.delete(&client, "api").await.unwrap() {
            break;
        }
    }
    assert!(state.children.is_empty());
    assert!(store.lock().unwrap().objects.is_empty());
}

#[tokio::test]
async fn adding_public_frontend_and_removing_private_preserves_shared_backend_registration() {
    let store = Arc::new(Mutex::new(Provider::default()));
    let client = provider(&store);
    let mut private = GcpHttpsIngressState::new(&plan("certificate"));
    ready(&mut private, &client, &store, "certificate").await;
    let private_frontend =
        store.lock().unwrap().objects[&format!("frontend/{}", private.name())].clone();
    let mut public_plan = plan("certificate");
    public_plan.scope = GcpHttpsIngressScope::GlobalPublic;
    let mut public = GcpHttpsIngressState::new(&public_plan);
    let mut complete = false;
    for _ in 0..40 {
        public = checkpoint(&public);
        let mut desired = plan("certificate");
        desired.scope = GcpHttpsIngressScope::GlobalPublic;
        if public.reconcile(&client, desired).await.unwrap() {
            complete = true;
            break;
        }
    }
    assert!(complete);
    {
        let provider = store.lock().unwrap();
        assert_eq!(
            provider.objects[&format!("frontend/{}", private.name())],
            private_frontend
        );
        let frontend = &provider.objects[&format!("frontend/{}", public.name())];
        assert_eq!(frontend["loadBalancingScheme"], json!("EXTERNAL"));
        assert_eq!(frontend["IPAddress"], json!("203.0.113.10"));
        assert!(frontend.get("network").is_none());
        assert!(frontend.get("subnetwork").is_none());
        assert!(frontend.get("allowGlobalAccess").is_none());
        assert_eq!(
            provider.objects[&format!("backend/{}", private.name())]["backends"],
            provider.objects[&format!("backend/{}", public.name())]["backends"]
        );
    }
    for _ in 0..40 {
        if private.delete(&client, "api").await.unwrap() {
            break;
        }
    }
    assert!(private.children.is_empty());
    assert!(store
        .lock()
        .unwrap()
        .objects
        .contains_key(&format!("frontend/{}", public.name())));
    for _ in 0..40 {
        if public.delete(&client, "api").await.unwrap() {
            break;
        }
    }
    assert!(public.children.is_empty());
    assert!(store.lock().unwrap().objects.is_empty());
}

#[tokio::test]
async fn checkpoint_without_scope_resumes_as_regional_private() {
    let store = Arc::new(Mutex::new(Provider::default()));
    let client = provider(&store);
    let mut private = GcpHttpsIngressState::new(&plan("certificate"));
    ready(&mut private, &client, &store, "certificate").await;
    let mut checkpoint = serde_json::to_value(&private).unwrap();
    checkpoint.as_object_mut().unwrap().remove("scope");
    checkpoint.as_object_mut().unwrap().remove("providerIds");
    checkpoint.as_object_mut().unwrap().remove("legacyNames");
    checkpoint
        .as_object_mut()
        .unwrap()
        .remove("legacyDescriptions");
    let mut restored: GcpHttpsIngressState = serde_json::from_value(checkpoint).unwrap();
    let before = store.lock().unwrap().mutations;
    ready(&mut restored, &client, &store, "certificate").await;
    assert_eq!(before, store.lock().unwrap().mutations);
}

#[tokio::test]
async fn recorded_global_graph_handoff_preserves_frontend_rotates_and_cleans_up() {
    let store = Arc::new(Mutex::new(Provider::default()));
    let client = provider(&store);
    let mut desired = plan("old-certificate");
    desired.scope = GcpHttpsIngressScope::GlobalPublic;
    let mut original = GcpHttpsIngressState::new(&desired);
    ready(&mut original, &client, &store, "old-certificate").await;
    // Model an older controller's independently named children, descriptions
    // and saved IDs, rather than reusing the new controller's UUID ownership.
    let collections = [
        ("backend", "backendServices"),
        ("url", "urlMaps"),
        ("certificate", "sslCertificates"),
        ("proxy", "targetHttpsProxies"),
        ("address", "addresses"),
        ("frontend", "forwardingRules"),
    ];
    let replacements: Vec<_> = {
        let provider = store.lock().unwrap();
        collections
            .iter()
            .map(|(kind, collection)| {
                let key = provider
                    .objects
                    .keys()
                    .find(|key| key.starts_with(&format!("{kind}/")))
                    .unwrap();
                (
                    original.url(collection, key.split_once('/').unwrap().1),
                    original.url(collection, &format!("legacy-{kind}")),
                )
            })
            .collect()
    };
    {
        let mut provider = store.lock().unwrap();
        for (key, object) in std::mem::take(&mut provider.objects) {
            let kind = key.split_once('/').unwrap().0;
            let mut text = object.to_string();
            for (old, new) in &replacements {
                text = text.replace(old, new);
            }
            let mut object: Value = serde_json::from_str(&text).unwrap();
            object["name"] = json!(format!("legacy-{kind}"));
            object["description"] = json!(format!("recorded {kind}"));
            provider
                .objects
                .insert(format!("{kind}/legacy-{kind}"), object);
        }
    }
    let records = store.lock().unwrap().objects.clone();
    let record = |kind: &str| {
        let object = &records[&format!("{kind}/legacy-{kind}")];
        GcpRecordedIngressChild {
            name: object["name"].as_str().unwrap(),
            description: object["description"].as_str().unwrap(),
            provider_id: object["id"].as_str(),
        }
    };
    let references = vec![original.url("sslCertificates", "legacy-certificate")];
    let recorded = || GcpRecordedHttpsIngress {
        health_check: None,
        backend: record("backend"),
        url_map: Some(record("url")),
        proxy: record("proxy"),
        address: record("address"),
        frontend: record("frontend"),
        managed_certificate: Some(record("certificate")),
        certificate_references: &references,
    };
    let before = store.lock().unwrap().mutations;
    let frontend = records["frontend/legacy-frontend"].clone();
    let mut desired = plan("renewed-certificate");
    desired.scope = GcpHttpsIngressScope::GlobalPublic;
    // A name, description and matching ID cannot authorize a graph wired to
    // an unrelated backend. Failed migration must have no external effects.
    store
        .lock()
        .unwrap()
        .objects
        .get_mut("url/legacy-url")
        .unwrap()["defaultService"] = json!("projects/example/global/backendServices/foreign");
    let error = GcpHttpsIngressState::from_recorded_global(&client, &desired, recorded())
        .await
        .unwrap_err();
    assert!(error.message.contains("inconsistent references"));
    assert_eq!(store.lock().unwrap().mutations, before);
    store.lock().unwrap().objects.insert(
        "url/legacy-url".to_string(),
        records["url/legacy-url"].clone(),
    );
    let mut state = GcpHttpsIngressState::from_recorded_global(&client, &desired, recorded())
        .await
        .unwrap();
    assert_eq!(state.backend_name(), "legacy-backend");
    assert_eq!(store.lock().unwrap().mutations, before);
    state = checkpoint(&state);
    ready(&mut state, &client, &store, "renewed-certificate").await;
    {
        let provider = store.lock().unwrap();
        assert_eq!(provider.objects["frontend/legacy-frontend"], frontend);
        assert!(!provider
            .objects
            .contains_key("certificate/legacy-certificate"));
        assert_eq!(
            provider
                .objects
                .keys()
                .filter(|key| key.starts_with("certificate/"))
                .count(),
            1
        );
        assert_eq!(
            provider.objects["proxy/legacy-proxy"]["sslCertificates"],
            json!([state.url(
                "sslCertificates",
                state
                    .children
                    .iter()
                    .find_map(|child| match child {
                        Child::Certificate(name) => Some(name.as_str()),
                        _ => None,
                    })
                    .unwrap()
            )])
        );
    }
    for _ in 0..60 {
        state = checkpoint(&state);
        if state.delete(&client, "api").await.unwrap() {
            break;
        }
    }
    assert!(state.children.is_empty());
    assert!(state.provider_ids.is_empty());
    assert!(state.legacy_descriptions.is_empty());
    assert!(store.lock().unwrap().objects.is_empty());
}

fn tcp_plan(scope: GcpHttpsIngressScope) -> GcpProxyIngressPlan<'static> {
    let mut desired = plan("unused");
    desired.scope = scope;
    desired.frontend = GcpIngressFrontend::Tcp { port: 5432 };
    desired.backend.protocol = Some(BackendServiceProtocol::Tcp);
    desired.health_check = Some(HealthCheck {
        r#type: Some(alien_gcp_clients::gcp::compute::HealthCheckType::Tcp),
        tcp_health_check: Some(alien_gcp_clients::gcp::compute::TcpHealthCheck {
            port_specification: Some(
                alien_gcp_clients::gcp::compute::PortSpecification::UseServingPort,
            ),
            ..Default::default()
        }),
        ..Default::default()
    });
    desired
}

#[tokio::test]
async fn tcp_proxy_lost_response_reload_and_delete_preserve_exact_graph() {
    for scope in [
        GcpHttpsIngressScope::GlobalPublic,
        GcpHttpsIngressScope::RegionalPrivate,
    ] {
        let store = Arc::new(Mutex::new(Provider {
            lose_tcp_proxy_response: true,
            ..Default::default()
        }));
        let client = provider(&store);
        let mut state = GcpProxyIngressState::new(&tcp_plan(scope));
        let mut ready = false;
        let mut lost = 0;
        for _ in 0..40 {
            state = checkpoint(&state);
            let before = store.lock().unwrap().mutations;
            match state.reconcile(&client, tcp_plan(scope)).await {
                Ok(done) => ready = done,
                Err(error) => {
                    assert!(error.retryable);
                    lost += 1;
                }
            }
            assert!(store.lock().unwrap().mutations - before <= 1);
            if ready {
                break;
            }
        }
        assert!(ready);
        assert_eq!(lost, 1);
        let objects = store.lock().unwrap().objects.clone();
        assert_eq!(objects.len(), 5);
        assert!(objects.keys().all(|key| !key.starts_with("certificate/")
            && !key.starts_with("url/")
            && !key.starts_with("proxy/")));
        let proxy = &objects[&format!("tcpProxy/{}", state.name())];
        assert_eq!(
            proxy["service"],
            state.url("backendServices", &state.name())
        );
        let frontend = &objects[&format!("frontend/{}", state.name())];
        assert_eq!(
            frontend["target"],
            state.url("targetTcpProxies", &state.name())
        );
        assert_eq!(frontend["portRange"], "5432");
        let before = store.lock().unwrap().mutations;
        state = checkpoint(&state);
        assert!(state.reconcile(&client, tcp_plan(scope)).await.unwrap());
        assert_eq!(store.lock().unwrap().mutations, before);
        assert_eq!(store.lock().unwrap().objects, objects);
        let mut deleted = false;
        for _ in 0..40 {
            state = checkpoint(&state);
            let before = store.lock().unwrap().mutations;
            deleted = state.delete(&client, "api").await.unwrap();
            assert!(store.lock().unwrap().mutations - before <= 1);
            if deleted {
                break;
            }
        }
        assert!(deleted);
        assert!(store.lock().unwrap().objects.is_empty());
    }
}

#[tokio::test]
async fn tcp_protocol_change_is_rejected_before_provider_mutation() {
    let desired = tcp_plan(GcpHttpsIngressScope::RegionalPrivate);
    let mut state = GcpProxyIngressState::new(&desired);
    let client = MockComputeApi::new();
    assert!(state.reconcile(&client, plan("cert")).await.is_err());
    let mut changed = tcp_plan(GcpHttpsIngressScope::RegionalPrivate);
    changed.frontend = GcpIngressFrontend::Tcp { port: 5433 };
    assert!(state.reconcile(&client, changed).await.is_err());
    assert!(state.children.is_empty());
}

#[tokio::test]
async fn recorded_tcp_graph_handoff_keeps_ids_and_rejects_proxy_replacement() {
    let scope = GcpHttpsIngressScope::GlobalPublic;
    let store = Arc::new(Mutex::new(Provider::default()));
    let client = provider(&store);
    let mut original = GcpProxyIngressState::new(&tcp_plan(scope));
    let mut complete = false;
    for _ in 0..40 {
        complete = original.reconcile(&client, tcp_plan(scope)).await.unwrap();
        if complete {
            break;
        }
    }
    assert!(complete);
    let objects = store.lock().unwrap().objects.clone();
    let name = original.name();
    let owner = original.owner();
    let record = |kind: &str| GcpRecordedIngressChild {
        name: &name,
        description: &owner,
        provider_id: objects[&format!("{kind}/{name}")]["id"].as_str(),
    };
    let mut adopted = GcpProxyIngressState::from_recorded_global(
        &client,
        &tcp_plan(scope),
        GcpRecordedProxyIngress {
            health_check: Some(record("health")),
            backend: record("backend"),
            url_map: None,
            proxy: record("tcpProxy"),
            address: record("address"),
            frontend: record("frontend"),
            managed_certificate: None,
            certificate_references: &[],
        },
    )
    .await
    .unwrap();
    adopted = checkpoint(&adopted);
    let before = store.lock().unwrap().mutations;
    assert!(adopted.reconcile(&client, tcp_plan(scope)).await.unwrap());
    assert_eq!(store.lock().unwrap().mutations, before);
    assert_eq!(store.lock().unwrap().objects, objects);
    store
        .lock()
        .unwrap()
        .objects
        .get_mut(&format!("tcpProxy/{name}"))
        .unwrap()["id"] = json!("replacement");
    let error = adopted
        .reconcile(&client, tcp_plan(scope))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("replaced"));
    assert_eq!(store.lock().unwrap().mutations, before);
    let mut refused = false;
    for _ in 0..20 {
        match adopted.delete(&client, "api").await {
            Ok(done) => assert!(!done, "must refuse the replaced proxy"),
            Err(error) => {
                assert!(error.to_string().contains("replaced"));
                refused = true;
                break;
            }
        }
        adopted = checkpoint(&adopted);
    }
    assert!(refused);
    assert_eq!(
        store.lock().unwrap().objects[&format!("tcpProxy/{name}")]["id"],
        "replacement"
    );
}

#[tokio::test]
async fn tcp_health_restore_clears_stale_probe_after_lost_reply_and_preserves_settings() {
    for scope in [
        GcpHttpsIngressScope::GlobalPublic,
        GcpHttpsIngressScope::RegionalPrivate,
    ] {
        let store = Arc::new(Mutex::new(Provider::default()));
        let client = provider(&store);
        let mut state = GcpProxyIngressState::new(&tcp_plan(scope));
        let mut ready = false;
        for _ in 0..40 {
            state = checkpoint(&state);
            ready = state.reconcile(&client, tcp_plan(scope)).await.unwrap();
            if ready {
                break;
            }
        }
        assert!(ready);
        let key = format!("health/{}", state.child_name("healthCheck"));
        let ids = state.provider_ids.clone();
        let name = state.name();
        for stale in [
            json!({"portSpecification":"USE_FIXED_PORT", "port":17080, "request":"GET /health HTTP/1.0\r\n\r\n", "response":"withdrawn"}),
            json!({"portSpecification":"USE_SERVING_PORT", "response":"withdrawn"}),
        ] {
            let before = {
                let mut provider = store.lock().unwrap();
                let health = provider.objects.get_mut(&key).unwrap();
                health["tcpHealthCheck"] = stale;
                health["checkIntervalSec"] = json!(30);
                health["timeoutSec"] = json!(5);
                health["healthyThreshold"] = json!(3);
                health["unhealthyThreshold"] = json!(4);
                health["logConfig"] = json!({"enable":true});
                provider.lose_health_update_response = true;
                provider.mutations
            };
            state = checkpoint(&state);
            let error = state.reconcile(&client, tcp_plan(scope)).await.unwrap_err();
            assert!(error.retryable);
            state = checkpoint(&state);
            assert!(state.reconcile(&client, tcp_plan(scope)).await.unwrap());
            assert_eq!(state.provider_ids, ids);
            assert_eq!(state.name(), name);
            let provider = store.lock().unwrap();
            assert_eq!(
                provider.mutations,
                before + 1,
                "lost reply must not repeat replacement"
            );
            let health = &provider.objects[&key];
            assert_eq!(
                health["tcpHealthCheck"],
                json!({"portSpecification":"USE_SERVING_PORT"})
            );
            assert_eq!(health["checkIntervalSec"], 30);
            assert_eq!(health["timeoutSec"], 5);
            assert_eq!(health["healthyThreshold"], 3);
            assert_eq!(health["unhealthyThreshold"], 4);
            assert_eq!(health["logConfig"], json!({"enable":true}));
        }
        let before = store.lock().unwrap().mutations;
        {
            let mut provider = store.lock().unwrap();
            let tcp = &mut provider.objects.get_mut(&key).unwrap()["tcpHealthCheck"];
            tcp["request"] = json!("");
            tcp["response"] = json!("");
            tcp["proxyHeader"] = json!("NONE");
        }
        assert!(state.reconcile(&client, tcp_plan(scope)).await.unwrap());
        assert_eq!(store.lock().unwrap().mutations, before);
        let mut deleted = false;
        for _ in 0..40 {
            state = checkpoint(&state);
            deleted = state.delete(&client, "api").await.unwrap();
            if deleted {
                break;
            }
        }
        assert!(deleted);
        assert!(store.lock().unwrap().objects.is_empty());
    }
}
