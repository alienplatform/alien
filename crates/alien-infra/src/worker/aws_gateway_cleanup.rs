//! Invoke only the immutable setup-owned task retirement capability.

use alien_aws_clients::lambda::{InvokeRequest, LambdaApi};
use alien_core::{RemoteStackManagement, RemoteStackManagementOutputs, ResourceLifecycle};
use alien_error::{AlienError, Context};
use serde::Deserialize;

use crate::{ErrorData, ResourceControllerContext, Result};

#[derive(Clone, Copy)]
pub(super) enum CleanupAction {
    Deregister,
    Delete,
}

pub(super) struct GatewayCleanup<'a> {
    pub lambda: &'a dyn LambdaApi,
    pub function_arn: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetirementReceipt {
    task_definition_arn: String,
    status: String,
}

impl GatewayCleanup<'_> {
    pub async fn retire(&self, action: CleanupAction, task_arn: &str) -> Result<()> {
        let version = self
            .function_arn
            .rsplit(':')
            .next()
            .filter(|version| {
                !version.is_empty()
                    && !version.starts_with('0')
                    && version.bytes().all(|byte| byte.is_ascii_digit())
            })
            .ok_or_else(|| invalid("Gateway retirement requires a published setup version"))?;
        let (action, expected_status) = match action {
            CleanupAction::Deregister => ("deregister", "INACTIVE"),
            CleanupAction::Delete => ("delete", "DELETE_IN_PROGRESS"),
        };
        let response = self
            .lambda
            .invoke(
                InvokeRequest::builder()
                    .function_name(self.function_arn.to_string())
                    .payload(
                        serde_json::json!({"action": action, "taskDefinitionArn": task_arn})
                            .to_string()
                            .into_bytes(),
                    )
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Invoke setup-owned gateway retirement".to_string(),
                resource_id: None,
            })?;
        if response.status_code != 200
            || response.function_error.is_some()
            || response.executed_version.as_deref() != Some(version)
            || response.payload.len() > 1024
        {
            return Err(invalid(
                "Gateway retirement did not return its published version receipt",
            ));
        }
        // Never attach untrusted function payloads to diagnostics: even an
        // incorrectly installed helper must not disclose task configuration.
        let receipt: RetirementReceipt = serde_json::from_slice(&response.payload)
            .map_err(|_| invalid("Gateway retirement returned an invalid receipt"))?;
        if receipt.task_definition_arn != task_arn || receipt.status != expected_status {
            return Err(invalid(
                "Gateway retirement returned a different identity or status",
            ));
        }
        // Acceptance is not absence. The caller keeps its receipt and polls ECS.
        Ok(())
    }
}

pub(super) fn installed_function(ctx: &ResourceControllerContext<'_>) -> Result<String> {
    let mut management = ctx.state.resources.iter().filter(|(_, state)| {
        state.resource_type == RemoteStackManagement::RESOURCE_TYPE.as_ref()
            && state.lifecycle == Some(ResourceLifecycle::Frozen)
    });
    let (id, _) = management.next().ok_or_else(setup_required)?;
    if management.next().is_some() {
        return Err(setup_required());
    }
    let outputs = ctx
        .state
        .get_resource_outputs::<RemoteStackManagementOutputs>(id)
        .map_err(|_| setup_required())?;
    let function = outputs
        .aws_worker_gateway_cleanup_function_arn
        .as_ref()
        .ok_or_else(setup_required)?;
    let aws = ctx.get_aws_config()?;
    if outputs.management_resource_id.split(':').nth(4) != Some(aws.account_id.as_str()) {
        return Err(setup_required());
    }
    alien_core::aws_worker_gateway::validate_cleanup_function_arn(
        function,
        &outputs.management_resource_id,
        &format!("{}-management", ctx.resource_prefix),
        &aws.region,
    )
    .map_err(|_| setup_required())?;
    Ok(function.clone())
}

