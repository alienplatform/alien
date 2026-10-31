//! Stop automatic DynamoDB expiry before relinquishing runtime access.

use std::collections::{HashMap, HashSet};

use alien_aws_clients::dynamodb::{
    DescribeTableRequest, DescribeTimeToLiveRequest, ListTagsOfResourceRequest,
    TimeToLiveSpecification, UpdateTimeToLiveRequest,
};
use alien_client_core::ErrorData as ClientError;
use alien_core::{
    ClientConfig, ExternalBindings, Kv, Platform, ResourceLifecycle, ResourceStatus, StackState,
};
use alien_error::{AlienError, Context, ContextError};

use super::AwsKvController;
use crate::{
    core::aws_ownership::is_tagged_as_ours, ErrorData, PlatformServiceProvider, Result,
    StackResourceStateExt,
};

/// Whether installed KV data needs runtime expiry protection. Deletion receipt
/// selection uses this same boundary so unrelated legacy receipts remain valid.
pub fn retained_kv_requires_runtime_protection(
    state: &StackState,
    id: &str,
    external_bindings: &ExternalBindings,
) -> bool {
    retained_kv_requires_protection(state, id, external_bindings, &[ResourceLifecycle::Live])
}

fn retained_kv_requires_protection(
    state: &StackState,
    id: &str,
    external_bindings: &ExternalBindings,
    lifecycles: &[ResourceLifecycle],
) -> bool {
    let Some(resource) = state.resources.get(id) else {
        return false;
    };
    let has_kv = std::iter::once(&resource.config)
        .chain(resource.previous_config.iter())
        .any(|config| config.downcast_ref::<Kv>().is_some());
    let installed_bucket = resource
        .internal_state
        .as_ref()
        .and_then(|value| value.get("type"))
        .and_then(serde_json::Value::as_str)
        == Some("AwsStorageController")
        && std::iter::once(&resource.config)
            .chain(resource.previous_config.iter())
            .any(|config| config.downcast_ref::<alien_core::Storage>().is_some());
    has_kv
        && !installed_bucket
        && resource.status != ResourceStatus::Deleted
        && resource
            .lifecycle
            .is_none_or(|lifecycle| lifecycles.contains(&lifecycle))
        && resource.controller_platform.unwrap_or(state.platform) == Platform::Aws
        && !(resource.internal_state.is_none()
            && matches!(
                external_bindings.0.get(id),
                Some(alien_core::ExternalBinding::Kv(_))
            ))
}

