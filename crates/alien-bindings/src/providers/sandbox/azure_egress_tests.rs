use super::*;
use crate::{
    sandbox_egress::tests::Harness,
    traits::{CreateSandboxRequest, Sandbox as SandboxTrait},
};
use alien_azure_clients::azure::sandbox_data_plane::{CreateSandbox, MockSandboxDataPlaneApi};
use alien_client_core::ErrorData as ClientError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::sync::Mutex;

type Record = Arc<Mutex<Option<Sandbox>>>;
type Request = Arc<Mutex<Option<CreateSandbox>>>;

fn provider(manager: SandboxEgressManager, corrupt: bool) -> (AzureSandbox, Record, Request) {
    let record: Record = Arc::new(Mutex::new(None));
    let request: Request = Arc::new(Mutex::new(None));
    let mut client = MockSandboxDataPlaneApi::new();
    let saved = record.clone();
    let submitted = request.clone();
    client
        .expect_create_sandbox()
        .times(1)
        .returning(move |_, req| {
            assert_eq!(
                req.customer_vnet_connection_name.as_deref(),
                Some("default")
            );
            let mut labels = req.labels.clone();
            if corrupt {
                labels.remove(IDENTITY_LABEL);
            }
            let sandbox = Sandbox {
                id: "vendor-id".into(),
                labels,
                vnet_connection_name: Some("default".into()),
                egress_policy: req.egress.clone(),
                state: Some("Running".into()),
            };
            *submitted.lock().unwrap() = Some(req);
            *saved.lock().unwrap() = Some(sandbox.clone());
            Ok(sandbox)
        });
    let saved = record.clone();
    client.expect_get_sandbox().returning(move |_, id| {
        assert_eq!(id, "vendor-id");
        saved.lock().unwrap().clone().ok_or_else(|| {
            AlienError::new(ClientError::HttpResponseError {
                message: "not found".into(),
                url: "https://example.invalid/sandboxes/vendor-id".into(),
                http_status: 404,
                http_request_text: None,
                http_response_text: None,
            })
        })
    });
    let saved = record.clone();
    client
        .expect_delete_sandbox()
        .times(1)
        .returning(move |_, id| {
            assert_eq!(id, "vendor-id");
            saved.lock().unwrap().take();
            Ok(())
        });
    let sandbox = AzureSandbox::new(
        Arc::new(client),
        "group".into(),
        "ubuntu".into(),
        alien_core::SandboxEgress::Internet,
        None,
        "1000m".into(),
        "2048Mi".into(),
        None,
    )
    .with_lifetime_ceiling(Some(600))
    .with_egress_manager(
        Arc::new(manager),
        Some(RuntimePolicy::Allow {
            allow: vec!["api.example.com".into()],
        }),
    );
    (sandbox, record, request)
}

