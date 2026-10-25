//! Registry repositories and identities when a deployment is deleted.

use std::collections::{HashMap, HashSet};

use alien_aws_clients::aws::ecr::{DeleteRepositoryRequest, DescribeRepositoriesRequest};
use alien_client_core::ErrorData as ClientError;
use alien_core::{
    standard_resource_tags, ClientConfig, Platform, ResourceStatus, StackResourceState, StackState,
    ALIEN_REGISTRY_OWNER_TAG_KEY, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY,
};
use alien_error::{AlienError, Context, ContextError};
use serde_json::Value;

use super::AwsArtifactRegistryController;
use crate::{ErrorData, PlatformServiceProvider, Result, StackResourceStateExt};

/// Delete the repositories the runtime created in each AWS registry of an explicitly erased
/// deployment.
///
/// The AWS registry controller owns only the pull and push roles, so its own delete leaves the
/// repositories and their images in the account. Repositories are found by the `<prefix>-`
/// naming the registry's bindings use, and every retry lists them again. A registry whose roles
/// an earlier retention already deleted still names its repositories, so a `Deleted` record
/// counts. A registered prefix is only a discovery hint: hyphenated prefixes can overlap.
/// Every deletion also requires provider-recorded deployment/resource ownership. Untagged
/// repositories require an explicit ownership review; never claim them from their name.
pub async fn erase_registry_repositories(
    state: &StackState,
    client_config: &ClientConfig,
    provider: &dyn PlatformServiceProvider,
) -> Result<()> {
    for (id, resource) in &state.resources {
        if !owned_registry(
            state,
            resource,
            Platform::Aws,
            "AwsArtifactRegistryController",
        ) {
            continue;
        }
        let controller =
            resource.get_internal_controller_typed::<AwsArtifactRegistryController>()?;
        // A registry that never recorded its prefix never handed out a binding to create
        // repositories with.
        let Some(prefix) = controller.repository_prefix.as_deref() else {
            continue;
        };
        if prefix != format!("{}-{id}", state.resource_prefix) {
            return Err(invalid(
                id,
                "Recorded registry prefix does not belong to this deployment resource",
            ));
        }
        let prefix = format!("{prefix}-");
        let config = client_config
            .aws_config()
            .ok_or_else(|| invalid(id, "AWS setup credentials are required"))?;
        if controller.account_id.as_deref() != Some(config.account_id.as_str())
            || controller.region.as_deref() != Some(config.region.as_str())
        {
            return Err(invalid(
                id,
                "Recorded registry account or region does not match the setup target",
            ));
        }
        let client = provider.get_aws_ecr_client(config).await?;
        let mut next_token = None;
        let mut seen_tokens = HashSet::new();
        let mut repositories = Vec::new();
        // Finish discovery before deleting: changing the collection while paginating can skip
        // repositories, and a denied later page must not leave a partially erased registry.
        loop {
            let page = client
                .describe_repositories(DescribeRepositoriesRequest {
                    registry_id: controller.account_id.clone(),
                    repository_names: None,
                    next_token,
                    max_results: Some(100),
                })
                .await
                .context(cloud_error(id))?;
            for repository in page.repositories {
                if repository.repository_name.starts_with(&prefix) {
                    if repository.registry_id != config.account_id {
                        return Err(invalid(
                            id,
                            "Repository discovery returned a different registry account",
                        ));
                    }
                    repositories.push(repository);
                }
            }
            next_token = page.next_token;
            match &next_token {
                None => break,
                Some(token) if token.is_empty() || !seen_tokens.insert(token.clone()) => {
                    return Err(invalid(
                        id,
                        "Repository discovery returned an invalid or repeated pagination token",
                    ));
                }
                Some(_) => {}
            }
        }
        let expected_tags = standard_resource_tags(&state.resource_prefix, id);
        let mut owned = Vec::new();
        // Prove every candidate before the first deletion, including candidates on later pages.
        // A historical untagged repository is ambiguous, even when its name looks right.
        for repository in repositories {
            let expected_arn = format!(
                "arn:aws:ecr:{}:{}:repository/{}",
                config.region, config.account_id, repository.repository_name
            );
            if repository.repository_arn != expected_arn {
                return Err(invalid(
                    id,
                    "Repository discovery returned a different account, region or name",
                ));
            }
            let tags = match client
                .list_tags_for_resource(&repository.repository_arn)
                .await
            {
                Ok(response) => response
                    .tags
                    .into_iter()
                    .map(|tag| (tag.key, tag.value))
                    .collect::<HashMap<_, _>>(),
                Err(error) if missing(&error) => continue,
                Err(error) => return Err(error.context(cloud_error(id))),
            };
            let marker = tags.get(ALIEN_REGISTRY_OWNER_TAG_KEY);
            let marker_matches = marker.is_some()
                && marker.map(String::as_str) == controller.push_role_arn.as_deref();
            let foreign_standard_tags = tags
                .get(ALIEN_STACK_TAG_KEY)
                .is_some_and(|value| Some(value) != expected_tags.get(ALIEN_STACK_TAG_KEY))
                || tags
                    .get(ALIEN_RESOURCE_TAG_KEY)
                    .is_some_and(|value| Some(value) != expected_tags.get(ALIEN_RESOURCE_TAG_KEY));
            if (marker_matches && !foreign_standard_tags)
                || (marker.is_none() && is_tagged_as_ours(&tags, &expected_tags))
            {
                owned.push(repository);
            } else if marker.is_some() && !marker_matches {
                // A repository created by another registry role may share this name prefix.
                continue;
            } else if tags.contains_key(ALIEN_STACK_TAG_KEY)
                && tags.contains_key(ALIEN_RESOURCE_TAG_KEY)
                && (tags.get(ALIEN_STACK_TAG_KEY) != expected_tags.get(ALIEN_STACK_TAG_KEY)
                    || tags.get(ALIEN_RESOURCE_TAG_KEY)
                        != expected_tags.get(ALIEN_RESOURCE_TAG_KEY))
            {
                // A different deployment/resource may legitimately share this textual prefix.
                continue;
            } else {
                return Err(invalid(
                    id,
                    &format!(
                        "Repository '{}' has no verified deployment ownership. No repositories in this registry were erased. Review its ownership with the account administrator before explicit cleanup; a matching name is not authorization.",
                        repository.repository_name
                    ),
                ));
            }
        }
        for repository in owned {
            match client
                .delete_repository(DeleteRepositoryRequest {
                    repository_name: repository.repository_name,
                    registry_id: Some(repository.registry_id),
                    force: Some(true),
                })
                .await
            {
                Ok(_) => {}
                Err(error) if missing(&error) => {}
                Err(error) => return Err(error.context(cloud_error(id))),
            }
        }
    }
    Ok(())
}

