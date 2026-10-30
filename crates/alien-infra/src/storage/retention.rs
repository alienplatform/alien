//! Protect retained storage before removing runtime access.

use alien_aws_clients::s3::S3Api;
use alien_azure_clients::azure::models::blob::{
    BlobContainer, ContainerProperties, ContainerPropertiesPublicAccess,
};
use alien_client_core::ErrorData as ClientError;
use alien_core::{
    ClientConfig, ExternalBindings, Platform, ResourceLifecycle, ResourceStatus, StackState,
    Storage,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_gcp_clients::gcs::{Bucket, GcsApi, IamConfiguration, Lifecycle};
use serde_json::Value;

use super::{AwsStorageController, AzureStorageController, GcpStorageController};
use crate::{
    core::aws_ownership::is_tagged_as_ours, infra_requirements::azure_utils, ErrorData,
    PlatformServiceProvider, Result, StackResourceStateExt,
};

/// Stop S3/GCS lifecycle actions and revoke declared public reads on retained storage.
///
/// Only storage whose lifecycle is in `lifecycles` is touched: runtime cleanup passes Live,
/// and the setup authority passes both when it finishes retention, because Frozen storage is
/// setup-owned. This operation does not certify removal of workload or management access.
/// Every retry re-reads provider state; saved resource identities and recovery data stay intact.
pub async fn protect_retained_storage(
    state: &StackState,
    retained_ids: &[String],
    client_config: &ClientConfig,
    provider: &dyn PlatformServiceProvider,
    external_bindings: &ExternalBindings,
    lifecycles: &[ResourceLifecycle],
) -> Result<()> {
    for id in retained_ids {
        let resource = state
            .resources
            .get(id)
            .ok_or_else(|| invalid(id, "Retained resource is missing from installed state"))?;
        let Some(storage) = resource
            .config
            .downcast_ref::<Storage>()
            .or_else(|| resource.previous_config.as_ref()?.downcast_ref::<Storage>())
        else {
            continue;
        };
        // During a type change the installed table, rather than the desired
        // bucket, is protected by the KV retention path.
        if resource
            .internal_state
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
            == Some("AwsKvController")
            && std::iter::once(&resource.config)
                .chain(resource.previous_config.iter())
                .any(|config| config.downcast_ref::<alien_core::Kv>().is_some())
        {
            continue;
        }
        // A binding without a controller was adopted, not provisioned by this deployment.
        // Adding a binding later must not exempt a resource we actually created.
        if resource.internal_state.is_none()
            && external_bindings
                .get_storage(id)
                .context(cloud_error(id))?
                .is_some()
        {
            continue;
        }
        let was_public = storage.public_read
            || resource
                .previous_config
                .as_ref()
                .and_then(|previous| previous.downcast_ref::<Storage>())
                .is_some_and(|previous| previous.public_read);
        let platform = resource.controller_platform.unwrap_or(state.platform);
        let has_local_notifications = platform == Platform::Local
            && (!storage.notifications.is_empty()
                || resource
                    .previous_config
                    .as_ref()
                    .and_then(|previous| previous.downcast_ref::<Storage>())
                    .is_some_and(|previous| !previous.notifications.is_empty()));
        if (!was_public
            && !has_local_notifications
            && !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure))
            || resource.status == ResourceStatus::Deleted
        {
            continue;
        }
        match resource.lifecycle {
            Some(lifecycle) if lifecycles.contains(&lifecycle) => {}
            Some(_) => continue,
            None => {
                return Err(invalid(
                    id,
                    "Storage lifecycle is required before protecting retained data",
                ));
            }
        }
        match platform {
            Platform::Aws => {
                let controller =
                    resource.get_internal_controller_typed::<AwsStorageController>()?;
                let name = controller.bucket_name.as_deref().ok_or_else(|| {
                    invalid(
                        id,
                        "Restore the recorded bucket identity before protecting retained data",
                    )
                })?;
                let config = client_config
                    .aws_config()
                    .ok_or_else(|| invalid(id, "AWS runtime credentials are required"))?;
                let client = provider.get_aws_s3_client(config).await?;
                // Legacy checkpoints may contain a name saved before a failed create.
                let tags = match client.get_bucket_tagging(name).await {
                    Ok(tags) => tags,
                    Err(error) if missing(&error) => continue,
                    Err(error) => return Err(error.context(cloud_error(id))),
                };
                if !is_tagged_as_ours(
                    &tags,
                    &alien_core::standard_resource_tags(&state.resource_prefix, id),
                ) {
                    return Err(invalid(
                        id,
                        "Bucket ownership tags do not match the retained resource",
                    ));
                }
                stop_s3_lifecycle(client.as_ref(), name, id).await?;
                if was_public {
                    revoke_s3_public_read(client.as_ref(), name, id).await?;
                }
            }
            Platform::Gcp => {
                let controller =
                    resource.get_internal_controller_typed::<GcpStorageController>()?;
                let name = controller.bucket_name.as_deref().ok_or_else(|| {
                    invalid(
                        id,
                        "Restore the recorded bucket identity before protecting retained data",
                    )
                })?;
                let config = client_config
                    .gcp_config()
                    .ok_or_else(|| invalid(id, "GCP runtime credentials are required"))?;
                let client = provider.get_gcp_gcs_client(config)?;
                let observed = match client.get_bucket(name.to_string()).await {
                    Ok(bucket) => bucket,
                    Err(error) if missing(&error) => continue,
                    Err(error) => return Err(error.context(cloud_error(id))),
                };
                if controller.notifications_managed
                    || !storage.notifications.is_empty()
                    || resource
                        .previous_config
                        .as_ref()
                        .and_then(|previous| previous.downcast_ref::<Storage>())
                        .is_some_and(|previous| !previous.notifications.is_empty())
                {
                    super::gcp_notifications::remove_owned(client.as_ref(), name, id).await?;
                }
                if !was_public {
                    stop_gcs_lifecycle(client.as_ref(), name, id, &observed).await?;
                    continue;
                }
                if observed
                    .iam_configuration
                    .as_ref()
                    .and_then(|iam| iam.public_access_prevention.as_deref())
                    != Some("enforced")
                {
                    // Preserve UBLA and private/conditional IAM grants. PAP also blocks old
                    // public ACL grants without changing the bucket's ACL ownership model.
                    client
                        .update_bucket(
                            name.to_string(),
                            Bucket {
                                iam_configuration: Some(IamConfiguration {
                                    uniform_bucket_level_access: None,
                                    public_access_prevention: Some("enforced".to_string()),
                                }),
                                ..Default::default()
                            },
                        )
                        .await
                        .context(cloud_error(id))?;
                }
                super::gcp::update_public_viewer(client.as_ref(), name, id, false).await?;
                stop_gcs_lifecycle(client.as_ref(), name, id, &observed).await?;
            }
            Platform::Azure => {
                let controller =
                    resource.get_internal_controller_typed::<AzureStorageController>()?;
                let name = controller.container_name.as_deref().ok_or_else(|| {
                    invalid(
                        id,
                        "Restore the recorded container identity before revoking public access",
                    )
                })?;
                let account = controller.storage_account_name.as_deref().ok_or_else(|| invalid(id, "Restore the recorded storage account identity before revoking public access"))?;
                let group = azure_utils::get_resource_group_name(state)?;
                let config = client_config
                    .azure_config()
                    .ok_or_else(|| invalid(id, "Azure runtime credentials are required"))?;
                if controller.notifications_managed
                    || !storage.notifications.is_empty()
                    || resource
                        .previous_config
                        .as_ref()
                        .and_then(|previous| previous.downcast_ref::<Storage>())
                        .is_some_and(|previous| !previous.notifications.is_empty())
                {
                    let notifications = provider.get_azure_event_grid_client(config)?;
                    let source = super::azure_notifications::source_id(
                        &config.subscription_id,
                        &group,
                        account,
                    );
                    super::azure_notifications::remove_owned(
                        notifications.as_ref(),
                        &source,
                        name,
                        id,
                    )
                    .await?;
                }
                let client = provider.get_azure_blob_container_client(config)?;
                let observed = match client.get_blob_container(&group, account, name).await {
                    Ok(container) => container,
                    Err(error) if missing(&error) => continue,
                    Err(error) => return Err(error.context(cloud_error(id))),
                };
                // The account policy is setup-owned: runtime verifies safety instead of
                // rewriting shared rules with a container's permissions.
                match client.get_management_policy(&group, account).await {
                    Ok(policy) => {
                        let policy = serde_json::to_value(policy)
                            .into_alien_error()
                            .context(cloud_error(id))?;
                        verify_azure_retention_policy(&policy, name, id)?;
                    }
                    Err(error) if missing(&error) => {}
                    Err(error) => return Err(error.context(cloud_error(id))),
                }
                if !was_public {
                    continue;
                }
                if observed
                    .properties
                    .as_ref()
                    .and_then(|properties| properties.public_access.as_ref())
                    == Some(&ContainerPropertiesPublicAccess::None)
                {
                    continue;
                }
                // Patch this container only: the shared account can serve other workloads.
                client
                    .update_blob_container(
                        &group,
                        account,
                        name,
                        &BlobContainer {
                            properties: Some(ContainerProperties {
                                public_access: Some(ContainerPropertiesPublicAccess::None),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    )
                    .await
                    .context(cloud_error(id))?;
            }
            Platform::Local =>
            {
                #[cfg(feature = "local")]
                if has_local_notifications {
                    let manager = provider
                        .get_local_storage_manager()
                        .ok_or_else(|| invalid(id, "Local storage manager is unavailable"))?;
                    manager
                        .stop_notifications(id)
                        .await
                        .context(cloud_error(id))?;
                }
            }
            Platform::Test => {}
            _ => {
                return Err(invalid(
                    id,
                    "Public storage revocation requires a supported cloud retention path",
                ));
            }
        }
    }
    Ok(())
}

/// Require applicable Azure lifecycle deletion rules to be disabled before handoff.
fn verify_azure_retention_policy(policy: &Value, container: &str, id: &str) -> Result<()> {
    let rules = policy
        .pointer("/properties/policy/rules")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(id, "Azure lifecycle policy has no readable rule inventory"))?;
    let prefix = format!("{container}/");
    for rule in rules {
        let filters = rule
            .pointer("/definition/filters/prefixMatch")
            .and_then(Value::as_array);
        let applies = match filters {
            Some(filters) if !filters.is_empty() => filters.iter().any(|value| {
                value
                    .as_str()
                    .is_none_or(|filter| prefix.starts_with(filter) || filter.starts_with(&prefix))
            }),
            _ => true,
        };
        if !applies {
            continue;
        }
        let actions = rule
            .pointer("/definition/actions")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                invalid(
                    id,
                    "Azure lifecycle policy has an unreadable applicable action",
                )
            })?;
        let deletes = actions.iter().any(|(kind, action)| {
            !matches!(kind.as_str(), "baseBlob" | "snapshot" | "version")
                || action.get("delete").is_some_and(|delete| !delete.is_null())
        });
        if !deletes {
            continue;
        }
        if rule.get("enabled").and_then(Value::as_bool) != Some(false) {
            return Err(AlienError::new(ErrorData::SetupRerunRequired {
                resource_id: id.to_string(),
                message: "Disable Azure lifecycle delete rules affecting this retained container using the original setup/account administrator, then retry deletion. Shared account policies are not rewritten by runtime; retention is not complete.".into(),
            }));
        }
    }
    Ok(())
}

