use super::*;
use alien_gcp_clients::gcp::compute::{Backend, MockComputeApi, OperationStatus};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Provider {
    objects: BTreeMap<String, Value>,
    mutations: usize,
    lose_backend_response: bool,
    fail_certificate_delete: bool,
}

type Store = Arc<Mutex<Provider>>;

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
        value["address"] = json!("10.20.0.10");
    }
    if kind == "backend" {
        value["fingerprint"] = json!("fresh");
    }
    let mut store = store.lock().unwrap();
    let key = format!("{kind}/{name}");
    assert!(!store.objects.contains_key(&key), "duplicate create {key}");
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

fn provider(store: &Store) -> MockComputeApi {
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

fn plan(certificate: &str) -> GcpPrivateHttpsPlan<'_> {
    GcpPrivateHttpsPlan {
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
        certificate: GcpPrivateIngressCertificate::Managed {
            certificate_pem: certificate,
            private_key_pem: "runtime-only-private-key",
        },
    }
}

fn checkpoint(state: &GcpPrivateIngressState) -> GcpPrivateIngressState {
    let json = serde_json::to_string(state).unwrap();
    assert!(!json.contains("runtime-only-private-key"));
    serde_json::from_str(&json).unwrap()
}

async fn ready(
    state: &mut GcpPrivateIngressState,
    client: &MockComputeApi,
    store: &Store,
    certificate: &str,
) {
    for _ in 0..60 {
        *state = checkpoint(state);
        let before = store.lock().unwrap().mutations;
        let complete = state.reconcile(client, plan(certificate)).await.unwrap();
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
    let mut state = GcpPrivateIngressState::new(&plan("certificate-one"));
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
    assert_eq!(state.private_ip, None);
}

#[tokio::test]
async fn foreign_child_is_refused_on_retry_and_delete() {
    let store = Arc::new(Mutex::new(Provider::default()));
    let client = provider(&store);
    let mut state = GcpPrivateIngressState::new(&plan("certificate"));
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
    let mut state = GcpPrivateIngressState::new(&plan("certificate"));
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
    let mut state = GcpPrivateIngressState::new(&plan("certificate"));
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
    let mut state = GcpPrivateIngressState::new(&plan("certificate"));
    ready(&mut state, &client, &store, "certificate").await;
    let references =
        vec!["projects/example/regions/us-central1/sslCertificates/customer".to_string()];
    let mut complete = false;
    for _ in 0..30 {
        state = checkpoint(&state);
        let mut desired = plan("unused");
        desired.certificate = GcpPrivateIngressCertificate::Custom {
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
