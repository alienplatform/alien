//! A generation-specific gateway task family. Registration is not idempotent: after
//! an uncertain reply, discover the recorded family rather than register again.

use std::collections::{BTreeMap, BTreeSet};

use alien_aws_clients::ecs::{
    ContainerDefinition, DeleteTaskDefinitionsRequest, EcsApi, ListTagsForResourceRequest,
    ListTaskDefinitionsRequest, PortMapping, RegisterTaskDefinitionRequest, RuntimePlatform, Tag,
    TaskDefinitionRequest, TaskDefinitionResponse,
};
use alien_core::standard_resource_tags;
use alien_error::{AlienError, Context, ContextError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::aws_gateway_config::{GATEWAY_HEALTH_PORT, GATEWAY_HTTP_PORT, GATEWAY_IMAGE};
use crate::core::aws_tag_scoped::{
    is_remote_not_found, preserve_composite_cleanup_error, CREATE_ATTEMPT_TAG,
};
use crate::{ErrorData, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GatewayTask {
    account_id: String,
    region: String,
    request: RegisterTaskDefinitionRequest,
    registration_permitted: bool,
    registration_attempted: bool,
    revisions: BTreeSet<String>,
    delete_requested: BTreeSet<String>,
    retiring: bool,
}

impl GatewayTask {
    /// Bootstrap contains routing metadata only. This task receives no IAM role or secrets.
    pub fn new(
        account_id: String,
        region: String,
        stack_prefix: &str,
        resource_id: &str,
        bootstrap: &serde_json::Value,
    ) -> Result<Self> {
        if account_id.len() != 12
            || !account_id.bytes().all(|b| b.is_ascii_digit())
            || region.is_empty()
            || stack_prefix.is_empty()
            || resource_id.is_empty()
        {
            return Err(invalid(
                "Gateway task requires account, region and ownership",
            ));
        }
        let token = Uuid::new_v4().to_string();
        let family = format!("{stack_prefix}-{resource_id}-gateway-{token}");
        if family.len() > 255
            || !family
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(invalid("Gateway task family exceeds ECS naming limits"));
        }
        let mut tags = standard_resource_tags(stack_prefix, resource_id);
        tags.insert(CREATE_ATTEMPT_TAG.into(), token);
        let tags = tags
            .into_iter()
            .map(|(key, value)| Tag { key, value })
            .collect();
        let request = RegisterTaskDefinitionRequest::builder()
            .family(family)
            .container_definitions(vec![ContainerDefinition::builder()
                .name("gateway".into())
                .image(GATEWAY_IMAGE.into())
                .essential(true)
                .entry_point(vec!["/usr/local/bin/envoy".into()])
                .command(vec![
                    "--config-yaml".into(),
                    bootstrap.to_string(),
                    "--log-level".into(),
                    "warn".into(),
                ])
                .port_mappings(
                    [GATEWAY_HTTP_PORT, GATEWAY_HEALTH_PORT]
                        .into_iter()
                        .map(|port| {
                            PortMapping::builder()
                                .container_port(i32::from(port))
                                .protocol("tcp".into())
                                .build()
                        })
                        .collect(),
                )
                .readonly_root_filesystem(true)
                .stop_timeout(120)
                .build()])
            .network_mode("awsvpc".into())
            .requires_compatibilities(vec!["FARGATE".into()])
            .cpu("256".into())
            .memory("512".into())
            .runtime_platform(
                RuntimePlatform::builder()
                    .cpu_architecture("X86_64".into())
                    .operating_system_family("LINUX".into())
                    .build(),
            )
            .tags(tags)
            .build();
        Ok(Self {
            account_id,
            region,
            request,
            registration_permitted: false,
            registration_attempted: false,
            revisions: BTreeSet::new(),
            delete_requested: BTreeSet::new(),
            retiring: false,
        })
    }

    /// Parent checkpoints this owner before calling. Pending steps require a polling delay.
    /// An unresolved registration is deliberately retained, including during cleanup.
    pub async fn step_create(&mut self, ecs: &dyn EcsApi) -> Result<Option<String>> {
        if self.retiring {
            return Err(invalid("A retiring gateway task cannot be reused"));
        }
        if !self.registration_permitted {
            // Checkpoint intent before a registration can commit. Cleanup must retain
            // this generation even when the process dies before saving the reply.
            self.registration_permitted = true;
            return Ok(None);
        }
        let found = self.inventory(ecs).await?;
        if let Some((arn, status)) = found.first_key_value() {
            if status != "ACTIVE" {
                return Err(invalid("Gateway task generation is no longer active"));
            }
            self.revisions.extend(found.keys().cloned());
            return Ok(Some(arn.clone()));
        }
        if self.registration_attempted {
            // There is no ECS client token for RegisterTaskDefinition. Empty eventual
            // discovery cannot establish that an earlier request did not commit.
            return Ok(None);
        }
        self.registration_attempted = true;
        let response = ecs
            .register_task_definition(self.request.clone())
            .await
            .context(cloud("Register gateway task definition"))?;
        // Retain the returned identity before any content validation can fail.
        let arn = response.task_definition.task_definition_arn.clone();
        self.validate_arn(&arn)?;
        self.revisions.insert(arn);
        self.verify(&response)?;
        Ok(None)
    }

    /// Caller must first prove the service and every task stopped and detached.
    pub async fn step_delete(&mut self, ecs: &dyn EcsApi) -> Result<bool> {
        self.delete_inner(ecs)
            .await
            .map_err(preserve_composite_cleanup_error)
    }

    async fn delete_inner(&mut self, ecs: &dyn EcsApi) -> Result<bool> {
        self.retiring = true;
        if !self.registration_permitted {
            return Ok(true);
        }
        let found = self.inventory(ecs).await?;
        self.revisions.extend(found.keys().cloned());
        if self.revisions.is_empty() {
            return Ok(false); // Uncertain registration must never disappear from state.
        }
        for arn in &self.revisions {
            let Some(status) = found.get(arn) else {
                if !self.delete_requested.contains(arn) {
                    return Ok(false);
                }
                match ecs
                    .list_tags_for_resource(ListTagsForResourceRequest {
                        resource_arn: arn.clone(),
                    })
                    .await
                {
                    Err(error) if is_remote_not_found(&error) => continue,
                    Err(error) => return Err(error.context(cloud("Confirm gateway task absence"))),
                    Ok(response) => {
                        self.verify_tags(&response.tags)?;
                        return Ok(false);
                    }
                }
            };
            match status.as_str() {
                "ACTIVE" => {
                    ecs.deregister_task_definition(TaskDefinitionRequest {
                        task_definition: arn.clone(),
                    })
                    .await
                    .context(cloud("Deregister owned gateway task definition"))?;
                    return Ok(false);
                }
                "INACTIVE" => {
                    if self.delete_requested.insert(arn.clone()) {
                        return Ok(false); // Checkpoint before a delete can commit remotely.
                    }
                    let response = ecs
                        .delete_task_definitions(DeleteTaskDefinitionsRequest {
                            task_definitions: vec![arn.clone()],
                        })
                        .await
                        .context(cloud("Delete owned gateway task definition"))?;
                    if !response.failures.is_empty() {
                        return Err(invalid("Gateway task deletion reported batch failures"));
                    }
                    if response.task_definitions.len() != 1
                        || response.task_definitions[0].task_definition_arn != *arn
                    {
                        return Err(invalid(
                            "Gateway task deletion returned a different identity",
                        ));
                    }
                    return Ok(false);
                }
                "DELETE_IN_PROGRESS" => {
                    self.delete_requested.insert(arn.clone());
                    return Ok(false);
                }
                _ => return Err(invalid("Unexpected gateway task status during cleanup")),
            }
        }
        Ok(found.is_empty())
    }

    async fn inventory(&self, ecs: &dyn EcsApi) -> Result<BTreeMap<String, String>> {
        let mut found = BTreeMap::new();
        for status in ["ACTIVE", "INACTIVE", "DELETE_IN_PROGRESS"] {
            let mut next_token = None;
            let mut cursors = BTreeSet::new();
            loop {
                let response = ecs
                    .list_task_definitions(
                        ListTaskDefinitionsRequest::builder()
                            .family_prefix(self.request.family.clone())
                            .status(status.into())
                            .maybe_next_token(next_token)
                            .build(),
                    )
                    .await
                    .context(cloud("Discover owned gateway task family"))?;
                for arn in response.task_definition_arns {
                    self.validate_arn(&arn)?;
                    let metadata = ecs
                        .list_tags_for_resource(ListTagsForResourceRequest {
                            resource_arn: arn.clone(),
                        })
                        .await
                        .context(cloud("Verify gateway task ownership metadata"))?;
                    self.verify_tags(&metadata.tags)?;
                    found.insert(arn, status.to_string());
                }
                next_token = response.next_token;
                let Some(token) = &next_token else {
                    break;
                };
                if token.is_empty() || !cursors.insert(token.clone()) {
                    return Err(invalid(
                        "Gateway task inventory repeated a pagination cursor",
                    ));
                }
            }
        }
        Ok(found)
    }

    fn validate_arn(&self, arn: &str) -> Result<()> {
        let prefix = format!(
            "arn:aws:ecs:{}:{}:task-definition/{}:",
            self.region, self.account_id, self.request.family
        );
        if !arn.strip_prefix(&prefix).is_some_and(|revision| {
            !revision.is_empty() && revision.bytes().all(|b| b.is_ascii_digit())
        }) {
            return Err(invalid(
                "Gateway task identity is outside the recorded family/account/region",
            ));
        }
        Ok(())
    }

    fn verify_tags(&self, actual: &[Tag]) -> Result<()> {
        let tags: BTreeMap<_, _> = actual.iter().map(|tag| (&tag.key, &tag.value)).collect();
        if tags.len() != actual.len()
            || self
                .request
                .tags
                .iter()
                .any(|tag| tags.get(&tag.key) != Some(&&tag.value))
        {
            return Err(invalid(
                "Gateway task tags do not match the recorded generation",
            ));
        }
        Ok(())
    }

    fn verify(&self, response: &TaskDefinitionResponse) -> Result<()> {
        let task = &response.task_definition;
        self.validate_arn(&task.task_definition_arn)?;
        self.verify_tags(&response.tags)?;
        let expected = &self.request.container_definitions[0];
        if task.family != self.request.family
            || task.container_definitions.len() != 1
            || task.network_mode.as_deref() != Some("awsvpc")
            || task.cpu.as_deref() != Some("256")
            || task.memory.as_deref() != Some("512")
            || task.task_role_arn.is_some()
            || task.execution_role_arn.is_some()
        {
            return Err(invalid("Gateway task execution boundary changed"));
        }
        if task.requires_compatibilities != self.request.requires_compatibilities
            || !task.runtime_platform.as_ref().is_some_and(|platform| {
                platform.cpu_architecture == "X86_64" && platform.operating_system_family == "LINUX"
            })
        {
            return Err(invalid("Gateway task runtime platform changed"));
        }
        let actual = &task.container_definitions[0];
        let ports: BTreeSet<_> = actual
            .port_mappings
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|port| port.protocol.as_deref().unwrap_or("tcp") == "tcp")
            .map(|port| port.container_port)
            .collect();
        if ports != BTreeSet::from([i32::from(GATEWAY_HTTP_PORT), i32::from(GATEWAY_HEALTH_PORT)])
            || actual.port_mappings.as_ref().map(Vec::len) != Some(2)
        {
            return Err(invalid("Gateway task listener ports changed"));
        }
        if actual.essential != Some(true)
            || actual.stop_timeout != Some(120)
            || actual.name != expected.name
            || actual.image != expected.image
            || actual.entry_point != expected.entry_point
            || actual.command != expected.command
            || actual.readonly_root_filesystem != expected.readonly_root_filesystem
            || actual
                .environment
                .as_ref()
                .is_some_and(|env| !env.is_empty())
        {
            return Err(invalid("Gateway task container configuration changed"));
        }
        Ok(())
    }
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.into(),
        resource_id: None,
    })
}