fn missing(error: &AlienError<ClientError>) -> bool {
    matches!(
        error.error,
        Some(ClientError::RemoteResourceNotFound { .. })
    )
}

fn invalid(id: &str, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        resource_id: Some(id.to_string()),
        message: message.to_string(),
    })
}

fn cloud_error(id: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        resource_id: Some(id.to_string()),
        message: "Failed to protect retained storage; restore the required setup grants and retry deletion".to_string(),
    }
}

async fn stop_gcs_lifecycle(
    client: &dyn GcsApi,
    bucket: &str,
    id: &str,
    observed: &Bucket,
) -> Result<()> {
    let has_rules = |bucket: &Bucket| {
        bucket
            .lifecycle
            .as_ref()
            .and_then(|lifecycle| lifecycle.rule.as_ref())
            .is_some_and(|rules| !rules.is_empty())
    };
    if !has_rules(observed) {
        return Ok(());
    }
    // This is an owned bucket, not an account-level policy. Remove all lifecycle
    // actions without round-tripping incomplete models of foreign rule conditions.
    // Explicit [] is required: an empty lifecycle object preserves existing rules.
    client
        .update_bucket(
            bucket.to_string(),
            Bucket {
                lifecycle: Some(Lifecycle { rule: Some(vec![]) }),
                ..Default::default()
            },
        )
        .await
        .context(cloud_error(id))?;
    let confirmed = client
        .get_bucket(bucket.to_string())
        .await
        .context(cloud_error(id))?;
    if has_rules(&confirmed) {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            resource_id: Some(id.to_string()),
            message: "GCS still reports lifecycle rules on the retained bucket; retry before removing access".to_string(),
        }));
    }
    Ok(())
}