/// A registry this deployment created on `platform`, including one whose declaration was
/// replaced by an interrupted update.
fn owned_registry(
    state: &StackState,
    resource: &StackResourceState,
    platform: Platform,
    controller_type: &str,
) -> bool {
    resource.controller_platform.unwrap_or(state.platform) == platform
        && std::iter::once(&resource.config)
            .chain(resource.previous_config.iter())
            .any(|config| config.resource_type().as_ref() == "artifact-registry")
        && resource
            .internal_state
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
            == Some(controller_type)
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
        message: "Failed to clean up registry access or repositories; restore the setup grants and retry deletion".to_string(),
    }
}

use alien_core::ALIEN_MANAGED_BY_TAG_KEY;

/// The `managed-by` value older versions wrote on the objects they created.
const LEGACY_MANAGED_BY_TAG_VALUE: &str = "deployment";

/// Whether `tags` carry every tag in `ours`, the tags this deployment writes for the resource
/// (`standard_resource_tags`). `managed-by=deployment`, written by versions before the
/// `runtime` value, is accepted too.
pub(crate) fn is_tagged_as_ours(
    tags: &HashMap<String, String>,
    ours: &HashMap<String, String>,
) -> bool {
    ours.iter().all(|(key, value)| {
        let found = tags.get(key).map(String::as_str);
        found == Some(value.as_str())
            || (key == ALIEN_MANAGED_BY_TAG_KEY && found == Some(LEGACY_MANAGED_BY_TAG_VALUE))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use alien_aws_clients::{
        aws::ecr::{
            DeleteRepositoryResponse, DescribeRepositoriesResponse, ListTagsForResourceResponse,
            MockEcrApi, Repository, Tag,
        },
        AwsClientConfigExt,
    };
    use alien_core::{ArtifactRegistry, AwsClientConfig, Resource};

    use super::*;
    use crate::{MockPlatformServiceProvider, ResourceController};

    fn installed(
        platform: Platform,
        entries: Vec<(&str, Box<dyn ResourceController>)>,
    ) -> StackState {
        let mut state = StackState::new(platform);
        state.resource_prefix = "owned".into();
        for (id, controller) in entries {
            let mut resource = StackResourceState::new_pending(
                "artifact-registry".into(),
                Resource::new(ArtifactRegistry::new(id.into()).build()),
                None,
                vec![],
            );
            resource.status = ResourceStatus::Running;
            resource.set_internal_controller(Some(controller)).unwrap();
            state.resources.insert(id.into(), resource);
        }
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap()
    }

    fn repository(name: &str) -> Repository {
        Repository {
            repository_arn: format!("arn:aws:ecr:us-east-1:123456789012:repository/{name}"),
            registry_id: "123456789012".into(),
            repository_name: name.into(),
            repository_uri: format!("123456789012.dkr.ecr.us-east-1.amazonaws.com/{name}"),
            created_at: 0.0,
            image_tag_mutability: None,
            image_scanning_configuration: None,
            encryption_configuration: None,
        }
    }

    #[tokio::test]
    async fn erase_deletes_every_page_of_the_registry_repositories_and_nothing_else() {
        let deleted = Arc::new(Mutex::new(Vec::new()));
        let mut ecr = MockEcrApi::new();
        ecr.expect_describe_repositories()
            .times(4)
            .returning(|request| {
                assert_eq!(request.registry_id.as_deref(), Some("123456789012"));
                Ok(match request.next_token.as_deref() {
                    None => DescribeRepositoriesResponse {
                        repositories: vec![
                            repository("owned-images-api"),
                            // Another registry whose id extends this one's.
                            repository("owned-images2-api"),
                            repository("unrelated"),
                        ],
                        next_token: Some("page-2".into()),
                    },
                    Some(_) => DescribeRepositoriesResponse {
                        repositories: vec![repository("owned-images-worker")],
                        next_token: None,
                    },
                })
            });
        ecr.expect_list_tags_for_resource().returning(|_| {
            Ok(ListTagsForResourceResponse {
                tags: standard_resource_tags("owned", "images")
                    .into_iter()
                    .map(|(key, value)| Tag { key, value })
                    .collect(),
            })
        });
        let recorded = deleted.clone();
        ecr.expect_delete_repository().returning(move |request| {
            assert_eq!(request.force, Some(true), "images go with the repository");
            recorded
                .lock()
                .unwrap()
                .push(request.repository_name.clone());
            if request.repository_name == "owned-images-worker" {
                // A retry after a lost response finds it already gone.
                return Err(AlienError::new(ClientError::RemoteResourceNotFound {
                    resource_type: "Repository".into(),
                    resource_name: request.repository_name,
                }));
            }
            Ok(DeleteRepositoryResponse {
                repository: repository(&request.repository_name),
            })
        });
        let ecr = Arc::new(ecr);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ecr_client()
            .returning(move |_| Ok(ecr.clone()));
        let registry = |prefix: Option<&str>| -> Box<dyn ResourceController> {
            Box::new(AwsArtifactRegistryController {
                account_id: Some("123456789012".into()),
                region: Some("us-east-1".into()),
                repository_prefix: prefix.map(Into::into),
                ..Default::default()
            })
        };
        let mut state = installed(
            Platform::Aws,
            vec![
                ("images", registry(Some("owned-images"))),
                ("never-created", registry(None)),
            ],
        );
        // An earlier retention deleted the registry's roles; its record still names the
        // repositories.
        state.resources.get_mut("images").unwrap().status = ResourceStatus::Deleted;
        let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));

        for _ in 0..2 {
            erase_registry_repositories(&state, &client, &provider)
                .await
                .unwrap();
        }

        let deleted = deleted.lock().unwrap();
        assert_eq!(
            deleted
                .iter()
                .filter(|name| *name == "owned-images-api")
                .count(),
            2
        );
        assert!(deleted.contains(&"owned-images-worker".to_string()));
        assert!(!deleted
            .iter()
            .any(|name| name == "owned-images2-api" || name == "unrelated"));
    }

    #[tokio::test]
    async fn erase_preserves_a_different_deployment_under_an_overlapping_prefix() {
        let mut ecr = MockEcrApi::new();
        ecr.expect_describe_repositories().returning(|_| {
            Ok(DescribeRepositoriesResponse {
                repositories: vec![
                    repository("owned-images-api"),
                    repository("owned-images-api-worker"),
                ],
                next_token: None,
            })
        });
        ecr.expect_list_tags_for_resource().returning(|arn| {
            let (deployment, resource) = if arn.ends_with("/owned-images-api-worker") {
                ("owned-images", "api")
            } else {
                ("owned", "images")
            };
            Ok(ListTagsForResourceResponse {
                tags: standard_resource_tags(deployment, resource)
                    .into_iter()
                    .map(|(key, value)| Tag { key, value })
                    .collect(),
            })
        });
        ecr.expect_delete_repository()
            .times(1)
            .returning(|request| {
                assert_eq!(
                    request.repository_name, "owned-images-api",
                    "a foreign prefix collision must never be erased"
                );
                Ok(DeleteRepositoryResponse {
                    repository: repository(&request.repository_name),
                })
            });
        let ecr = Arc::new(ecr);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ecr_client()
            .returning(move |_| Ok(ecr.clone()));
        let state = installed(
            Platform::Aws,
            vec![(
                "images",
                Box::new(AwsArtifactRegistryController {
                    repository_prefix: Some("owned-images".into()),
                    account_id: Some("123456789012".into()),
                    region: Some("us-east-1".into()),
                    ..Default::default()
                }),
            )],
        );
        erase_registry_repositories(
            &state,
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &provider,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn erase_uses_creation_owner_markers_to_preserve_overlapping_foreign_repositories() {
        let mut ecr = MockEcrApi::new();
        ecr.expect_describe_repositories().returning(|_| {
            Ok(DescribeRepositoriesResponse {
                repositories: vec![
                    repository("owned-images-api"),
                    repository("owned-images-api-worker"),
                ],
                next_token: None,
            })
        });
        ecr.expect_list_tags_for_resource().returning(|arn| {
            let owner = if arn.ends_with("/owned-images-api-worker") {
                "arn:aws:iam::123456789012:role/foreign-push"
            } else {
                "arn:aws:iam::123456789012:role/owned-push"
            };
            Ok(ListTagsForResourceResponse {
                tags: vec![Tag {
                    key: ALIEN_REGISTRY_OWNER_TAG_KEY.to_string(),
                    value: owner.to_string(),
                }],
            })
        });
        ecr.expect_delete_repository()
            .times(1)
            .returning(|request| {
                assert_eq!(
                    request.repository_name, "owned-images-api",
                    "a foreign prefix collision must never be erased"
                );
                Ok(DeleteRepositoryResponse {
                    repository: repository(&request.repository_name),
                })
            });
        let ecr = Arc::new(ecr);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ecr_client()
            .returning(move |_| Ok(ecr.clone()));
        let mut state = installed(
            Platform::Aws,
            vec![(
                "images",
                Box::new(AwsArtifactRegistryController {
                    repository_prefix: Some("owned-images".into()),
                    push_role_arn: Some("arn:aws:iam::123456789012:role/owned-push".into()),
                    account_id: Some("123456789012".into()),
                    region: Some("us-east-1".into()),
                    ..Default::default()
                }),
            )],
        );
        state.resources.get_mut("images").unwrap().status = ResourceStatus::Deleted;
        let state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        erase_registry_repositories(
            &state,
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &provider,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn erase_refuses_unproven_or_unreadable_ownership_before_any_deletion() {
        for denied in [false, true] {
            let mut ecr = MockEcrApi::new();
            ecr.expect_describe_repositories().returning(|_| {
                Ok(DescribeRepositoriesResponse {
                    repositories: vec![
                        repository("owned-images-api"),
                        repository("owned-images-unknown"),
                    ],
                    next_token: None,
                })
            });
            ecr.expect_list_tags_for_resource().returning(move |arn| {
                if arn.ends_with("/owned-images-unknown") {
                    if denied {
                        return Err(AlienError::new(ClientError::RemoteAccessDenied {
                            resource_type: "Repository".into(),
                            resource_name: arn.into(),
                        }));
                    }
                    return Ok(ListTagsForResourceResponse { tags: vec![] });
                }
                Ok(ListTagsForResourceResponse {
                    tags: standard_resource_tags("owned", "images")
                        .into_iter()
                        .map(|(key, value)| Tag { key, value })
                        .collect(),
                })
            });
            ecr.expect_delete_repository().times(0);
            let ecr = Arc::new(ecr);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_ecr_client()
                .returning(move |_| Ok(ecr.clone()));
            let state = installed(
                Platform::Aws,
                vec![(
                    "images",
                    Box::new(AwsArtifactRegistryController {
                        repository_prefix: Some("owned-images".into()),
                        account_id: Some("123456789012".into()),
                        region: Some("us-east-1".into()),
                        ..Default::default()
                    }),
                )],
            );
            erase_registry_repositories(
                &state,
                &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
                &provider,
            )
            .await
            .expect_err("ambiguous or unreadable ownership must preserve every repository");
        }
    }

    #[tokio::test]
    async fn erase_refuses_a_foreign_recorded_registry_before_using_credentials() {
        let state = installed(
            Platform::Aws,
            vec![(
                "images",
                Box::new(AwsArtifactRegistryController {
                    repository_prefix: Some("another-deployment-images".into()),
                    ..Default::default()
                }),
            )],
        );
        let error = erase_registry_repositories(
            &state,
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &MockPlatformServiceProvider::new(),
        )
        .await
        .expect_err("foreign prefix must not be deleted");
        assert!(error.to_string().contains("prefix"));
    }

    #[tokio::test]
    async fn erase_discovers_every_page_before_deleting_and_refuses_broken_pagination() {
        for denied in [true, false] {
            let mut ecr = MockEcrApi::new();
            ecr.expect_describe_repositories()
                .returning(move |request| {
                    if request.next_token.is_some() && denied {
                        return Err(AlienError::new(ClientError::RemoteAccessDenied {
                            resource_type: "Repository".into(),
                            resource_name: "page-2".into(),
                        }));
                    }
                    Ok(DescribeRepositoriesResponse {
                        repositories: vec![repository("owned-images-api")],
                        next_token: Some("page-2".into()),
                    })
                });
            // Any deletion before discovery completes is a mock failure.
            let ecr = Arc::new(ecr);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_ecr_client()
                .returning(move |_| Ok(ecr.clone()));
            let state = installed(
                Platform::Aws,
                vec![(
                    "images",
                    Box::new(AwsArtifactRegistryController {
                        repository_prefix: Some("owned-images".into()),
                        account_id: Some("123456789012".into()),
                        region: Some("us-east-1".into()),
                        ..Default::default()
                    }),
                )],
            );
            erase_registry_repositories(
                &state,
                &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
                &provider,
            )
            .await
            .expect_err("incomplete discovery must never authorize deletion");
        }
    }
}