#[tokio::test]
async fn durable_identity_drives_create_reconnect_update_and_termination() {
    let h = Harness::new().await;
    let (sandbox, record, request) = provider(h.reopen().await, false);
    let created = sandbox
        .create(CreateSandboxRequest {
            timeout_ms: Some(900_000),
            env: BTreeMap::from([
                ("HTTPS_PROXY".into(), "http://wrong.invalid".into()),
                ("no_proxy".into(), "*".into()),
            ]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(created.sandbox_id, "vendor-id");
    let req = request.lock().unwrap().clone().unwrap();
    let id = req.labels[IDENTITY_LABEL].clone();
    assert_ne!(id, "vendor-id");
    let proxy = req.environment["HTTPS_PROXY"]
        .strip_prefix("http://alien:")
        .unwrap();
    let token = proxy.split('@').next().unwrap();
    let claims: serde_json::Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(token.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(claims["sub"], id);
    assert_eq!(
        claims["exp"].as_i64().unwrap(),
        req.labels[SANDBOX_EXPIRY_LABEL].parse::<i64>().unwrap() / 1000
    );
    assert!(claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap() <= 600);
    assert_eq!(req.environment["no_proxy"], "localhost,127.0.0.1");
    assert_eq!(
        req.environment["http_proxy"],
        req.environment["HTTPS_PROXY"]
    );
    assert_eq!(
        h.policy(&id),
        Some(RuntimePolicy::Allow {
            allow: vec!["api.example.com".into()]
        })
    );
    assert_eq!(h.policy("vendor-id"), None);
    let floor = req.egress.unwrap();
    assert_eq!(floor.default_action, "Allow");
    assert_eq!(floor.traffic_inspection.as_deref(), Some("Partial"));
    assert_eq!(floor.host_rules[0].pattern, "127.0.0.1:3128");
    assert_eq!(floor.host_rules[1].action, "Deny");
    assert_eq!(floor.host_rules[1].pattern, "*");

    // Reopening the manager loses all in-process state; the provider label remains authoritative.
    let reopened = AzureSandbox {
        egress_manager: Some(Arc::new(h.reopen().await)),
        ..sandbox
    };
    let empty = RuntimePolicy::Allow { allow: vec![] };
    reopened
        .update_egress("vendor-id", empty.clone())
        .await
        .unwrap();
    assert_eq!(h.policy(&id), Some(empty));
    let resolved = reopened
        .get_or_create(CreateSandboxRequest {
            sandbox_id: Some("vendor-id".into()),
            egress: Some(RuntimePolicy::Internet),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(resolved.sandbox.sandbox_id, "vendor-id");
    assert_eq!(h.policy(&id), Some(RuntimePolicy::Internet));
    record.lock().unwrap().as_mut().unwrap().state = Some("Suspended".into());
    reopened
        .update_egress("vendor-id", RuntimePolicy::Allow { allow: vec![] })
        .await
        .unwrap();
    assert_eq!(h.policy(&id), Some(RuntimePolicy::Allow { allow: vec![] }));
    reopened.terminate("vendor-id").await.unwrap();
    assert!(record.lock().unwrap().is_none());
    assert_eq!(h.policy(&id), None);
}

#[tokio::test]
async fn missing_identity_on_readback_discards_the_sandbox_without_publishing_policy() {
    let h = Harness::new().await;
    let (sandbox, record, request) = provider(h.reopen().await, true);
    let error = sandbox
        .create(CreateSandboxRequest::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED");
    assert!(record.lock().unwrap().is_none());
    let id = request.lock().unwrap().as_ref().unwrap().labels[IDENTITY_LABEL].clone();
    assert_eq!(h.policy(&id), None);
}

#[tokio::test]
async fn a_changed_native_floor_is_refused_before_policy_updates() {
    let h = Harness::new().await;
    let (sandbox, record, request) = provider(h.reopen().await, false);
    sandbox
        .create(CreateSandboxRequest::default())
        .await
        .unwrap();
    let id = request.lock().unwrap().as_ref().unwrap().labels[IDENTITY_LABEL].clone();
    record
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .egress_policy
        .as_mut()
        .unwrap()
        .host_rules
        .reverse();
    let error = sandbox
        .update_egress("vendor-id", RuntimePolicy::Internet)
        .await
        .unwrap_err();
    assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED");
    assert_eq!(
        h.policy(&id),
        Some(RuntimePolicy::Allow {
            allow: vec!["api.example.com".into()]
        })
    );
    // Teardown still revokes and deletes a sandbox whose floor no longer matches.
    sandbox.terminate("vendor-id").await.unwrap();
    assert_eq!(h.policy(&id), None);
}

#[tokio::test]
async fn admin_failure_discards_create_and_does_not_prevent_termination() {
    let h = Harness::new().await;
    let (sandbox, record, request) = provider(h.reopen().await, false);
    h.refuse_writes(true);
    sandbox
        .create(CreateSandboxRequest::default())
        .await
        .expect_err("policy registration refused");
    assert!(record.lock().unwrap().is_none());
    let id = request.lock().unwrap().as_ref().unwrap().labels[IDENTITY_LABEL].clone();
    assert_eq!(h.policy(&id), None);

    h.refuse_writes(false);
    let (sandbox, record, request) = provider(h.reopen().await, false);
    sandbox
        .create(CreateSandboxRequest::default())
        .await
        .unwrap();
    let id = request.lock().unwrap().as_ref().unwrap().labels[IDENTITY_LABEL].clone();
    h.refuse_writes(true);
    sandbox
        .terminate("vendor-id")
        .await
        .expect_err("revocation failure reported");
    assert!(
        record.lock().unwrap().is_none(),
        "vendor deletion still completes"
    );
    assert!(
        h.policy(&id).is_some(),
        "failed revocation is not claimed as success"
    );
}