fn setup_required() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SetupRequired {
        resource_id: None,
        message: "Rerun setup to register gateway retirement for this deployment before changing private worker access".to_string(),
    })
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.to_string(),
        resource_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::lambda::{InvocationType, InvokeResponse, MockLambdaApi};
    use serde_json::json;

    const FUNCTION: &str =
        "arn:aws:lambda:us-east-1:123456789012:function:_gateway-cleanup-example:7";
    const TASK: &str =
        "arn:aws:ecs:us-east-1:123456789012:task-definition/example-jobs-gateway-01:1";

    fn response(payload: serde_json::Value) -> InvokeResponse {
        InvokeResponse {
            status_code: 200,
            function_error: None,
            log_result: None,
            payload: payload.to_string().into_bytes(),
            executed_version: Some("7".to_string()),
        }
    }

    #[tokio::test]
    async fn retirement_invokes_only_the_published_version_with_exact_operation_and_task() {
        for (action, name, status) in [
            (CleanupAction::Deregister, "deregister", "INACTIVE"),
            (CleanupAction::Delete, "delete", "DELETE_IN_PROGRESS"),
        ] {
            let mut lambda = MockLambdaApi::new();
            lambda.expect_invoke().times(1).return_once(move |request| {
                assert_eq!(request.function_name, FUNCTION);
                assert!(matches!(
                    request.invocation_type,
                    InvocationType::RequestResponse
                ));
                assert!(request.qualifier.is_none());
                assert!(request.log_type.is_none());
                assert!(request.client_context.is_none());
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&request.payload).unwrap(),
                    json!({"action": name, "taskDefinitionArn": TASK})
                );
                Ok(response(
                    json!({"taskDefinitionArn": TASK, "status": status}),
                ))
            });
            GatewayCleanup {
                lambda: &lambda,
                function_arn: FUNCTION,
            }
            .retire(action, TASK)
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn uncertain_or_mismatched_replies_are_refused_without_exposing_payloads() {
        let expected = json!({"taskDefinitionArn": TASK, "status": "DELETE_IN_PROGRESS"});
        let mut cases = vec![
            response(json!({"status": "REFUSED"})),
            response(json!({"taskDefinitionArn": "foreign", "status": "DELETE_IN_PROGRESS"})),
            response(json!({"taskDefinitionArn": TASK, "status": "ACTIVE"})),
            response(
                json!({"taskDefinitionArn": TASK, "status": "DELETE_IN_PROGRESS", "configuration": "sensitive-marker"}),
            ),
        ];
        let mut wrong_version = response(expected.clone());
        wrong_version.executed_version = Some("8".into());
        cases.push(wrong_version);
        let mut missing_version = response(expected.clone());
        missing_version.executed_version = None;
        cases.push(missing_version);
        let mut failed = response(expected.clone());
        failed.function_error = Some("sensitive-marker".into());
        cases.push(failed);
        let mut asynchronous = response(expected.clone());
        asynchronous.status_code = 202;
        cases.push(asynchronous);
        let mut oversized = response(expected);
        oversized.payload = vec![b'x'; 1025];
        cases.push(oversized);
        for response in cases {
            let mut lambda = MockLambdaApi::new();
            lambda
                .expect_invoke()
                .times(1)
                .return_once(move |_| Ok(response));
            let error = GatewayCleanup {
                lambda: &lambda,
                function_arn: FUNCTION,
            }
            .retire(CleanupAction::Delete, TASK)
            .await
            .unwrap_err();
            assert!(!format!("{error:?}").contains("sensitive-marker"));
        }
    }

    #[tokio::test]
    async fn mutable_function_references_are_rejected_before_invocation() {
        for qualifier in ["$LATEST", "current", "0", ""] {
            let function = FUNCTION.replace(":7", &format!(":{qualifier}"));
            let lambda = MockLambdaApi::new();
            assert!(
                GatewayCleanup {
                    lambda: &lambda,
                    function_arn: &function
                }
                .retire(CleanupAction::Delete, TASK)
                .await
                .is_err()
            );
        }
    }
}
