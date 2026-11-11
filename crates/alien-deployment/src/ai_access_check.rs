//! Setup-time check that the cloud lets the account call each model an AI resource declares.
//!
//! It runs once, with the credentials the customer's admin supplies for setup. The management
//! identity never gets a grant for it. The answer is stored in `RuntimeMetadata` and reported by
//! the AI controller's heartbeat.

use alien_aws_clients::{BedrockRuntimeApi, BedrockRuntimeClient, InvokeCheck};
use alien_core::{
    ai_catalog, Ai, AiAccessCheck, AiAccessCheckResult, AiAccessTest, AiHeartbeatData,
    ClientConfig, Platform, ResourceHeartbeat, ResourceHeartbeatData, Stack,
};
use chrono::Utc;
use tracing::warn;

/// Check every declared model of every AI resource in `stack`. Empty when the stack declares none
/// or the platform has no check yet.
pub async fn check_declared_ai_models(
    stack: &Stack,
    platform: Platform,
    client_config: &ClientConfig,
) -> Vec<AiAccessCheck> {
    let declared = declared_models(stack);
    if declared.is_empty() || platform != Platform::Aws {
        return Vec::new();
    }
    let Some(aws_config) = client_config.aws_config() else {
        return Vec::new();
    };
    let region = aws_config.region.clone();
    let credentials =
        match alien_aws_clients::AwsCredentialProvider::from_config(aws_config.clone()).await {
            Ok(credentials) => credentials,
            Err(error) => {
                warn!(%error, "AI access check skipped: could not build setup credentials");
                return declared
                    .into_iter()
                    .map(|(resource_id, model_id)| {
                        not_checked(resource_id, model_id, "setup credentials unavailable")
                    })
                    .collect();
            }
        };
    let client = BedrockRuntimeClient::new(reqwest::Client::new(), credentials);
    check_with(&client, &region, declared).await
}

/// `(resource id, model id)` for each model an AI resource declares.
fn declared_models(stack: &Stack) -> Vec<(String, String)> {
    stack
        .resources()
        .filter_map(|(id, entry)| entry.config.downcast_ref::<Ai>().map(|ai| (id, ai)))
        .flat_map(|(id, ai)| {
            ai.models
                .iter()
                .map(move |model| (id.clone(), model.id.clone()))
        })
        .collect()
}

pub(crate) async fn check_with(
    client: &dyn BedrockRuntimeApi,
    region: &str,
    declared: Vec<(String, String)>,
) -> Vec<AiAccessCheck> {
    let mut checks = Vec::with_capacity(declared.len());
    for (resource_id, model_id) in declared {
        let Some(model) = ai_catalog::resolve_for(&model_id, Platform::Aws) else {
            checks.push(not_checked(
                resource_id,
                model_id,
                "model is not in the supported catalog",
            ));
            continue;
        };
        // The OpenAI-compatible (Responses-only) models are not served through Converse.
        if model.provider_api == ai_catalog::ProviderApi::OpenAiResponses {
            checks.push(not_checked(
                resource_id,
                model_id,
                "this model cannot be checked with a test call",
            ));
            continue;
        }
        // Claude is invoked through the geo inference profile, the same id the gateway uses.
        let invoke_id = if model.provider_api == ai_catalog::ProviderApi::Anthropic {
            ai_catalog::bedrock_inference_profile_id(region, model.upstream_id)
        } else {
            model.upstream_id.to_string()
        };
        let (result, detail) = match client.check_invoke(&invoke_id).await {
            InvokeCheck::Verified => (AiAccessCheckResult::Verified, None),
            InvokeCheck::Refused { code } => (AiAccessCheckResult::Refused, Some(code)),
            InvokeCheck::NotChecked { reason } => (AiAccessCheckResult::NotChecked, Some(reason)),
        };
        checks.push(AiAccessCheck {
            resource_id,
            model_id,
            result,
            detail,
            tested_at: Utc::now(),
        });
    }
    checks
}