/// Returns false while TTL is transitioning. The caller must persist/yield without
/// deleting access resources, then read again. Only tables whose lifecycle is in
/// `lifecycles` are touched: runtime cleanup passes Live, and the setup authority
/// passes both when it finishes retention. Externally bound data is never touched.
/// This proves API state only: AWS can process already queued TTL deletions for
/// approximately 30 minutes after disabling TTL.
/// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/time-to-live-ttl-how-to.html
pub async fn protect_retained_kv(
    state: &StackState,
    retained_ids: &[String],
    client_config: &ClientConfig,
    provider: &dyn PlatformServiceProvider,
    external_bindings: &ExternalBindings,
    lifecycles: &[ResourceLifecycle],
) -> Result<bool> {
    let mut ready = true;
    for id in retained_ids {
        let resource = state
            .resources
            .get(id)
            .ok_or_else(|| invalid(id, "Retained resource is missing from installed state"))?;
        if !retained_kv_requires_protection(state, id, external_bindings, lifecycles) {
            continue;
        }
        if resource.internal_state.is_none() {
            external_bindings.get_kv(id).context(cloud_error(id))?;
        }
        if resource.lifecycle.is_none() {
            return Err(invalid(
                id,
                "KV lifecycle is required before protecting retained data",
            ));
        }
        let controller = resource.get_internal_controller_typed::<AwsKvController>()?;
        // A rejected request or a lost response can leave no checkpoint identity.
        // Read the deterministic requested name before deciding whether data exists.
        let requested_name = alien_permissions::aws_resource_policies::aws_prefixed_resource_name(
            &state.resource_prefix,
            id,
        );
        let name = controller.table_name.as_ref().unwrap_or(&requested_name);
        let config = client_config
            .aws_config()
            .ok_or_else(|| invalid(id, "AWS runtime credentials are required"))?;
        let client = provider.get_aws_dynamodb_client(config).await?;
        let table = match client
            .describe_table(DescribeTableRequest {
                table_name: name.clone(),
            })
            .await
        {
            Ok(response) => response.table,
            Err(error) if missing(&error) => continue,
            Err(error) => {
                return Err(error.context(ErrorData::DurableResourceLookupFailed {
                    resource_id: id.clone(),
                    message: format!("DynamoDB table {name}"),
                }))
            }
        };
        let arn = table
            .table_arn
            .ok_or_else(|| invalid(id, "DynamoDB did not return the retained table ARN"))?;
        if controller
            .table_arn
            .as_ref()
            .is_some_and(|saved| saved != &arn)
        {
            return Err(invalid(
                id,
                "DynamoDB table identity differs from the retained checkpoint",
            ));
        }
        let mut tags = HashMap::new();
        let mut tokens = HashSet::new();
        let mut next_token = None;
        loop {
            let page = client
                .list_tags_of_resource(ListTagsOfResourceRequest {
                    resource_arn: arn.clone(),
                    next_token,
                })
                .await
                .context(cloud_error(id))?;
            for tag in page.tags {
                if tags.insert(tag.key, tag.value).is_some() {
                    return Err(invalid(id, "DynamoDB returned duplicate ownership tags"));
                }
            }
            next_token = page.next_token;
            let Some(token) = next_token.as_ref() else {
                break;
            };
            if tokens.len() >= 100 || !tokens.insert(token.clone()) {
                return Err(invalid(
                    id,
                    "DynamoDB ownership tag pagination did not finish",
                ));
            }
        }
        if !is_tagged_as_ours(
            &tags,
            &alien_core::standard_resource_tags(&state.resource_prefix, id),
        ) {
            return Err(invalid(
                id,
                "Table ownership tags do not match the retained resource",
            ));
        }
        let description = client
            .describe_time_to_live(DescribeTimeToLiveRequest {
                table_name: name.clone(),
            })
            .await
            .context(cloud_error(id))?
            .time_to_live_description
            .ok_or_else(|| {
                invalid(
                    id,
                    "DynamoDB did not return TTL metadata for the retained table",
                )
            })?;
        match description.time_to_live_status.as_deref() {
            Some("DISABLED") => {}
            Some("DISABLING" | "ENABLING") => ready = false,
            Some("ENABLED") => {
                let attribute_name = description
                    .attribute_name
                    .filter(|attribute| !attribute.is_empty())
                    .ok_or_else(|| {
                        invalid(id, "DynamoDB did not return the enabled TTL attribute")
                    })?;
                match client
                    .update_time_to_live(UpdateTimeToLiveRequest {
                        table_name: name.clone(),
                        time_to_live_specification: TimeToLiveSpecification {
                            attribute_name,
                            enabled: false,
                        },
                    })
                    .await
                {
                    Ok(_) => {}
                    // DynamoDB refuses another TTL change for up to an hour after the last one,
                    // which setup made when it created the table. A deletion soon after setup
                    // waits for that window like any other transition.
                    Err(error) if ttl_change_window_open(&error) => {}
                    Err(error) => return Err(error.context(cloud_error(id))),
                }
                // The command response is not completion. Never issue a second update
                // while AWS is ENABLING/DISABLING, including after a lost response.
                ready = false;
            }
            _ => {
                return Err(invalid(
                    id,
                    "DynamoDB returned an unknown TTL state for the retained table",
                ))
            }
        }
    }
    Ok(ready)
}

