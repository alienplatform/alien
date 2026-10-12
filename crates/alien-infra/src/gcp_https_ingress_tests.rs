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
    fail_certificate_delete: bool,
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
        certificate: GcpIngressCertificate::Managed {
            certificate_pem: certificate,
            private_key_pem: "runtime-only-private-key",
        },
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
        desired.certificate = GcpIngressCertificate::Custom {
            references: &references,
        };
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
        url_map: record("url"),
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
