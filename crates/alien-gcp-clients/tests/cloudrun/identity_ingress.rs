use super::CloudRunTestContext;
use alien_client_core::ErrorData;
use alien_gcp_clients::cloudrun::{CloudRunApi, CloudRunClient, Ingress, Service};
use alien_gcp_clients::longrunning::{Operation, OperationResult};
use alien_gcp_clients::{
    GcpClientConfig, GcpClientConfigExt, GcpCredentials, GcpImpersonationConfig,
};
use reqwest::Client;
use std::{collections::HashMap, env, time::Duration};
use test_context::test_context;
use tokio::time::{sleep, Instant};
use uuid::Uuid;

const TOKEN_LABEL: &str = "test-create-token";

fn required(value: Option<&str>, field: &str) -> Result<String, String> {
    value
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("Cloud Run response has no {field}"))
}

fn concurrency_conflict(error: &alien_client_core::Error) -> bool {
    matches!(
        error.error.as_ref(),
        Some(ErrorData::RemoteResourceConflict { .. })
            | Some(ErrorData::HttpResponseError {
                http_status: 409 | 412,
                ..
            })
    )
}

async fn wait(ctx: &CloudRunTestContext, operation: Operation) -> Result<(), String> {
    let name = required(operation.name.as_deref(), "operation name")?;
    let id = name.rsplit('/').next().ok_or("invalid operation name")?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let operation = ctx
            .client
            .get_operation(ctx.location.clone(), id.to_string())
            .await
            .map_err(|error| error.to_string())?;
        if let Some(OperationResult::Error { error }) = operation.result {
            return Err(format!("Cloud Run operation failed: {error:?}"));
        }
        if operation.done == Some(true) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("Timed out waiting for {name}"));
        }
        sleep(Duration::from_secs(2)).await;
    }
}

// A successful command response can still carry an asynchronously rejected
// operation. Check the final result before deciding whether a stale write won.
async fn expect_conflict(
    ctx: &CloudRunTestContext,
    result: alien_client_core::Result<Operation>,
    action: &str,
) -> Result<(), String> {
    let mut operation = match result {
        Err(error) if concurrency_conflict(&error) => return Ok(()),
        Err(error) => return Err(format!("Stale {action} failed unexpectedly: {error}")),
        Ok(operation) => operation,
    };
    let name = required(operation.name.as_deref(), "conflict operation name")?;
    let id = name.rsplit('/').next().ok_or("invalid operation name")?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if let Some(OperationResult::Error { error }) = operation.result {
            // google.rpc.Code: FAILED_PRECONDITION or ABORTED. Permission and
            // transport failures must never count as concurrency protection.
            return if matches!(error.code, 9 | 10) {
                Ok(())
            } else {
                Err(format!(
                    "Stale {action} operation failed unexpectedly: {error:?}"
                ))
            };
        }
        if operation.done == Some(true) {
            return Err(format!("Provider applied a stale {action} ETag"));
        }
        if Instant::now() >= deadline {
            return Err(format!("Timed out waiting for stale {action} rejection"));
        }
        sleep(Duration::from_secs(2)).await;
        operation = ctx
            .client
            .get_operation(ctx.location.clone(), id.to_string())
            .await
            .map_err(|error| error.to_string())?;
    }
}

async fn observe_native_access(client: &Client, uri: &str, public: bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let status = client
            .get(uri)
            .send()
            .await
            .map_err(|error| error.to_string())?
            .status();
        if (public && status.is_success()) || (!public && matches!(status.as_u16(), 403 | 404)) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Native URL access public={public} remained HTTP {status}"
            ));
        }
        sleep(Duration::from_secs(2)).await;
    }
}

