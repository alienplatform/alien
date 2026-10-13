//! A generation-specific gateway task family. Registration is not idempotent: after
//! an uncertain reply, discover the recorded family rather than register again.

use std::collections::{BTreeMap, BTreeSet};

use alien_aws_clients::ecs::{
    ContainerDefinition, DeleteTaskDefinitionsRequest, DescribeTaskDefinitionRequest, EcsApi,
    ListTaskDefinitionsRequest, PortMapping, RegisterTaskDefinitionRequest, RuntimePlatform,
    Tag, TaskDefinitionRequest, TaskDefinitionResponse,
};
use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_error::{AlienError, Context};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::aws_gateway_config::{GATEWAY_HEALTH_PORT, GATEWAY_HTTP_PORT, GATEWAY_IMAGE};
use crate::core::aws_tag_scoped::{preserve_composite_cleanup_error, CREATE_ATTEMPT_TAG};
use crate::{ErrorData, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GatewayTask {
    account_id: String,
    region: String,
    request: RegisterTaskDefinitionRequest,
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
        if account_id.len() != 12 || !account_id.bytes().all(|b| b.is_ascii_digit())
            || region.is_empty() || stack_prefix.is_empty() || resource_id.is_empty()
        {
            return Err(invalid("Gateway task requires account, region and ownership"));
        }
        let token = Uuid::new_v4().to_string();
        let tags = [
            (ALIEN_MANAGED_BY_TAG_KEY, "alien"),
            (ALIEN_STACK_TAG_KEY, stack_prefix),
            (ALIEN_RESOURCE_TAG_KEY, resource_id),
            (CREATE_ATTEMPT_TAG, token.as_str()),
        ].into_iter().map(|(key, value)| Tag { key: key.into(), value: value.into() }).collect();
        let request = RegisterTaskDefinitionRequest::builder()
            .family(format!("alien-worker-gateway-{token}"))
            .container_definitions(vec![ContainerDefinition::builder()
                .name("gateway".into())
                .image(GATEWAY_IMAGE.into())
                .essential(true)
                .entry_point(vec!["/usr/local/bin/envoy".into()])
                .command(vec!["--config-yaml".into(), bootstrap.to_string(), "--log-level".into(), "warn".into()])
                .port_mappings([GATEWAY_HTTP_PORT, GATEWAY_HEALTH_PORT].into_iter()
                    .map(|port| PortMapping::builder().container_port(i32::from(port)).protocol("tcp".into()).build()).collect())
                .readonly_root_filesystem(true)
                .stop_timeout(120)
                .build()])
            .network_mode("awsvpc".into())
            .requires_compatibilities(vec!["FARGATE".into()])
            .cpu("256".into()).memory("512".into())
            .runtime_platform(RuntimePlatform::builder().cpu_architecture("X86_64".into()).operating_system_family("LINUX".into()).build())
            .tags(tags).build();
        Ok(Self { account_id, region, request, registration_attempted: false,
            revisions: BTreeSet::new(), delete_requested: BTreeSet::new(), retiring: false })
    }

    /// Parent checkpoints this owner before calling. Pending steps require a polling delay.
    /// An unresolved registration is deliberately retained, including during cleanup.
    pub async fn step_create(&mut self, ecs: &dyn EcsApi) -> Result<Option<String>> {
        if self.retiring { return Err(invalid("A retiring gateway task cannot be reused")); }
        let found = self.inventory(ecs).await?;
        if let Some(arn) = found.first() {
            let task = self.describe(ecs, arn).await?;
            if task.task_definition.status.as_deref() != Some("ACTIVE") {
                return Err(invalid("Gateway task generation is no longer active"));
            }
            self.revisions.extend(found.iter().cloned());
            return Ok(Some(arn.clone()));
        }
        if self.registration_attempted {
            // There is no ECS client token for RegisterTaskDefinition. Empty eventual
            // discovery cannot establish that an earlier request did not commit.
            return Ok(None);
        }
        self.registration_attempted = true;
        let response = ecs.register_task_definition(self.request.clone()).await
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
        self.delete_inner(ecs).await.map_err(preserve_composite_cleanup_error)
    }

    async fn delete_inner(&mut self, ecs: &dyn EcsApi) -> Result<bool> {
        self.retiring = true;
        if !self.registration_attempted { return Ok(true); }
        let found = self.inventory(ecs).await?;
        self.revisions.extend(found.iter().cloned());
        if self.revisions.is_empty() {
            return Ok(false); // Uncertain registration must never disappear from state.
        }
        for arn in &self.revisions {
            if !found.contains(arn) {
                if !self.delete_requested.contains(arn) { return Ok(false); }
                continue;
            }
            let response = self.describe(ecs, arn).await?;
            match response.task_definition.status.as_deref() {
                Some("ACTIVE") => {
                    ecs.deregister_task_definition(TaskDefinitionRequest { task_definition: arn.clone() }).await
                        .context(cloud("Deregister owned gateway task definition"))?;
                    return Ok(false);
                }
                Some("INACTIVE") => {
                    self.delete_requested.insert(arn.clone());
                    let response = ecs.delete_task_definitions(DeleteTaskDefinitionsRequest { task_definitions: vec![arn.clone()] }).await
                        .context(cloud("Delete owned gateway task definition"))?;
                    if !response.failures.is_empty() {
                        return Err(invalid("Gateway task deletion reported batch failures"));
                    }
                    if response.task_definitions.len() != 1 || response.task_definitions[0].task_definition_arn != *arn {
                        return Err(invalid("Gateway task deletion returned a different identity"));
                    }
                    return Ok(false);
                }
                Some("DELETE_IN_PROGRESS") => { self.delete_requested.insert(arn.clone()); return Ok(false); }
                _ => return Err(invalid("Unexpected gateway task status during cleanup")),
            }
        }
        Ok(found.is_empty())
    }

    async fn inventory(&self, ecs: &dyn EcsApi) -> Result<BTreeSet<String>> {
        let mut found = BTreeSet::new();
        for status in ["ACTIVE", "INACTIVE", "DELETE_IN_PROGRESS"] {
            let mut next_token = None;
            let mut cursors = BTreeSet::new();
            loop {
                let response = ecs.list_task_definitions(ListTaskDefinitionsRequest::builder()
                    .family_prefix(self.request.family.clone()).status(status.into())
                    .maybe_next_token(next_token).build()).await
                    .context(cloud("Discover owned gateway task family"))?;
                for arn in response.task_definition_arns {
                    self.validate_arn(&arn)?;
                    self.describe(ecs, &arn).await?;
                    found.insert(arn);
                }
                next_token = response.next_token;
                let Some(token) = &next_token else { break; };
                if token.is_empty() || !cursors.insert(token.clone()) {
                    return Err(invalid("Gateway task inventory repeated a pagination cursor"));
                }
            }
        }
        Ok(found)
    }

    fn validate_arn(&self, arn: &str) -> Result<()> {
        let prefix = format!("arn:aws:ecs:{}:{}:task-definition/{}:", self.region, self.account_id, self.request.family);
        if !arn.strip_prefix(&prefix).is_some_and(|revision| !revision.is_empty() && revision.bytes().all(|b| b.is_ascii_digit())) {
            return Err(invalid("Gateway task identity is outside the recorded family/account/region"));
        }
        Ok(())
    }

    async fn describe(&self, ecs: &dyn EcsApi, arn: &str) -> Result<TaskDefinitionResponse> {
        let response = ecs.describe_task_definition(DescribeTaskDefinitionRequest::builder()
            .task_definition(arn.into()).include(vec!["TAGS".into()]).build()).await
            .context(cloud("Verify gateway task generation"))?;
        if response.task_definition.task_definition_arn != arn {
            return Err(invalid("Gateway task describe returned a different identity"));
        }
        self.verify(&response)?;
        Ok(response)
    }

    fn verify(&self, response: &TaskDefinitionResponse) -> Result<()> {
        let task = &response.task_definition;
        self.validate_arn(&task.task_definition_arn)?;
        let tags: BTreeMap<_, _> = response.tags.iter().map(|tag| (&tag.key, &tag.value)).collect();
        if tags.len() != response.tags.len() || self.request.tags.iter().any(|tag| tags.get(&tag.key) != Some(&&tag.value)) {
            return Err(invalid("Gateway task tags do not match the recorded generation"));
        }
        let expected = &self.request.container_definitions[0];
        if task.family != self.request.family || task.container_definitions.len() != 1
            || task.network_mode.as_deref() != Some("awsvpc") || task.cpu.as_deref() != Some("256")
            || task.memory.as_deref() != Some("512") || task.task_role_arn.is_some() || task.execution_role_arn.is_some()
        {
            return Err(invalid("Gateway task execution boundary changed"));
        }
        let actual = &task.container_definitions[0];
        if actual.name != expected.name || actual.image != expected.image
            || actual.entry_point != expected.entry_point || actual.command != expected.command
            || actual.readonly_root_filesystem != expected.readonly_root_filesystem
            || actual.environment.as_ref().is_some_and(|env| !env.is_empty())
        {
            return Err(invalid("Gateway task container configuration changed"));
        }
        Ok(())
    }
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid { message: message.into(), resource_id: None })
}

fn cloud(message: &str) -> ErrorData {
    ErrorData::CloudPlatformError { message: message.into(), resource_id: None }
}
