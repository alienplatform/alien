//! An interceptor hook runs past its 100 ms deadline and is stopped there. Stopped in `matches`,
//! before the call is sent, the call fails with INTERCEPTOR_FAILED unsent, and the executor may
//! run the step again; on a multi-thread runtime and on a current-thread one. Stopped in
//! `modify_response`, after the cloud acted on the call, the call is sent once and fails for good.

use alien_azure_clients::azure::flexible_server::FlexibleServer;
use alien_error::Context;
use alien_infra::{DefaultPlatformServiceProvider, LoadedExtensions, PlatformServiceProvider};
use std::collections::HashMap;
use std::sync::Arc;

/// A hook that never returns.
const SPIN: &str = "(loop (br 0)) i64.const 0";

/// A hook that answers `matches` with yes, returns the request unchanged from `modify_request`,
/// and never returns from `modify_response`. It tells the three apart by the 16th byte of its
/// input, `{"op":"matches",`, `{"op":"modifyRequest"` or `{"op":"modifyResponse"`.
const SPIN_ON_RESPONSE: &str = r#"
    (if (i32.eq (i32.load8_u offset=15 (local.get 2)) (i32.const 44))
      (then
        ;; {"matches":true} at 512
        (i64.store (i32.const 512) (i64.const 7307199746910528123))
        (i64.store (i32.const 520) (i64.const 9035757361825456755))
        (return (i64.or (i64.const 512) (i64.shl (i64.const 16) (i64.const 32))))))
    (if (i32.eq (i32.load8_u offset=15 (local.get 2)) (i32.const 113))
      (then
        ;; {"op":"modifyRequest","request":{..}} becomes {"request":{..}}
        (i32.store8 offset=21 (local.get 2) (i32.const 123))
        (return (i64.or
          (i64.extend_i32_u (i32.add (local.get 2) (i32.const 21)))
          (i64.shl (i64.extend_i32_u (i32.sub (local.get 3) (i32.const 21))) (i64.const 32))))))
    (loop (br 0))
    i64.const 0"#;

fn provider(call: &str) -> DefaultPlatformServiceProvider {
    let stack: alien_core::Stack = serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {},
        "extensions": [{
            "name": "spin", "version": "1.0.0", "digest": "sha256:spin", "resourceTypes": {}
        }]
    }))
    .unwrap();
    DefaultPlatformServiceProvider::default()
        .with_extensions(
            &stack,
            Arc::new(LoadedExtensions::default().with_hooks(
                "spin",
                alien_deployment::testing::hooks_module(
                    "spin",
                    r#"[{"name":"all","kind":"azureInterceptor"}]"#,
                    call,
                ),
            )),
        )
        .unwrap()
}

fn config(base: &str) -> alien_azure_clients::AzureClientConfig {
    alien_azure_clients::AzureClientConfig {
        subscription_id: "sub".into(),
        tenant_id: "tenant".into(),
        region: Some("eastus".into()),
        credentials: alien_azure_clients::AzureCredentials::AccessToken {
            token: "azure-test-token".into(),
        },
        service_overrides: Some(alien_azure_clients::ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), base.to_string())]),
        }),
    }
}

fn server_config() -> FlexibleServer {
    serde_json::from_value(serde_json::json!({
        "location": "eastus",
        "sku": { "name": "Standard_B1ms", "tier": "Burstable" },
        "properties": { "version": "17", "administratorLogin": "alien",
            "storage": { "storageSizeGB": 32 }, "backup": { "backupRetentionDays": 7 },
            "network": { "publicNetworkAccess": "Disabled" } }
    }))
    .unwrap()
}

/// Whether the executor runs a step again after it failed with `error`, under a controller's
/// wrapper that says the opposite of the interceptor.
fn executor_retries(error: alien_error::AlienError<alien_azure_clients::ErrorData>) -> bool {
    let mut wrapped = Err::<(), _>(error)
        .context(alien_infra::ErrorData::CloudPlatformError {
            message: "Failed to create 'pg'".to_string(),
            resource_id: Some("pg".to_string()),
        })
        .unwrap_err();
    wrapped.retryable = !wrapped.retryable;
    alien_infra::step_error_is_retryable(&wrapped)
}

async fn run() {
    let server = httpmock::MockServer::start_async().await;
    let any = server
        .mock_async(|_when, then| {
            then.status(200)
                .header("content-type", "application/json")
                .body("{}");
        })
        .await;
    let config = config(&server.base_url());
    let provider = provider(SPIN);
    let started = std::time::Instant::now();
    let error = provider
        .get_azure_flexible_server_client(&config)
        .unwrap()
        .create_or_update_server("rg", "pg", &server_config())
        .await
        .unwrap_err();
    let elapsed = started.elapsed();
    assert_eq!(error.code, "INTERCEPTOR_FAILED", "{error}");
    assert_eq!(any.hits_async().await, 0);
    assert!(elapsed < std::time::Duration::from_secs(2), "{elapsed:?}");
    assert!(
        elapsed >= std::time::Duration::from_millis(100),
        "{elapsed:?}"
    );
    let chain = format!("{error:?}");
    assert!(chain.contains("HOOK_DEADLINE_EXCEEDED"), "{chain}");
    assert!(
        error.retryable,
        "a hook stopped at its deadline may finish on another try"
    );
    assert!(executor_retries(error));
}

async fn run_on_response() {
    let server = httpmock::MockServer::start_async().await;
    let create = server
        .mock_async(|_when, then| {
            then.status(200)
                .header("content-type", "application/json")
                .body("{}");
        })
        .await;
    let config = config(&server.base_url());
    let error = provider(SPIN_ON_RESPONSE)
        .get_azure_flexible_server_client(&config)
        .unwrap()
        .create_or_update_server("rg", "pg", &server_config())
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERCEPTOR_FAILED", "{error}");
    assert!(format!("{error:?}").contains("HOOK_DEADLINE_EXCEEDED"));
    assert_eq!(
        create.hits_async().await,
        1,
        "the cloud acted on the create; sending it again would create the server twice"
    );
    assert!(!error.retryable);
    assert!(!executor_retries(error));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_matches_past_its_deadline_fails_the_call_multi_thread() {
    run().await;
}

#[tokio::test]
async fn a_matches_past_its_deadline_fails_the_call_current_thread() {
    run().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_modify_response_past_its_deadline_is_sent_once_and_final_multi_thread() {
    run_on_response().await;
}

#[tokio::test]
async fn a_modify_response_past_its_deadline_is_sent_once_and_final_current_thread() {
    run_on_response().await;
}