// Stop the whole lifecycle on this owned bucket, including noncurrent-version expiry
// and transitions. Rewriting the limited lifecycle model could silently drop provider
// fields. Never touch a shared account policy or a setup-owned/adopted bucket here.
async fn stop_s3_lifecycle(client: &dyn S3Api, bucket: &str, id: &str) -> Result<()> {
    match client.get_bucket_lifecycle_configuration(bucket).await {
        Ok(configuration) if configuration.rules.is_empty() => return Ok(()),
        Ok(_) => {}
        Err(error) if missing(&error) => return Ok(()),
        Err(error) => return Err(error.context(cloud_error(id))),
    }
    match client.delete_bucket_lifecycle(bucket).await {
        Ok(()) => {}
        Err(error) if missing(&error) => return Ok(()),
        Err(error) => return Err(error.context(cloud_error(id))),
    }
    // Confirm removal before revoking the access needed to repair it. S3 applies
    // lifecycle changes asynchronously; this readback is not an immediate expiry guarantee.
    match client.get_bucket_lifecycle_configuration(bucket).await {
        Ok(configuration) if configuration.rules.is_empty() => Ok(()),
        Err(error) if missing(&error) => Ok(()),
        Err(error) => Err(error.context(cloud_error(id))),
        Ok(_) => Err(AlienError::new(ErrorData::CloudPlatformError {
            resource_id: Some(id.to_string()),
            message: "S3 still reports lifecycle rules on the retained bucket; retry before removing access".to_string(),
        })),
    }
}

async fn revoke_s3_public_read(client: &dyn S3Api, bucket: &str, id: &str) -> Result<()> {
    let response = match client.get_bucket_policy(bucket).await {
        Ok(policy) => policy,
        Err(error)
            if matches!(
                error.error,
                Some(ClientError::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error.context(cloud_error(id))),
    };
    let mut policy: Value = serde_json::from_str(&response.policy)
        .into_alien_error()
        .context(cloud_error(id))?;
    let statements = policy
        .get_mut("Statement")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            invalid(
                id,
                "Bucket policy statements must be an array before public access can be revoked",
            )
        })?;
    let before = statements.len();
    // S3 accepts scalar and singleton-array fields and both anonymous principal
    // forms. Remove only this bucket's unconditional public GetObject grant;
    // keep conditions, deny statements and other workload principals intact.
    statements.retain(|statement| !is_public_read(statement, bucket));
    if statements.len() == before {
        return Ok(());
    }
    if statements.is_empty() {
        match client.delete_bucket_policy(bucket).await {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.context(cloud_error(id))),
        }
    } else {
        client
            .put_bucket_policy(bucket, &policy.to_string())
            .await
            .context(cloud_error(id))
    }
}