fn not_checked(resource_id: String, model_id: String, reason: &str) -> AiAccessCheck {
    AiAccessCheck {
        resource_id,
        model_id,
        result: AiAccessCheckResult::NotChecked,
        detail: Some(reason.to_string()),
        tested_at: Utc::now(),
    }
}

/// Put the setup-time answers on the AI controller's heartbeat. The controller's own refresh only
/// reads the control plane and reports `not-checked`; a setup answer must not be lost to it.
/// A `not-checked` setup answer changes nothing.
///
/// A check applies only to the exact resource and model it tested, and only while `stack` still
/// declares that model: after a release changes the list, a model without a fresh check shows
/// `not-checked`, never an older result.
pub(crate) fn apply_checks(
    heartbeats: &mut [ResourceHeartbeat],
    checks: &[AiAccessCheck],
    stack: Option<&Stack>,
) {
    if checks.is_empty() {
        return;
    }
    let Some(stack) = stack else { return };
    let declared = declared_models(stack);
    for heartbeat in heartbeats {
        let ResourceHeartbeatData::Ai(AiHeartbeatData::AwsBedrock(data)) = &mut heartbeat.data
        else {
            continue;
        };
        for check in checks.iter().filter(|c| {
            c.resource_id == heartbeat.resource_id
                && declared.contains(&(c.resource_id.clone(), c.model_id.clone()))
        }) {
            let Some(model) = ai_catalog::resolve_for(&check.model_id, Platform::Aws) else {
                continue;
            };
            let Some(observed) = data
                .availability
                .models
                .iter_mut()
                .find(|m| m.public_model_id == model.public_id)
            else {
                continue;
            };
            match check.result {
                AiAccessCheckResult::Verified => {
                    observed.access_test = AiAccessTest::Verified;
                    observed.tested_at = Some(check.tested_at);
                }
                AiAccessCheckResult::Refused => {
                    observed.access_test = AiAccessTest::Failed;
                    observed.tested_at = Some(check.tested_at);
                    observed.error_code = check.detail.clone();
                }
                AiAccessCheckResult::NotChecked => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::bedrock_runtime::MockBedrockRuntimeApi;

    #[tokio::test]
    async fn refusal_is_reported_and_claude_uses_the_inference_profile() {
        let mut client = MockBedrockRuntimeApi::new();
        client
            .expect_check_invoke()
            .withf(|id| id.starts_with("us.anthropic."))
            .returning(|_| InvokeCheck::Refused {
                code: "AccessDeniedException".into(),
            });
        let checks = check_with(
            &client,
            "us-west-2",
            vec![("llm".into(), "claude-sonnet-4-6".into())],
        )
        .await;
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].result, AiAccessCheckResult::Refused);
        assert_eq!(checks[0].detail.as_deref(), Some("AccessDeniedException"));
    }

    #[tokio::test]
    async fn checker_without_permission_is_not_checked_never_refused() {
        let mut client = MockBedrockRuntimeApi::new();
        client
            .expect_check_invoke()
            .returning(|_| InvokeCheck::NotChecked {
                reason: "not authorized".into(),
            });
        let checks = check_with(
            &client,
            "us-west-2",
            vec![("llm".into(), "claude-sonnet-4-6".into())],
        )
        .await;
        assert_eq!(checks[0].result, AiAccessCheckResult::NotChecked);
    }

    fn bedrock_heartbeat(resource_id: &str, public_id: &str) -> ResourceHeartbeat {
        use alien_core::*;
        ResourceHeartbeat {
            deployment_id: None,
            resource_id: resource_id.into(),
            resource_type: Ai::RESOURCE_TYPE,
            controller_platform: Platform::Aws,
            backend: HeartbeatBackend::Aws,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Ai(AiHeartbeatData::AwsBedrock(
                AwsBedrockAiHeartbeatData {
                    status: AiHeartbeatStatus {
                        health: ObservedHealth::Healthy,
                        lifecycle: ProviderLifecycleState::Running,
                        message: None,
                        stale: false,
                        partial: false,
                        collection_issues: vec![],
                    },
                    region: "us-west-2".into(),
                    availability: AiAvailabilityObservation {
                        source: AiAvailabilitySource::AwsBedrock,
                        catalog_revision: String::new(),
                        location: None,
                        models: vec![AiModelAvailabilityObservation {
                            public_model_id: public_id.into(),
                            client_apis: vec![],
                            availability: AiModelAvailability::Available,
                            blockers: vec![],
                            access_test: AiAccessTest::NotChecked,
                            tested_at: None,
                            error_code: None,
                            quota_warnings: Vec::new(),
                        }],
                    },
                },
            )),
            raw: vec![],
        }
    }

    fn stack_declaring(model: &str) -> Stack {
        use alien_core::{AiModel, ResourceLifecycle};
        Stack::new("s".to_string())
            .add(
                Ai::new("llm".to_string())
                    .models(vec![AiModel {
                        id: model.to_string(),
                        required: true,
                    }])
                    .build(),
                ResourceLifecycle::Live,
            )
            .build()
    }

    fn access_of(heartbeat: &ResourceHeartbeat) -> AiAccessTest {
        match &heartbeat.data {
            ResourceHeartbeatData::Ai(AiHeartbeatData::AwsBedrock(d)) => {
                d.availability.models[0].access_test
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn setup_answers_land_on_the_heartbeat_and_not_checked_changes_nothing() {
        let check = |result| AiAccessCheck {
            resource_id: "llm".into(),
            model_id: "claude-sonnet-4-6".into(),
            result,
            detail: Some("AccessDeniedException".into()),
            tested_at: Utc::now(),
        };
        let stack = stack_declaring("claude-sonnet-4-6");
        let mut beats = vec![bedrock_heartbeat("llm", "claude-sonnet-4.6")];
        apply_checks(
            &mut beats,
            &[check(AiAccessCheckResult::Refused)],
            Some(&stack),
        );
        assert_eq!(access_of(&beats[0]), AiAccessTest::Failed);
        apply_checks(
            &mut beats,
            &[check(AiAccessCheckResult::Verified)],
            Some(&stack),
        );
        assert_eq!(access_of(&beats[0]), AiAccessTest::Verified);
        apply_checks(
            &mut beats,
            &[check(AiAccessCheckResult::NotChecked)],
            Some(&stack),
        );
        assert_eq!(access_of(&beats[0]), AiAccessTest::Verified);
        let mut other = vec![bedrock_heartbeat("other", "claude-sonnet-4.6")];
        apply_checks(
            &mut other,
            &[check(AiAccessCheckResult::Refused)],
            Some(&stack),
        );
        assert_eq!(access_of(&other[0]), AiAccessTest::NotChecked);
    }

    #[test]
    fn a_result_for_a_model_the_release_no_longer_declares_is_dropped() {
        let stack = stack_declaring("claude-haiku-4-5");
        let mut beats = vec![bedrock_heartbeat("llm", "claude-sonnet-4.6")];
        let old = AiAccessCheck {
            resource_id: "llm".into(),
            model_id: "claude-sonnet-4-6".into(),
            result: AiAccessCheckResult::Verified,
            detail: None,
            tested_at: Utc::now(),
        };
        apply_checks(&mut beats, &[old], Some(&stack));
        assert_eq!(access_of(&beats[0]), AiAccessTest::NotChecked);
    }

    #[test]
    fn metadata_without_checks_still_parses_and_empty_checks_are_not_serialized() {
        let old: alien_core::RuntimeMetadata = serde_json::from_str("{}").unwrap();
        assert!(old.ai_access_checks.is_empty());
        let json = serde_json::to_value(&old).unwrap();
        assert!(json.get("aiAccessChecks").is_none());
    }

    #[tokio::test]
    async fn unknown_model_is_not_checked_without_a_call() {
        let client = MockBedrockRuntimeApi::new();
        let checks = check_with(&client, "us-west-2", vec![("llm".into(), "no-such".into())]).await;
        assert_eq!(checks[0].result, AiAccessCheckResult::NotChecked);
    }
}
