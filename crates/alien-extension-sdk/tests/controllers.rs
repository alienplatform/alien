//! What a controller's handler reads besides Alien's context: an AWS SDK config with the step's
//! credentials, and a dependency's outputs. The controller runs in the step protocol's host, as
//! `run_controllers` runs it, with no network. Also the docs' controller test, on a type
//! `#[alien_resource]` writes.

mod common;

use alien_extension_sdk::controller_prelude::*;
use alien_extension_sdk::AlienResource;
use alien_infra::step_protocol::protocol::codes;
use common::{
    aws_credentials, bucket, failed, host, refused, request, slow_host, stepped, Thing,
    ThingOutputs, FAR_DEADLINE,
};
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread")]
async fn an_aws_sdk_client_gets_the_steps_credentials_and_region() {
    let state = stepped(
        host(Some("bucket"))
            .handle(request(aws_credentials(), bucket(), FAR_DEADLINE))
            .await,
    );
    assert_eq!(state["region"], "eu-west-3");
    assert_eq!(state["accessKeyId"], "ASIASTEPKEY");
    assert_eq!(state["sessionToken"], "step-session");
    assert_eq!(state["bucketName"], "stack-bucket");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_step_on_a_cloud_without_credentials_is_refused() {
    let (_, message) = refused(
        host(None)
            .handle(request(Value::Null, bucket(), FAR_DEADLINE))
            .await,
    );
    assert!(message.contains("carries no credentials"), "{message}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dependency_the_step_does_not_carry_fails_the_step() {
    let reply = host(Some("bucket"))
        .handle(request(aws_credentials(), json!({}), FAR_DEADLINE))
        .await;
    assert_eq!(failed(reply).0, "DEPENDENCY_NOT_FOUND");
}

/// A resource is not its own dependency, so its own id reads as one the step doesn't carry.
#[tokio::test(flavor = "multi_thread")]
async fn the_resources_own_id_is_not_a_dependency() {
    let reply = host(Some("thing"))
        .handle(request(aws_credentials(), bucket(), FAR_DEADLINE))
        .await;
    assert_eq!(failed(reply).0, "DEPENDENCY_NOT_FOUND");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dependency_without_outputs_is_not_ready() {
    let dependencies = json!({ "bucket": {
        "resourceType": "storage", "status": "provisioning", "outputs": null, "bindingParams": null
    }});
    let reply = host(Some("bucket"))
        .handle(request(aws_credentials(), dependencies, FAR_DEADLINE))
        .await;
    assert_eq!(failed(reply).0, "DEPENDENCY_NOT_READY");
}

#[tokio::test(flavor = "multi_thread")]
async fn outputs_that_do_not_read_as_the_type_asked_for_fail_the_step() {
    let dependencies = json!({ "bucket": {
        "resourceType": "acme/other", "status": "running",
        "outputs": { "type": "acme/other", "somethingElse": 1 }, "bindingParams": null
    }});
    let reply = host(Some("bucket"))
        .handle(request(aws_credentials(), dependencies, FAR_DEADLINE))
        .await;
    let (code, message, _) = failed(reply);
    assert_eq!(code, "RESOURCE_CONFIG_INVALID");
    assert!(message.contains("the outputs of 'bucket'"), "{message}");
}

/// Outputs with no type tag aren't resource outputs; read as none, a running dependency would
/// look not ready on every retry.
#[tokio::test(flavor = "multi_thread")]
async fn outputs_that_are_not_resource_outputs_refuse_the_step() {
    let dependencies = json!({ "bucket": {
        "resourceType": "storage", "status": "running",
        "outputs": { "bucketName": "b" }, "bindingParams": null
    }});
    let (code, message) = refused(
        host(Some("bucket"))
            .handle(request(aws_credentials(), dependencies, FAR_DEADLINE))
            .await,
    );
    assert_eq!(code, codes::STEP_REFUSED);
    assert!(
        message.contains("the outputs of dependency 'bucket' are not resource outputs"),
        "{message}"
    );
}

/// Read as zero time left, an unreadable deadline would fail the step as a retryable deadline on
/// every retry.
#[tokio::test(flavor = "multi_thread")]
async fn a_deadline_that_does_not_parse_refuses_the_step() {
    let (code, message) = refused(
        host(None)
            .handle(request(aws_credentials(), json!({}), "next tuesday"))
            .await,
    );
    assert_eq!(code, codes::STEP_REFUSED);
    assert!(
        message.contains("the deadline 'next tuesday' is not an RFC 3339 time"),
        "{message}"
    );
}

/// A deadline already past leaves the handler no time: one that has to wait is cut at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_deadline_already_past_fails_the_step_as_retryable() {
    let reply = slow_host(1000)
        .handle(request(
            aws_credentials(),
            json!({}),
            "2001-01-01T00:00:00Z",
        ))
        .await;
    let (code, _, retryable) = failed(reply);
    assert_eq!(code, codes::STEP_DEADLINE);
    assert!(retryable);
}

#[test]
fn the_type_names_its_descriptor_consts() {
    assert_eq!(Thing::RESOURCE_TYPE.to_string(), "probe/thing");
    assert_eq!(
        <Thing as AlienResource>::LIFECYCLES,
        [alien_core::ResourceLifecycle::Live]
    );
}

// The docs' "Test controllers" shape, on a type `#[alien_resource]` writes.
#[controller]
pub struct LocalThingController {
    name: Option<String>,
}

#[controller]
impl LocalThingController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let thing = ctx.desired_resource_config::<Thing>()?;
        self.name = Some(thing.id.clone());
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = CreateFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    #[flow_entry(Update)]
    #[handler(state = UpdateStart, on_failure = CreateFailed, status = ResourceStatus::Updating)]
    async fn update_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = CreateFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        Some(ResourceOutputs::new(ThingOutputs {
            name: self.name.clone()?,
        }))
    }
}

#[tokio::test]
async fn the_docs_controller_test_runs_on_an_alien_resource_type() {
    let mut executor = SingleControllerExecutor::builder()
        .resource(Thing::new("thing".into()).build())
        .controller(LocalThingController::default())
        .platform(Platform::Aws)
        .build()
        .await
        .unwrap();
    executor
        .run_until_status(ResourceStatus::Running)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(executor.outputs().unwrap()).unwrap(),
        json!({ "type": "probe/thing", "name": "thing" })
    );
}