/// DynamoDB answers a TTL change made too soon after the previous one with a
/// ValidationException naming the repeated modification.
fn ttl_change_window_open(error: &AlienError<ClientError>) -> bool {
    matches!(
        &error.error,
        Some(ClientError::GenericError { message })
            if message.to_ascii_lowercase().contains("modified multiple times")
    )
}
fn missing(error: &AlienError<ClientError>) -> bool {
    matches!(
        error.error,
        Some(ClientError::RemoteResourceNotFound { .. })
    )
}
fn invalid(id: &str, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        resource_id: Some(id.into()),
        message: message.into(),
    })
}
fn cloud_error(id: &str) -> ErrorData {
    ErrorData::CloudPlatformError { resource_id: Some(id.into()), message: "Failed to stop retained table expiry; restore the required setup grants and retry deletion".into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockPlatformServiceProvider;
    use alien_aws_clients::{dynamodb::MockDynamoDbApi, AwsClientConfigExt};
    use alien_core::{AwsClientConfig, Resource, StackResourceState};
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    const ARN: &str = "arn:aws:dynamodb:us-east-1:123456789012:table/owned-data";

    fn installed() -> StackState {
        let mut state = StackState::new(Platform::Aws);
        state.resource_prefix = "owned".into();
        let mut resource = StackResourceState::new_pending(
            "kv".into(),
            Resource::new(Kv::new("data".into()).build()),
            None,
            vec![],
        );
        resource.lifecycle = Some(ResourceLifecycle::Live);
        resource.status = ResourceStatus::Running;
        resource
            .set_internal_controller(Some(Box::new(AwsKvController {
                table_name: Some("owned-data".into()),
                table_arn: Some(ARN.into()),
                ..Default::default()
            })))
            .unwrap();
        state.resources.insert("data".into(), resource);
        state
    }
    fn ownership(api: &mut MockDynamoDbApi, foreign: bool) {
        api.expect_describe_table().returning(|request| {
            assert_eq!(request.table_name, "owned-data");
            Ok(serde_json::from_value(json!({"Table":{"TableArn":ARN}})).unwrap())
        });
        api.expect_list_tags_of_resource()
            .returning(move |request| {
                assert_eq!(request.resource_arn, ARN);
                // Require both pages before deciding ownership; preserve unrelated tags.
                let response = if request.next_token.is_none() {
                    json!({"Tags":[{"Key":"owner-note","Value":"untouched"}], "NextToken":"second"})
                } else {
                    assert_eq!(request.next_token.as_deref(), Some("second"));
                    let tags: Vec<Value> = alien_core::standard_resource_tags(
                        if foreign { "foreign" } else { "owned" },
                        "data",
                    )
                    .into_iter()
                    .map(|(key, value)| json!({"Key":key,"Value":value}))
                    .collect();
                    json!({"Tags":tags})
                };
                Ok(serde_json::from_value(response).unwrap())
            });
    }
    fn provider(api: MockDynamoDbApi) -> MockPlatformServiceProvider {
        let api = Arc::new(api);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_dynamodb_client()
            .returning(move |_| Ok(api.clone()));
        provider
    }
    async fn step(state: &StackState, provider: &MockPlatformServiceProvider) -> Result<bool> {
        protect_retained_kv(
            state,
            &["data".into()],
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live],
        )
        .await
    }

    #[tokio::test]
    async fn lost_disable_response_reloads_waits_and_confirms_without_resending() {
        let mut api = MockDynamoDbApi::new();
        ownership(&mut api, false);
        let status = Arc::new(Mutex::new("ENABLING"));
        let read = status.clone();
        api.expect_describe_time_to_live().returning(move |request| {
            assert_eq!(request.table_name, "owned-data");
            Ok(serde_json::from_value(json!({"TimeToLiveDescription":{"TimeToLiveStatus":*read.lock().unwrap(),"AttributeName":"custom-expiry"}})).unwrap())
        });
        let write = status.clone();
        api.expect_update_time_to_live()
            .times(1)
            .returning(move |request| {
                assert_eq!(request.table_name, "owned-data");
                assert_eq!(
                    request.time_to_live_specification.attribute_name,
                    "custom-expiry"
                );
                assert!(!request.time_to_live_specification.enabled);
                *write.lock().unwrap() = "DISABLING";
                Err(AlienError::new(ClientError::RemoteServiceUnavailable {
                    message: "response lost after disable".into(),
                }))
            });
        let provider = provider(api);
        let state = installed();
        let saved = serde_json::to_value(&state).unwrap();
        assert!(!step(&state, &provider).await.unwrap());
        *status.lock().unwrap() = "ENABLED";
        assert!(step(&state, &provider).await.is_err());
        let reloaded = serde_json::from_value(saved.clone()).unwrap();
        assert!(!step(&reloaded, &provider).await.unwrap());
        *status.lock().unwrap() = "DISABLED";
        for _ in 0..2 {
            assert!(step(&reloaded, &provider).await.unwrap());
        }
        assert_eq!(serde_json::to_value(&reloaded).unwrap(), saved);
    }

    /// Setup enabled TTL when it created the table, so a deletion within the next hour is
    /// refused the change. It waits and disables TTL once DynamoDB allows it.
    #[tokio::test]
    async fn deletion_soon_after_setup_waits_for_the_ttl_change_window() {
        let mut api = MockDynamoDbApi::new();
        ownership(&mut api, false);
        let status = Arc::new(Mutex::new("ENABLED"));
        let read = status.clone();
        api.expect_describe_time_to_live().returning(move |_| {
            Ok(serde_json::from_value(json!({"TimeToLiveDescription":{"TimeToLiveStatus":*read.lock().unwrap(),"AttributeName":"ttl"}})).unwrap())
        });
        let refusals = Arc::new(Mutex::new(1));
        let write = status.clone();
        api.expect_update_time_to_live()
            .times(2)
            .returning(move |request| {
                assert!(!request.time_to_live_specification.enabled);
                let mut refusals = refusals.lock().unwrap();
                if *refusals > 0 {
                    *refusals -= 1;
                    return Err(AlienError::new(ClientError::GenericError {
                        message:
                            "Time to live has been modified multiple times within a fixed interval"
                                .into(),
                    }));
                }
                *write.lock().unwrap() = "DISABLING";
                Ok(alien_aws_clients::dynamodb::UpdateTimeToLiveResponse {
                    time_to_live_specification: None,
                })
            });
        let provider = provider(api);
        let state = installed();
        assert!(
            !step(&state, &provider).await.unwrap(),
            "refused change waits"
        );
        assert!(
            !step(&state, &provider).await.unwrap(),
            "accepted change waits"
        );
        *status.lock().unwrap() = "DISABLED";
        assert!(step(&state, &provider).await.unwrap());
    }

    #[tokio::test]
    async fn other_ttl_validation_errors_still_fail() {
        let mut api = MockDynamoDbApi::new();
        ownership(&mut api, false);
        api.expect_describe_time_to_live().returning(|_| {
            Ok(serde_json::from_value(json!({"TimeToLiveDescription":{"TimeToLiveStatus":"ENABLED","AttributeName":"ttl"}})).unwrap())
        });
        api.expect_update_time_to_live().times(1).returning(|_| {
            Err(AlienError::new(ClientError::GenericError {
                message: "One or more parameter values were invalid".into(),
            }))
        });
        assert!(step(&installed(), &provider(api)).await.is_err());
    }

    #[tokio::test]
    async fn interrupted_type_change_protects_the_installed_table_not_the_desired_bucket() {
        let mut state = installed();
        let entry = state.resources.get_mut("data").unwrap();
        entry.previous_config = Some(entry.config.clone());
        entry.config = Resource::new(alien_core::Storage::new("data".into()).build());
        let mut api = MockDynamoDbApi::new();
        ownership(&mut api, false);
        api.expect_describe_time_to_live().times(1).returning(|_| {
            Ok(serde_json::from_value(
                json!({"TimeToLiveDescription":{"TimeToLiveStatus":"DISABLED"}}),
            )
            .unwrap())
        });
        let provider = provider(api);
        let state: StackState =
            serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        assert!(step(&state, &provider).await.unwrap());
        assert!(crate::protect_retained_storage(
            &state,
            &["data".into()],
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live],
        )
        .await
        .unwrap());

        let mut bucket = state.clone();
        let entry = bucket.resources.get_mut("data").unwrap();
        entry.previous_config = Some(entry.config.clone());
        entry.config = Resource::new(Kv::new("data".into()).build());
        entry
            .set_internal_controller(Some(Box::new(crate::AwsStorageController::mock_ready(
                "data",
            ))))
            .unwrap();
        // The inverse transition must not try DynamoDB with a bucket checkpoint.
        assert!(step(&bucket, &MockPlatformServiceProvider::new())
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn command_success_requires_separate_readback() {
        let mut api = MockDynamoDbApi::new();
        ownership(&mut api, false);
        api.expect_describe_time_to_live().times(1).returning(|_| Ok(serde_json::from_value(json!({"TimeToLiveDescription":{"TimeToLiveStatus":"ENABLED","AttributeName":"ttl"}})).unwrap()));
        api.expect_update_time_to_live().times(1).returning(|_| {
            Ok(serde_json::from_value(
                json!({"TimeToLiveSpecification":{"Enabled":false,"AttributeName":"ttl"}}),
            )
            .unwrap())
        });
        assert!(!step(&installed(), &provider(api)).await.unwrap());
    }

    #[tokio::test]
    async fn foreign_table_and_unknown_or_denied_metadata_never_mutate() {
        let mut foreign = MockDynamoDbApi::new();
        ownership(&mut foreign, true);
        assert_eq!(
            step(&installed(), &provider(foreign))
                .await
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
        for response in [
            json!({}),
            json!({"TimeToLiveDescription":{}}),
            json!({"TimeToLiveDescription":{"TimeToLiveStatus":"UNKNOWN"}}),
            json!({"TimeToLiveDescription":{"TimeToLiveStatus":"ENABLED"}}),
            json!(null),
        ] {
            let mut api = MockDynamoDbApi::new();
            ownership(&mut api, false);
            api.expect_describe_time_to_live().returning(move |_| {
                if response.is_null() {
                    Err(AlienError::new(ClientError::RemoteAccessDenied {
                        resource_type: "table".into(),
                        resource_name: "owned-data".into(),
                    }))
                } else {
                    Ok(serde_json::from_value(response.clone()).unwrap())
                }
            });
            let state = installed();
            let original = serde_json::to_value(&state).unwrap();
            assert!(step(&state, &provider(api)).await.is_err());
            assert_eq!(serde_json::to_value(state).unwrap(), original);
        }
    }

    #[tokio::test]
    async fn frozen_external_and_explicit_erase_preserve_setup_authority() {
        let mut state = installed();
        state.resources.get_mut("data").unwrap().lifecycle = Some(ResourceLifecycle::Frozen);
        let no_cloud = MockPlatformServiceProvider::new();
        assert!(step(&state, &no_cloud).await.unwrap());
        state.resources.get_mut("data").unwrap().lifecycle = Some(ResourceLifecycle::Live);
        assert!(protect_retained_kv(
            &state,
            &[],
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &no_cloud,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live]
        )
        .await
        .unwrap());
        state.resources.get_mut("data").unwrap().internal_state = None;
        let bindings = ExternalBindings(HashMap::from([(
            "data".into(),
            alien_core::ExternalBinding::Kv(alien_core::KvBinding::redis("redis://external")),
        )]));
        assert!(protect_retained_kv(
            &state,
            &["data".into()],
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &no_cloud,
            &bindings,
            &[ResourceLifecycle::Live]
        )
        .await
        .unwrap());
    }

    #[tokio::test]
    async fn uncertain_create_without_saved_identity_looks_up_requested_table() {
        let mut state = installed();
        state
            .resources
            .get_mut("data")
            .unwrap()
            .set_internal_controller(Some(Box::new(AwsKvController::default())))
            .unwrap();
        let mut api = MockDynamoDbApi::new();
        api.expect_describe_table().times(1).returning(|request| {
            assert_eq!(request.table_name, "owned-data");
            Err(AlienError::new(ClientError::RemoteResourceNotFound {
                resource_type: "table".into(),
                resource_name: request.table_name,
            }))
        });
        assert!(step(&state, &provider(api)).await.unwrap());
    }

    #[tokio::test]
    async fn unknown_table_create_found_is_protected_and_denied_lookup_can_retry() {
        let mut state = installed();
        state
            .resources
            .get_mut("data")
            .unwrap()
            .set_internal_controller(Some(Box::new(AwsKvController::default())))
            .unwrap();
        let mut denied = MockDynamoDbApi::new();
        denied
            .expect_describe_table()
            .times(1)
            .returning(|request| {
                Err(AlienError::new(ClientError::RemoteAccessDenied {
                    resource_type: "table".into(),
                    resource_name: request.table_name,
                }))
            });
        let error = step(&state, &provider(denied)).await.unwrap_err();
        assert!(error.retryable);
        assert_eq!(error.code, "DURABLE_RESOURCE_LOOKUP_FAILED");
        let mut found = MockDynamoDbApi::new();
        ownership(&mut found, false);
        found
            .expect_describe_time_to_live()
            .times(1)
            .returning(|request| {
                assert_eq!(request.table_name, "owned-data");
                Ok(serde_json::from_value(
                    json!({"TimeToLiveDescription":{"TimeToLiveStatus":"DISABLED"}}),
                )
                .unwrap())
            });
        assert!(step(&state, &provider(found)).await.unwrap());
    }

    #[tokio::test]
    async fn verified_missing_table_is_safe_but_denied_lookup_is_not() {
        for absent in [true, false] {
            let mut api = MockDynamoDbApi::new();
            api.expect_describe_table().returning(move |_| {
                Err(AlienError::new(if absent {
                    ClientError::RemoteResourceNotFound {
                        resource_type: "table".into(),
                        resource_name: "owned-data".into(),
                    }
                } else {
                    ClientError::RemoteAccessDenied {
                        resource_type: "table".into(),
                        resource_name: "owned-data".into(),
                    }
                }))
            });
            assert_eq!(step(&installed(), &provider(api)).await.is_ok(), absent);
        }
    }
}