fn is_public_read(statement: &Value, bucket: &str) -> bool {
    let Some(fields) = statement.as_object() else {
        return false;
    };
    if fields.keys().any(|key| {
        !matches!(
            key.as_str(),
            "Sid" | "Effect" | "Principal" | "Action" | "Resource"
        )
    }) {
        return false;
    }
    let principal = &statement["Principal"];
    let public = singleton(principal, "*")
        || principal.as_object().is_some_and(|fields| {
            fields.len() == 1 && fields.get("AWS").is_some_and(|value| singleton(value, "*"))
        });
    statement["Effect"] == "Allow"
        && public
        && singleton(&statement["Action"], "s3:GetObject")
        && singleton(&statement["Resource"], &format!("arn:aws:s3:::{bucket}/*"))
}

fn singleton(value: &Value, expected: &str) -> bool {
    value.as_str() == Some(expected)
        || value
            .as_array()
            .is_some_and(|values| values.len() == 1 && values[0].as_str() == Some(expected))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use alien_aws_clients::{
        s3::{GetBucketPolicyOutput, MockS3Api},
        AwsClientConfigExt,
    };
    use alien_azure_clients::{blob_containers::MockBlobContainerApi, AzureClientConfigExt};
    use alien_core::{
        AwsClientConfig, AzureClientConfig, AzureResourceGroup, AzureResourceGroupOutputs,
        GcpClientConfig, Resource, ResourceOutputs, StackResourceState,
    };
    use alien_gcp_clients::{gcs::MockGcsApi, iam::IamPolicy, GcpClientConfigExt};
    use serde_json::json;

    use super::*;
    use crate::{MockPlatformServiceProvider, ResourceController};

    fn installed(platform: Platform, controller: Box<dyn ResourceController>) -> StackState {
        let mut state = StackState::new(platform);
        state.resource_prefix = "owned".into();
        let mut resource = StackResourceState::new_pending(
            "storage".into(),
            Resource::new(Storage::new("data".into()).public_read(true).build()),
            None,
            vec![],
        );
        resource.lifecycle = Some(ResourceLifecycle::Live);
        resource.status = ResourceStatus::Running;
        resource.set_internal_controller(Some(controller)).unwrap();
        state.resources.insert("data".into(), resource);
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap()
    }

    fn transient() -> AlienError<ClientError> {
        AlienError::new(ClientError::RemoteServiceUnavailable {
            message: "response lost".into(),
        })
    }

    #[tokio::test]
    async fn s3_retention_preserves_foreign_grants_and_recovers_lost_policy_response() {
        let public = json!({"Effect":"Allow", "Principal":"*", "Action":"s3:GetObject", "Resource":"arn:aws:s3:::owned-data/*"});
        let foreign = json!({"Sid":"ForeignWorkload", "Effect":"Allow", "Principal":{"AWS":"arn:aws:iam::123456789012:role/other"}, "Action":"s3:GetObject", "Resource":"arn:aws:s3:::owned-data/*"});
        let normalized_public = json!({"Sid":"PublicRead", "Effect":"Allow", "Principal":{"AWS":"*"}, "Action":["s3:GetObject"], "Resource":["arn:aws:s3:::owned-data/*"]});
        let conditional = json!({"Effect":"Allow", "Principal":"*", "Action":"s3:GetObject", "Resource":"arn:aws:s3:::owned-data/*", "Condition":{"ArnEquals":{"aws:PrincipalArn":"arn:aws:iam::123456789012:role/other"}}});
        let policy = Arc::new(Mutex::new(
            json!({"Version":"2012-10-17", "Statement":[public, normalized_public, foreign.clone(), conditional.clone()]}),
        ));
        let mut api = MockS3Api::new();
        api.expect_get_bucket_tagging()
            .times(4)
            .returning(|bucket| {
                assert_eq!(bucket, "owned-data");
                Ok(alien_core::standard_resource_tags("owned", "data"))
            });
        api.expect_get_bucket_lifecycle_configuration()
            .returning(|_| Ok(alien_aws_clients::s3::LifecycleConfiguration { rules: vec![] }));
        let read = policy.clone();
        api.expect_get_bucket_policy().times(4).returning(move |_| {
            Ok(GetBucketPolicyOutput {
                policy: read.lock().unwrap().to_string(),
            })
        });
        let write = policy.clone();
        api.expect_put_bucket_policy()
            .times(1)
            .returning(move |_, body| {
                *write.lock().unwrap() = serde_json::from_str(body).unwrap();
                Err(transient())
            });
        let api = Arc::new(api);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_s3_client()
            .returning(move |_| Ok(api.clone()));
        let state = installed(
            Platform::Aws,
            Box::new(AwsStorageController {
                bucket_name: Some("owned-data".into()),
                ..Default::default()
            }),
        );
        // A privacy update may have been interrupted before the provider mutation.
        let mut state = state;
        let entry = state.resources.get_mut("data").unwrap();
        entry.previous_config = Some(entry.config.clone());
        entry.config = Resource::new(Storage::new("data".into()).build());
        let original = serde_json::to_value(&state).unwrap();
        let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
        assert!(protect_retained_storage(
            &state,
            &["data".into()],
            &client,
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live]
        )
        .await
        .is_err());
        for _ in 0..2 {
            let reloaded = serde_json::from_value(original.clone()).unwrap();
            protect_retained_storage(
                &reloaded,
                &["data".into()],
                &client,
                &provider,
                &ExternalBindings::default(),
                &[ResourceLifecycle::Live],
            )
            .await
            .unwrap();
            assert_eq!(serde_json::to_value(reloaded).unwrap(), original);
        }
        // A failed replacement can leave the old storage controller behind a
        // new declaration. Its previous config still owns retained data.
        let entry = state.resources.get_mut("data").unwrap();
        entry.config = Resource::new(alien_core::Vault::new("data".into()).build());
        let old_identity = entry.internal_state.clone();
        protect_retained_storage(
            &state,
            &["data".into()],
            &client,
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live],
        )
        .await
        .unwrap();
        assert_eq!(state.resources["data"].internal_state, old_identity);
        assert_eq!(
            policy.lock().unwrap()["Statement"],
            json!([foreign, conditional])
        );
    }

    #[tokio::test]
    async fn s3_lifecycle_removal_requires_readback_and_retries_without_redeleting() {
        let mut api = MockS3Api::new();
        let configuration = alien_aws_clients::s3::LifecycleConfiguration {
            rules: vec![alien_aws_clients::s3::LifecycleRule::builder()
                .status(alien_aws_clients::s3::LifecycleRuleStatus::Enabled)
                .filter(alien_aws_clients::s3::LifecycleRuleFilter::builder().build())
                .expiration(
                    alien_aws_clients::s3::LifecycleExpiration::builder()
                        .days(1)
                        .build(),
                )
                .build()],
        };
        let mut sequence = mockall::Sequence::new();
        api.expect_get_bucket_lifecycle_configuration()
            .times(2)
            .in_sequence(&mut sequence)
            .returning(move |_| Ok(configuration.clone()));
        api.expect_get_bucket_lifecycle_configuration()
            .times(2)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Err(AlienError::new(ClientError::RemoteResourceNotFound {
                    resource_type: "lifecycle".into(),
                    resource_name: "owned-data".into(),
                }))
            });
        api.expect_delete_bucket_lifecycle()
            .times(1)
            .returning(|name| {
                assert_eq!(name, "owned-data");
                Ok(())
            });
        assert!(stop_s3_lifecycle(&api, "owned-data", "data").await.is_err());
        stop_s3_lifecycle(&api, "owned-data", "data").await.unwrap();
        stop_s3_lifecycle(&api, "owned-data", "data").await.unwrap();
    }

    #[tokio::test]
    async fn gcs_lifecycle_removal_refuses_denied_patch_and_unconfirmed_readback() {
        for denied in [true, false] {
            let mut api = MockGcsApi::new();
            let observed = Bucket {
                lifecycle: Some(Lifecycle {
                    rule: Some(vec![alien_gcp_clients::gcs::LifecycleRule::default()]),
                }),
                ..Default::default()
            };
            api.expect_update_bucket()
                .times(1)
                .returning(move |_, patch| {
                    assert_eq!(
                        serde_json::to_value(&patch.lifecycle).unwrap(),
                        json!({"rule":[]})
                    );
                    assert!(
                        patch.labels.is_none(),
                        "lifecycle patch leaves labels alone"
                    );
                    if denied {
                        Err(AlienError::new(ClientError::RemoteAccessDenied {
                            resource_type: "bucket".into(),
                            resource_name: "owned-data".into(),
                        }))
                    } else {
                        Ok(patch)
                    }
                });
            let readback = observed.clone();
            api.expect_get_bucket()
                .times(usize::from(!denied))
                .returning(move |_| Ok(readback.clone()));
            assert!(stop_gcs_lifecycle(&api, "owned-data", "data", &observed)
                .await
                .is_err());
        }
    }

    #[tokio::test]
    async fn private_gcs_lifecycle_removal_recovers_lost_response_without_changing_identity() {
        let active = Arc::new(Mutex::new(true));
        let mut api = MockGcsApi::new();
        let read = active.clone();
        api.expect_get_bucket().times(4).returning(move |name| {
            assert_eq!(name, "owned-data");
            Ok(Bucket {
                lifecycle: Some(Lifecycle {
                    rule: Some(if *read.lock().unwrap() {
                        vec![alien_gcp_clients::gcs::LifecycleRule::default()]
                    } else {
                        vec![]
                    }),
                }),
                labels: Some(
                    [("customer".to_owned(), "kept".to_owned())]
                        .into_iter()
                        .collect(),
                ),
                ..Default::default()
            })
        });
        let write = active.clone();
        api.expect_update_bucket()
            .times(1)
            .returning(move |name, patch| {
                assert_eq!(name, "owned-data");
                assert_eq!(
                    serde_json::to_value(&patch.lifecycle).unwrap(),
                    json!({"rule":[]})
                );
                assert!(patch.labels.is_none());
                *write.lock().unwrap() = false;
                Err(transient())
            });
        let api = Arc::new(api);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_gcs_client()
            .returning(move |_| Ok(api.clone()));
        let mut state = installed(
            Platform::Gcp,
            Box::new(GcpStorageController {
                bucket_name: Some("owned-data".into()),
                ..Default::default()
            }),
        );
        state.resources.get_mut("data").unwrap().config =
            Resource::new(Storage::new("data".into()).build());
        let original = serde_json::to_value(&state).unwrap();
        let client = ClientConfig::Gcp(Box::new(GcpClientConfig::mock()));
        assert!(protect_retained_storage(
            &state,
            &["data".into()],
            &client,
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live]
        )
        .await
        .is_err());
        for phase in ["runtime", "handoff-pending", "handoff-ready"] {
            let reloaded = serde_json::from_value(original.clone()).unwrap();
            let result = protect_retained_storage(
                &reloaded,
                &["data".into()],
                &client,
                &provider,
                &ExternalBindings::default(),
                if phase == "runtime" {
                    &[ResourceLifecycle::Live]
                } else {
                    &[ResourceLifecycle::Live, ResourceLifecycle::Frozen]
                },
            )
            .await;
            assert_eq!(
                result.is_ok(),
                true,
                "confirmed lifecycle removal permits retention after reload"
            );
            assert_eq!(serde_json::to_value(reloaded).unwrap(), original);
        }
        assert!(!*active.lock().unwrap());
    }

    #[tokio::test]
    async fn s3_retention_accepts_verified_absence_but_refuses_access_denial() {
        for absent in [true, false] {
            let mut api = MockS3Api::new();
            api.expect_get_bucket_tagging()
                .times(1)
                .returning(move |_| {
                    Err(AlienError::new(if absent {
                        ClientError::RemoteResourceNotFound {
                            resource_type: "bucket".into(),
                            resource_name: "owned-data".into(),
                        }
                    } else {
                        ClientError::RemoteAccessDenied {
                            resource_type: "bucket".into(),
                            resource_name: "owned-data".into(),
                        }
                    }))
                });
            let api = Arc::new(api);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_s3_client()
                .returning(move |_| Ok(api.clone()));
            let state = installed(
                Platform::Aws,
                Box::new(AwsStorageController {
                    bucket_name: Some("owned-data".into()),
                    ..Default::default()
                }),
            );
            let original = serde_json::to_value(&state).unwrap();
            let result = protect_retained_storage(
                &state,
                &["data".into()],
                &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
                &provider,
                &ExternalBindings::default(),
                &[ResourceLifecycle::Live],
            )
            .await;
            assert_eq!(result.is_ok(), absent);
            assert_eq!(serde_json::to_value(&state).unwrap(), original);
        }
    }

    #[tokio::test]
    async fn s3_retention_refuses_foreign_bucket_before_touching_policy() {
        let mut api = MockS3Api::new();
        api.expect_get_bucket_tagging()
            .times(1)
            .returning(|_| Ok(alien_core::standard_resource_tags("other", "data")));
        let api = Arc::new(api);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_s3_client()
            .returning(move |_| Ok(api.clone()));
        let state = installed(
            Platform::Aws,
            Box::new(AwsStorageController {
                bucket_name: Some("owned-data".into()),
                ..Default::default()
            }),
        );
        let error = protect_retained_storage(
            &state,
            &["data".into()],
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live],
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
    }

    #[tokio::test]
    async fn gcs_retention_keeps_ubla_conditions_members_and_retries_with_fresh_etag() {
        let private = json!({"role":"roles/storage.objectAdmin", "members":["serviceAccount:foreign@example.test"], "condition":{"title":"expiry","expression":"request.time < timestamp('2030-01-01T00:00:00Z')"}});
        let policy = Arc::new(Mutex::new(serde_json::from_value::<IamPolicy>(json!({"version":3,"etag":"first", "bindings":[private.clone(), {"role":"roles/storage.objectViewer", "members":["allUsers","user:owner@example.test"]}]})).unwrap()));
        let pap = Arc::new(Mutex::new(false));
        let mut api = MockGcsApi::new();
        let read = pap.clone();
        api.expect_get_bucket().times(3).returning(move |name| {
            assert_eq!(name, "owned-data");
            Ok(Bucket {
                iam_configuration: Some(IamConfiguration {
                    uniform_bucket_level_access: None,
                    public_access_prevention: Some(
                        if *read.lock().unwrap() {
                            "enforced"
                        } else {
                            "inherited"
                        }
                        .into(),
                    ),
                }),
                ..Default::default()
            })
        });
        let write = pap.clone();
        api.expect_update_bucket()
            .times(1)
            .returning(move |_, patch| {
                let iam = patch.iam_configuration.as_ref().unwrap();
                assert!(iam.uniform_bucket_level_access.is_none());
                assert_eq!(iam.public_access_prevention.as_deref(), Some("enforced"));
                *write.lock().unwrap() = true;
                Ok(patch)
            });
        let read = policy.clone();
        api.expect_get_bucket_iam_policy()
            .times(3)
            .returning(move |_| Ok(read.lock().unwrap().clone()));
        let write = policy.clone();
        let mut attempts = 0;
        api.expect_set_bucket_iam_policy()
            .times(2)
            .returning(move |_, mut proposed| {
                attempts += 1;
                let mut actual = write.lock().unwrap();
                assert_eq!(proposed.etag, actual.etag);
                if attempts == 1 {
                    // Concurrent administrator update rejected this stale write.
                    actual.etag = Some("second".into());
                    actual.bindings[0]
                        .members
                        .push("user:new@example.test".into());
                    return Err(transient());
                }
                proposed.etag = Some("third".into());
                *actual = proposed.clone();
                Ok(proposed)
            });
        let api = Arc::new(api);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_gcs_client()
            .returning(move |_| Ok(api.clone()));
        let state = installed(
            Platform::Gcp,
            Box::new(GcpStorageController {
                bucket_name: Some("owned-data".into()),
                ..Default::default()
            }),
        );
        let original = serde_json::to_value(&state).unwrap();
        let client = ClientConfig::Gcp(Box::new(GcpClientConfig::mock()));
        assert!(protect_retained_storage(
            &state,
            &["data".into()],
            &client,
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live]
        )
        .await
        .is_err());
        for _ in 0..2 {
            let reloaded = serde_json::from_value(original.clone()).unwrap();
            protect_retained_storage(
                &reloaded,
                &["data".into()],
                &client,
                &provider,
                &ExternalBindings::default(),
                &[ResourceLifecycle::Live],
            )
            .await
            .unwrap();
            assert_eq!(serde_json::to_value(reloaded).unwrap(), original);
        }
        let actual = serde_json::to_value(policy.lock().unwrap().clone()).unwrap();
        assert_eq!(actual["bindings"][0]["condition"], private["condition"]);
        assert_eq!(
            actual["bindings"][0]["members"],
            json!([
                "serviceAccount:foreign@example.test",
                "user:new@example.test"
            ])
        );
        assert_eq!(
            actual["bindings"][1]["members"],
            json!(["user:owner@example.test"])
        );
    }

    #[tokio::test]
    async fn private_azure_container_cannot_finish_retention_with_enabled_expiry() {
        let mut state = installed(
            Platform::Azure,
            Box::new(AzureStorageController {
                container_name: Some("owned-data".into()),
                storage_account_name: Some("recorded-account".into()),
                ..Default::default()
            }),
        );
        state.resources.get_mut("data").unwrap().config =
            Resource::new(Storage::new("data".into()).build());
        let mut group = StackResourceState::new_pending(
            "infra/azure-resource-group".into(),
            Resource::new(AzureResourceGroup::new("default-resource-group".into()).build()),
            None,
            vec![],
        );
        group.outputs = Some(ResourceOutputs::new(AzureResourceGroupOutputs {
            name: "recorded-group".into(),
            location: "eastus".into(),
            resource_id: "/subscriptions/test/resourceGroups/recorded-group".into(),
        }));
        state
            .resources
            .insert("default-resource-group".into(), group);
        let original = serde_json::to_value(&state).unwrap();
        let mut api = MockBlobContainerApi::new();
        api.expect_get_blob_container()
            .times(1)
            .returning(|_, _, _| {
                Ok(BlobContainer {
                    properties: Some(ContainerProperties {
                        public_access: Some(ContainerPropertiesPublicAccess::None),
                        ..Default::default()
                    }),
                    ..Default::default()
                })
            });
        api.expect_get_management_policy()
            .times(1)
            .returning(|group, account| {
                assert_eq!((group, account), ("recorded-group", "recorded-account"));
                Ok(
                    serde_json::from_value(json!({"properties":{"policy":{"rules":[{
                        "name":"expiry", "enabled":true, "type":"Lifecycle",
                        "definition":{"filters":{"blobTypes":["blockBlob"],"prefixMatch":["owned-data/"]},
                            "actions":{"version":{"delete":{"daysAfterCreationGreaterThan":1}}}}
                    }]}}}))
                    .unwrap(),
                )
            });
        let api = Arc::new(api);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(api.clone()));
        let reloaded = serde_json::from_value(original.clone()).unwrap();
        let error = protect_retained_storage(
            &reloaded,
            &["data".into()],
            &ClientConfig::Azure(Box::new(AzureClientConfig::mock())),
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live],
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "SETUP_RERUN_REQUIRED");
        assert_eq!(serde_json::to_value(&reloaded).unwrap(), original);
    }

    #[test]
    fn azure_retention_requires_disabled_applicable_deletes() {
        let now = chrono::Utc::now();
        let rule = json!({"name":"expiry", "enabled":true, "type":"Lifecycle",
            "definition":{"filters":{"blobTypes":["blockBlob"],"prefixMatch":["owned-data/"]},
            "actions":{"baseBlob":{"delete":{"daysAfterModificationGreaterThan":1}}}}});
        let mut policy = json!({"properties":{"lastModifiedTime": (now - chrono::Duration::hours(25)).to_rfc3339(),
            "policy":{"rules":[rule]}}});
        assert!(verify_azure_retention_policy(&policy, "owned-data", "data").is_err());
        assert!(verify_azure_retention_policy(&policy, "sibling", "data").is_ok());
        policy["properties"]["policy"]["rules"][0]["enabled"] = json!(false);
        assert!(verify_azure_retention_policy(&policy, "owned-data", "data").is_ok());
        for changed in [
            Value::Null,
            json!("invalid"),
            json!((now - chrono::Duration::hours(23)).to_rfc3339()),
        ] {
            policy["properties"]["lastModifiedTime"] = changed;
            assert!(verify_azure_retention_policy(&policy, "owned-data", "data").is_ok());
        }
        policy["properties"]["policy"]["rules"][0]["definition"]["actions"] =
            json!({"baseBlob":{"tierToCool":{"daysAfterModificationGreaterThan":1}}});
        assert!(verify_azure_retention_policy(&policy, "owned-data", "data").is_ok());
    }

    #[tokio::test]
    async fn azure_retention_only_changes_installed_container_and_retries_lost_response() {
        let mut state = installed(
            Platform::Azure,
            Box::new(AzureStorageController {
                container_name: Some("owned-data".into()),
                storage_account_name: Some("recorded-account".into()),
                ..Default::default()
            }),
        );
        let mut group = StackResourceState::new_pending(
            "infra/azure-resource-group".into(),
            Resource::new(AzureResourceGroup::new("default-resource-group".into()).build()),
            None,
            vec![],
        );
        group.outputs = Some(ResourceOutputs::new(AzureResourceGroupOutputs {
            name: "recorded-group".into(),
            location: "eastus".into(),
            resource_id: "/subscriptions/test/resourceGroups/recorded-group".into(),
        }));
        state
            .resources
            .insert("default-resource-group".into(), group);
        let private = Arc::new(Mutex::new(false));
        let mut api = MockBlobContainerApi::new();
        api.expect_get_management_policy().returning(|_, _| {
            Ok(serde_json::from_value(json!({"properties":{"policy":{"rules":[]}}})).unwrap())
        });
        let read = private.clone();
        api.expect_get_blob_container()
            .times(3)
            .returning(move |group, account, container| {
                assert_eq!(
                    (group, account, container),
                    ("recorded-group", "recorded-account", "owned-data")
                );
                Ok(BlobContainer {
                    properties: Some(ContainerProperties {
                        public_access: Some(if *read.lock().unwrap() {
                            ContainerPropertiesPublicAccess::None
                        } else {
                            ContainerPropertiesPublicAccess::Blob
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                })
            });
        let write = private.clone();
        api.expect_update_blob_container().times(1).returning(
            move |group, account, container, patch| {
                assert_eq!(
                    (group, account, container),
                    ("recorded-group", "recorded-account", "owned-data")
                );
                assert_eq!(
                    patch.properties.as_ref().unwrap().public_access,
                    Some(ContainerPropertiesPublicAccess::None)
                );
                *write.lock().unwrap() = true;
                Err(transient())
            },
        );
        let api = Arc::new(api);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(api.clone()));
        let client = ClientConfig::Azure(Box::new(AzureClientConfig::mock()));
        assert!(protect_retained_storage(
            &state,
            &["data".into()],
            &client,
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live]
        )
        .await
        .is_err());
        let original = serde_json::to_value(&state).unwrap();
        for _ in 0..2 {
            let reloaded = serde_json::from_value(original.clone()).unwrap();
            protect_retained_storage(
                &reloaded,
                &["data".into()],
                &client,
                &provider,
                &ExternalBindings::default(),
                &[ResourceLifecycle::Live],
            )
            .await
            .unwrap();
            assert_eq!(serde_json::to_value(reloaded).unwrap(), original);
        }
        assert!(*private.lock().unwrap());
    }

    #[tokio::test]
    async fn adopted_external_storage_is_not_mutated_but_a_binding_cannot_hide_owned_storage() {
        let mut state = installed(Platform::Aws, Box::new(AwsStorageController::default()));
        let original_controller = state.resources["data"].internal_state.clone();
        state.resources.get_mut("data").unwrap().internal_state = None;
        let bindings = ExternalBindings(std::collections::HashMap::from([(
            "data".into(),
            alien_core::ExternalBinding::Storage(alien_core::StorageBinding::s3("foreign-bucket")),
        )]));
        let provider = MockPlatformServiceProvider::new();
        let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
        protect_retained_storage(
            &state,
            &["data".into()],
            &client,
            &provider,
            &bindings,
            &[ResourceLifecycle::Live],
        )
        .await
        .unwrap();
        state.resources.get_mut("data").unwrap().internal_state = original_controller;
        assert!(protect_retained_storage(
            &state,
            &["data".into()],
            &client,
            &provider,
            &bindings,
            &[ResourceLifecycle::Live]
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn frozen_gcs_lifecycle_remains_under_setup_authority() {
        let mut state = installed(
            Platform::Gcp,
            Box::new(GcpStorageController {
                bucket_name: Some("setup-data".into()),
                ..Default::default()
            }),
        );
        state.resources.get_mut("data").unwrap().lifecycle = Some(ResourceLifecycle::Frozen);
        let original = serde_json::to_value(&state).unwrap();
        protect_retained_storage(
            &state,
            &["data".into()],
            &ClientConfig::Gcp(Box::new(GcpClientConfig::mock())),
            &MockPlatformServiceProvider::new(),
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live],
        )
        .await
        .unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), original);
    }

    #[tokio::test]
    async fn frozen_and_explicit_erase_do_not_use_runtime_public_access_mutation() {
        let mut state = installed(Platform::Aws, Box::new(AwsStorageController::default()));
        state.resources.get_mut("data").unwrap().lifecycle = Some(ResourceLifecycle::Frozen);
        let provider = MockPlatformServiceProvider::new();
        let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
        protect_retained_storage(
            &state,
            &["data".into()],
            &client,
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live],
        )
        .await
        .unwrap();
        state.resources.get_mut("data").unwrap().lifecycle = Some(ResourceLifecycle::Live);
        protect_retained_storage(
            &state,
            &[],
            &client,
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live],
        )
        .await
        .unwrap();
        state.resources.get_mut("data").unwrap().lifecycle = None;
        assert!(protect_retained_storage(
            &state,
            &["data".into()],
            &client,
            &provider,
            &ExternalBindings::default(),
            &[ResourceLifecycle::Live]
        )
        .await
        .is_err());
    }
}