/// Real-provider prerequisite for access-only worker updates. This proves native
/// URL denial, ETag fencing and revision preservation, not private ALB traffic
/// or the controller's restricted runtime permission boundary.
#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn access_only_updates_preserve_identity_revision_and_fence_stale_writes(
    ctx: &mut CloudRunTestContext,
) {
    let name = env::var("ALIEN_TEST_CLOUDRUN_IDENTITY_NAME")
        .unwrap_or_else(|_| ctx.generate_unique_service_name());
    let token = env::var("ALIEN_TEST_CLOUDRUN_IDENTITY_TOKEN")
        .map(|value| Uuid::parse_str(&value).expect("probe token must be a UUID"))
        .unwrap_or_else(|_| Uuid::new_v4())
        .to_string();
    println!("Cloud Run identity probe: service={name}; attempt={token}");
    let mut confirmed_uid = None;
    // Do not use the context's unconditional name-based fallback cleanup. A
    // replacement must never be deleted after this test's identity check fails.
    let observed: Result<(), String> = async {
        if let Ok(email) = env::var("ALIEN_TEST_CLOUDRUN_RUNTIME_SERVICE_ACCOUNT") {
            let source = GcpClientConfig {
                project_id: ctx.project_id.clone(), region: ctx.location.clone(),
                credentials: GcpCredentials::ServiceAccountKey {
                    json: env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY").map_err(|error| error.to_string())?,
                },
                service_overrides: None, project_number: None,
            };
            let runtime = source.impersonate(GcpImpersonationConfig {
                service_account_email: email, ..Default::default()
            }).await.map_err(|error| error.to_string())?;
            // All Cloud Run commands, polls and cleanup below use the restricted
            // identity. Source credentials only mint its refreshable token.
            ctx.client = CloudRunClient::new(Client::new(), runtime.clone());
            let bearer = runtime.get_bearer_token("https://cloudresourcemanager.googleapis.com/")
                .await.map_err(|error| error.to_string())?;
            let status = Client::builder().timeout(Duration::from_secs(10)).build()
                .map_err(|error| error.to_string())?
                .post(format!("https://cloudresourcemanager.googleapis.com/v1/projects/{}:getIamPolicy", ctx.project_id))
                .bearer_auth(bearer).json(&serde_json::json!({})).send().await
                .map_err(|error| error.to_string())?.status();
            if status.as_u16() != 403 {
                return Err(format!("Restricted runtime project IAM read expected403, got {status}"));
            }
            println!("Verified restricted runtime cannot read project IAM policy");
        }
        let mut service = ctx.create_basic_service(name.clone());
        if let Ok(email) = env::var("ALIEN_TEST_CLOUDRUN_EXECUTION_SERVICE_ACCOUNT") {
            service.template.as_mut().ok_or("Probe service has no template")?.service_account = Some(email);
        }
        service.ingress = Some(Ingress::IngressTrafficAll);
        service.invoker_iam_disabled = Some(true);
        service.labels = Some(HashMap::from([
            (TOKEN_LABEL.to_string(), token.clone()),
            ("test-purpose".to_string(), "endpoint-identity".to_string()),
        ]));
        let operation = ctx.client.create_service(ctx.location.clone(), name.clone(), service, None)
            .await.map_err(|error| error.to_string())?;
        wait(ctx, operation).await?;
        let original = ctx.client.get_service(ctx.location.clone(), name.clone())
            .await.map_err(|error| error.to_string())?;
        let uid = required(original.uid.as_deref(), "UID")?;
        confirmed_uid = Some(uid.clone());
        let revision = required(original.latest_created_revision.as_deref(), "created revision")?;
        let uri = required(original.uri.as_deref(), "canonical URI")?;
        let client = Client::builder().timeout(Duration::from_secs(10)).build()
            .map_err(|error| error.to_string())?;
        observe_native_access(&client, &uri, true).await?;
        let template = serde_json::to_value(&original.template).map_err(|error| error.to_string())?;
        let mut current = original;
        for (index, ingress) in [Ingress::IngressTrafficInternalLoadBalancer,
            Ingress::IngressTrafficInternalOnly, Ingress::IngressTrafficAll].into_iter().enumerate() {
            let previous_etag = required(current.etag.as_deref(), "ETag")?;
            let patch = Service::builder().etag(previous_etag.clone())
                .ingress(ingress.clone()).invoker_iam_disabled(true).build();
            let operation = ctx.client.patch_service(ctx.location.clone(), name.clone(), patch,
                Some("ingress,invokerIamDisabled,etag".to_string()), None, Some(false))
                .await.map_err(|error| error.to_string())?;
            wait(ctx, operation).await?;
            current = ctx.client.get_service(ctx.location.clone(), name.clone())
                .await.map_err(|error| error.to_string())?;
            if current.uid.as_deref() != Some(uid.as_str())
                || current.latest_created_revision.as_deref() != Some(revision.as_str())
                || current.latest_ready_revision.as_deref() != Some(revision.as_str())
                || current.uri.as_deref() != Some(uri.as_str()) || current.ingress != Some(ingress)
                || serde_json::to_value(&current.template).map_err(|error| error.to_string())? != template {
                return Err("Access-only update changed identity, revision, URI, template or failed to apply ingress".to_string());
            }
            if required(current.etag.as_deref(), "updated ETag")? == previous_etag {
                return Err("Access-only mutation did not advance its ETag".to_string());
            }
            observe_native_access(&client, &uri, index == 2).await?;
            if index == 0 {
                let stale_patch = Service::builder().etag(previous_etag.clone())
                    .ingress(Ingress::IngressTrafficAll).build();
                expect_conflict(ctx, ctx.client.patch_service(ctx.location.clone(), name.clone(), stale_patch,
                    Some("ingress,etag".to_string()), None, Some(false)).await, "patch").await?;
                expect_conflict(ctx, ctx.client.delete_service(ctx.location.clone(), name.clone(), None,
                    Some(previous_etag)).await, "delete").await?;
                let unchanged = ctx.client.get_service(ctx.location.clone(), name.clone())
                    .await.map_err(|error| error.to_string())?;
                if unchanged.uid != current.uid || unchanged.etag != current.etag
                    || unchanged.ingress != current.ingress {
                    return Err("Stale write changed the current service".to_string());
                }
            }
            println!("Verified access phase {index}: same service UID, revision and URI");
        }
        Ok(())
    }.await;
    let cleanup: Result<(), String> = async {
        let service = match ctx
            .client
            .get_service(ctx.location.clone(), name.clone())
            .await
        {
            Ok(service) => service,
            Err(error) if matches!(error.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                return Ok(())
            }
            Err(error) => return Err(error.to_string()),
        };
        if service
            .labels
            .as_ref()
            .and_then(|labels| labels.get(TOKEN_LABEL))
            != Some(&token)
            || confirmed_uid
                .as_ref()
                .is_some_and(|uid| service.uid.as_ref() != Some(uid))
        {
            return Err("Refusing cleanup of a foreign or replaced probe service".to_string());
        }
        let etag = required(service.etag.as_deref(), "cleanup ETag")?;
        let operation = ctx
            .client
            .delete_service(ctx.location.clone(), name.clone(), None, Some(etag))
            .await
            .map_err(|error| error.to_string())?;
        wait(ctx, operation).await?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match ctx
                .client
                .get_service(ctx.location.clone(), name.clone())
                .await
            {
                Err(error)
                    if matches!(error.error, Some(ErrorData::RemoteResourceNotFound { .. })) =>
                {
                    return Ok(())
                }
                Err(error) => return Err(error.to_string()),
                Ok(service) if service.uid != confirmed_uid && confirmed_uid.is_some() => {
                    return Err("Probe service was replaced during cleanup".to_string())
                }
                Ok(_) => {}
            }
            if Instant::now() >= deadline {
                return Err("Probe service remains after deletion".to_string());
            }
            sleep(Duration::from_secs(2)).await;
        }
    }
    .await;
    assert!(
        cleanup.is_ok(),
        "probe cleanup failed: {cleanup:?}; observation: {observed:?}"
    );
    println!("Probe service absence confirmed: {name}");
    assert!(
        observed.is_ok(),
        "identity/ingress qualification failed: {observed:?}"
    );
}