fn cloud(message: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: message.into(),
        resource_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::ecs::{
        ListTagsForResourceResponse, ListTaskDefinitionsResponse, MockEcsApi,
    };
    use alien_client_core::ErrorData as ClientError;
    use serde_json::json;

    fn owner() -> GatewayTask {
        GatewayTask::new(
            "123456789012".into(),
            "us-east-1".into(),
            "stack",
            "worker",
            &json!({"static_resources": {}}),
        )
        .unwrap()
    }

    fn reload(owner: &GatewayTask) -> GatewayTask {
        serde_json::from_value(serde_json::to_value(owner).unwrap()).unwrap()
    }

    fn response(owner: &GatewayTask, revision: i32, status: &str) -> TaskDefinitionResponse {
        let request = &owner.request;
        serde_json::from_value(json!({
            "taskDefinition": {
                "taskDefinitionArn": format!("arn:aws:ecs:us-east-1:123456789012:task-definition/{}:{revision}", request.family),
                "family": request.family, "revision": revision, "status": status,
                "networkMode": request.network_mode, "cpu": request.cpu, "memory": request.memory,
                "containerDefinitions": request.container_definitions,
                "runtimePlatform": request.runtime_platform,
                "requiresCompatibilities": request.requires_compatibilities
            }, "tags": request.tags
        })).unwrap()
    }

    fn inventory(ecs: &mut MockEcsApi, items: Vec<TaskDefinitionResponse>) {
        let listing = items.clone();
        ecs.expect_list_task_definitions()
            .times(3)
            .returning(move |request| {
                Ok(ListTaskDefinitionsResponse {
                    task_definition_arns: listing
                        .iter()
                        .filter(|item| item.task_definition.status == request.status)
                        .map(|item| item.task_definition.task_definition_arn.clone())
                        .collect(),
                    next_token: None,
                })
            });
        if !items.is_empty() {
            ecs.expect_list_tags_for_resource()
                .returning(move |request| {
                    Ok(ListTagsForResourceResponse {
                        tags: items
                            .iter()
                            .find(|item| {
                                item.task_definition.task_definition_arn == request.resource_arn
                            })
                            .unwrap()
                            .tags
                            .clone(),
                    })
                });
        }
    }

    #[tokio::test]
    async fn lost_registration_reply_recovers_after_reload_without_another_registration() {
        let mut owner = owner();
        let mut ecs = MockEcsApi::new();
        assert!(owner.step_create(&ecs).await.unwrap().is_none());
        owner = reload(&owner);
        inventory(&mut ecs, vec![]);
        ecs.expect_register_task_definition()
            .times(1)
            .return_once(|_| {
                Err(AlienError::new(ClientError::RemoteServiceUnavailable {
                    message: "lost response".into(),
                }))
            });
        assert!(owner.step_create(&ecs).await.is_err());
        assert!(owner.registration_attempted);
        ecs.checkpoint();
        owner = reload(&owner);
        inventory(&mut ecs, vec![]);
        assert!(owner.step_create(&ecs).await.unwrap().is_none());
        ecs.checkpoint();
        let task = response(&owner, 1, "ACTIVE");
        let arn = task.task_definition.task_definition_arn.clone();
        inventory(&mut ecs, vec![task]);
        assert_eq!(owner.step_create(&ecs).await.unwrap(), Some(arn));
    }

    #[tokio::test]
    async fn persisted_pre_call_intent_is_not_forgotten_after_a_process_crash() {
        let mut owner = owner();
        let mut ecs = MockEcsApi::new();
        assert!(owner.step_create(&ecs).await.unwrap().is_none());
        owner = reload(&owner);
        assert!(!owner.registration_attempted);
        inventory(&mut ecs, vec![]);
        assert!(!owner.step_delete(&ecs).await.unwrap());
    }

    #[tokio::test]
    async fn uncertain_registration_cannot_be_forgotten_by_cleanup() {
        let mut owner = owner();
        owner.registration_permitted = true;
        owner.registration_attempted = true;
        let mut ecs = MockEcsApi::new();
        inventory(&mut ecs, vec![]);
        assert!(!owner.step_delete(&ecs).await.unwrap());
        assert!(reload(&owner).retiring);
        assert!(owner.step_create(&ecs).await.is_err());
    }

    #[tokio::test]
    async fn cleanup_rejects_foreign_generation_before_mutation() {
        let mut owner = owner();
        owner.registration_permitted = true;
        owner.registration_attempted = true;
        let mut foreign = response(&owner, 1, "ACTIVE");
        foreign
            .tags
            .iter_mut()
            .find(|tag| tag.key == CREATE_ATTEMPT_TAG)
            .unwrap()
            .value = "foreign".into();
        let mut ecs = MockEcsApi::new();
        ecs.expect_list_task_definitions().times(1).return_once({
            let arn = foreign.task_definition.task_definition_arn.clone();
            move |_| {
                Ok(ListTaskDefinitionsResponse {
                    task_definition_arns: vec![arn],
                    next_token: None,
                })
            }
        });
        ecs.expect_list_tags_for_resource()
            .times(1)
            .return_once(move |_| Ok(ListTagsForResourceResponse { tags: foreign.tags }));
        assert!(owner.step_delete(&ecs).await.is_err());
    }

    #[tokio::test]
    async fn paginated_family_recovery_retains_every_revision_before_cleanup_mutates() {
        let mut owner = owner();
        owner.registration_permitted = true;
        let first = response(&owner, 1, "ACTIVE");
        let second = response(&owner, 2, "ACTIVE");
        let first_arn = first.task_definition.task_definition_arn.clone();
        let second_arn = second.task_definition.task_definition_arn.clone();
        let mut ecs = MockEcsApi::new();
        ecs.expect_list_task_definitions().times(4).returning({
            let first_arn = first_arn.clone();
            let second_arn = second_arn.clone();
            move |request| {
                let (task_definition_arns, next_token) =
                    match (request.status.as_deref(), request.next_token.as_deref()) {
                        (Some("ACTIVE"), None) => {
                            (vec![first_arn.clone()], Some("second-page".into()))
                        }
                        (Some("ACTIVE"), Some("second-page")) => (vec![second_arn.clone()], None),
                        (Some("INACTIVE" | "DELETE_IN_PROGRESS"), None) => (vec![], None),
                        _ => panic!("unexpected inventory request"),
                    };
                Ok(ListTaskDefinitionsResponse {
                    task_definition_arns,
                    next_token,
                })
            }
        });
        ecs.expect_list_tags_for_resource()
            .returning(move |request| {
                let tags = if request.resource_arn == first.task_definition.task_definition_arn {
                    first.tags.clone()
                } else {
                    assert_eq!(
                        request.resource_arn,
                        second.task_definition.task_definition_arn
                    );
                    second.tags.clone()
                };
                Ok(ListTagsForResourceResponse { tags })
            });
        ecs.expect_deregister_task_definition()
            .times(1)
            .return_once(|_| {
                Err(AlienError::new(ClientError::RemoteAccessDenied {
                    resource_type: "task definition".into(),
                    resource_name: "owned".into(),
                }))
            });
        let error = owner.step_delete(&ecs).await.unwrap_err();
        assert!(!matches!(error.http_status_code, Some(401 | 403 | 404)));
        assert_eq!(
            reload(&owner).revisions,
            BTreeSet::from([first_arn, second_arn])
        );
    }

    #[tokio::test]
    async fn cleanup_keeps_identity_on_lost_delete_and_waits_for_inventory_absence() {
        let mut owner = owner();
        owner.registration_permitted = true;
        owner.registration_attempted = true;
        let task = response(&owner, 1, "INACTIVE");
        let arn = task.task_definition.task_definition_arn.clone();
        let mut ecs = MockEcsApi::new();
        inventory(&mut ecs, vec![task.clone()]);
        assert!(!owner.step_delete(&ecs).await.unwrap());
        owner = reload(&owner);
        ecs.checkpoint();
        inventory(&mut ecs, vec![task]);
        ecs.expect_delete_task_definitions()
            .times(1)
            .return_once(|_| {
                Err(AlienError::new(ClientError::RemoteServiceUnavailable {
                    message: "lost delete reply".into(),
                }))
            });
        assert!(owner.step_delete(&ecs).await.is_err());
        assert!(owner.delete_requested.contains(&arn));
        ecs.checkpoint();
        owner = reload(&owner);
        inventory(&mut ecs, vec![response(&owner, 1, "DELETE_IN_PROGRESS")]);
        assert!(!owner.step_delete(&ecs).await.unwrap());
        ecs.checkpoint();
        owner = reload(&owner);
        inventory(&mut ecs, vec![]);
        ecs.expect_list_tags_for_resource()
            .times(1)
            .return_once(|_| {
                Err(AlienError::new(ClientError::RemoteResourceNotFound {
                    resource_type: "task definition".into(),
                    resource_name: "owned".into(),
                }))
            });
        assert!(owner.step_delete(&ecs).await.unwrap());
    }
}
