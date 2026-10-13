//! Live Fargate service ownership inside an explicitly owned ECS cluster.
//! The parent owns its task family, target group, network and cluster separately.

use std::collections::{BTreeMap, BTreeSet};

use alien_aws_clients::ecs::{
    CreateServiceRequest, DeleteServiceRequest, DescribeServicesRequest, DescribeTasksRequest,
    EcsApi, ListTasksRequest, Service, UpdateServiceRequest,
};
use alien_core::{
    ALIEN_MANAGED_BY_TAG_KEY, ALIEN_MANAGED_BY_TAG_VALUE, ALIEN_RESOURCE_TAG_KEY,
    ALIEN_STACK_TAG_KEY,
};
use alien_error::{AlienError, Context};
use serde::{Deserialize, Serialize};

use crate::core::aws_tag_scoped::{preserve_composite_cleanup_error, CREATE_ATTEMPT_TAG};
use crate::{ErrorData, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GatewayService {
    request: CreateServiceRequest,
    expected_arn: String,
    create_permitted: bool,
    observed: bool,
    retiring: bool,
    delete_requested: bool,
    /// Retained until the parent independently verifies ENI absence before removing its SG.
    pub network_interface_ids: BTreeSet<String>,
}

impl GatewayService {
    pub fn new(request: CreateServiceRequest) -> Result<Self> {
        let Some((cluster_prefix, cluster_name)) = request.cluster.split_once(":cluster/") else {
            return Err(invalid("Gateway service requires an explicit cluster ARN"));
        };
        let tags: BTreeMap<_, _> = request
            .tags
            .iter()
            .map(|tag| (&tag.key, &tag.value))
            .collect();
        let has_boundary_tags = [ALIEN_STACK_TAG_KEY, ALIEN_RESOURCE_TAG_KEY]
            .into_iter()
            .all(|key| {
                request
                    .tags
                    .iter()
                    .any(|tag| tag.key == key && !tag.value.is_empty())
            })
            && request.tags.iter().any(|tag| {
                tag.key == ALIEN_MANAGED_BY_TAG_KEY && tag.value == ALIEN_MANAGED_BY_TAG_VALUE
            });
        if !has_boundary_tags
            || !cluster_prefix.starts_with("arn:aws:ecs:")
            || cluster_name.is_empty()
            || cluster_name.contains('/')
            || request.service_name.is_empty()
            || request.service_name.contains('/')
            || !request.service_name.ends_with(&request.client_token)
            || !request
                .task_definition
                .starts_with(&format!("{cluster_prefix}:task-definition/"))
            || request.client_token.is_empty()
            || !request.client_token.is_ascii()
            || tags.len() != request.tags.len()
            || tags.get(&CREATE_ATTEMPT_TAG.to_string()) != Some(&&request.client_token)
            || request.launch_type != "FARGATE"
            || request.platform_version != "1.4.0"
            || request.desired_count < 2
            || request.task_definition.is_empty()
            || request.load_balancers.len() != 1
        {
            return Err(invalid(
                "Gateway service requires a tagged generation, Fargate task and target group",
            ));
        }
        let network = &request.network_configuration.awsvpc_configuration;
        if network.subnets.iter().collect::<BTreeSet<_>>().len() != network.subnets.len()
            || network
                .security_groups
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != network.security_groups.len()
            || network
                .subnets
                .iter()
                .chain(&network.security_groups)
                .any(String::is_empty)
            || network.subnets.len() < 2
            || network.security_groups.is_empty()
            || !matches!(network.assign_public_ip.as_str(), "ENABLED" | "DISABLED")
        {
            return Err(invalid(
                "Gateway service requires explicit subnet, security group and IP assignment",
            ));
        }
        let expected_arn = format!(
            "{cluster_prefix}:service/{cluster_name}/{}",
            request.service_name
        );
        Ok(Self {
            request,
            expected_arn,
            create_permitted: false,
            observed: false,
            retiring: false,
            delete_requested: false,
            network_interface_ids: BTreeSet::new(),
        })
    }

    /// Parent checkpoints pending results and independently checks target-group health.
    pub async fn step_create(&mut self, ecs: &dyn EcsApi) -> Result<bool> {
        if self.retiring {
            return Err(invalid("A retiring gateway service cannot be reused"));
        }
        if !self.create_permitted {
            self.create_permitted = true;
            return Ok(false);
        }
        let Some(service) = self.find(ecs).await? else {
            if self.observed {
                return Err(invalid(
                    "Observed gateway service disappeared; refusing replacement",
                ));
            }
            self.create(ecs).await?;
            return Ok(false);
        };
        self.observed = true;
        self.inventory_tasks(ecs).await?;
        if service.status.as_deref() != Some("ACTIVE") {
            return Err(invalid("Gateway service is not active"));
        }
        if service
            .deployments
            .iter()
            .any(|deployment| deployment.rollout_state.as_deref() == Some("FAILED"))
        {
            return Err(invalid("Gateway service deployment failed"));
        }
        Ok(service.desired_count == Some(self.request.desired_count)
            && service.running_count == Some(self.request.desired_count)
            && service.pending_count == Some(0)
            && service.deployments.len() == 1
            && service.deployments[0].rollout_state.as_deref() == Some("COMPLETED"))
    }

    /// Retire incoming routing before draining. Completion still requires parent ENI checks.
    pub async fn step_delete(&mut self, ecs: &dyn EcsApi) -> Result<bool> {
        self.delete_inner(ecs)
            .await
            .map_err(preserve_composite_cleanup_error)
    }

    async fn delete_inner(&mut self, ecs: &dyn EcsApi) -> Result<bool> {
        self.retiring = true;
        if !self.create_permitted {
            return Ok(true);
        }
        let service = self.find(ecs).await?;
        let Some(service) = service else {
            if self.delete_requested && self.observed {
                return self.inventory_tasks(ecs).await;
            }
            if self.observed {
                return Err(invalid(
                    "Gateway service disappeared before requested deletion",
                ));
            }
            // Replay the same token and parameters to resolve a lost create reply.
            self.create(ecs).await?;
            return Ok(false);
        };
        self.observed = true;
        let tasks_stopped = self.inventory_tasks(ecs).await?;
        if service.status.as_deref() == Some("INACTIVE") {
            if !self.delete_requested {
                return Err(invalid(
                    "Gateway service became inactive before requested deletion",
                ));
            }
            return Ok(tasks_stopped);
        }
        if service.desired_count != Some(0) {
            ecs.update_service(
                UpdateServiceRequest::builder()
                    .cluster(self.request.cluster.clone())
                    .service(self.expected_arn.clone())
                    .desired_count(0)
                    .build(),
            )
            .await
            .context(cloud("Drain owned gateway service"))?;
            return Ok(false);
        }
        if !tasks_stopped || service.running_count != Some(0) || service.pending_count != Some(0) {
            return Ok(false);
        }
        if service.status.as_deref() == Some("DRAINING") {
            return Ok(false);
        }
        if !self.delete_requested {
            self.delete_requested = true;
            return Ok(false); // Persist permission to delete before a process crash can lose the reply.
        }
        ecs.delete_service(DeleteServiceRequest {
            cluster: self.request.cluster.clone(),
            service: self.expected_arn.clone(),
            force: false,
        })
        .await
        .context(cloud("Delete drained gateway service"))?;
        Ok(false)
    }

    async fn create(&mut self, ecs: &dyn EcsApi) -> Result<()> {
        let response = ecs
            .create_service(self.request.clone())
            .await
            .context(cloud("Create gateway service"))?;
        self.verify(&response.service)?;
        // A create response does not prove tasks healthy or a service visible to Describe.
        Ok(())
    }

    async fn find(&self, ecs: &dyn EcsApi) -> Result<Option<Service>> {
        let response = ecs
            .describe_services(
                DescribeServicesRequest::builder()
                    .cluster(self.request.cluster.clone())
                    .services(vec![self.expected_arn.clone()])
                    .include(vec!["TAGS".into()])
                    .build(),
            )
            .await
            .context(cloud("Read gateway service ownership"))?;
        if !response.failures.is_empty() {
            if response.services.is_empty() && response.failures.len() == 1 {
                let failure = &response.failures[0];
                if failure.reason.as_deref() == Some("MISSING")
                    && failure.arn.as_deref() == Some(self.expected_arn.as_str())
                {
                    return Ok(None);
                }
            }
            return Err(invalid(
                "Gateway service describe reported unexpected batch failures",
            ));
        }
        if response.services.len() != 1 {
            return Err(invalid(
                "Gateway service describe did not return exactly one service",
            ));
        }
        let service = response
            .services
            .into_iter()
            .next()
            .ok_or_else(|| invalid("Gateway service response is empty"))?;
        self.verify(&service)?;
        Ok(Some(service))
    }

    fn verify(&self, service: &Service) -> Result<()> {
        if service.service_arn != self.expected_arn
            || service.service_name != self.request.service_name
            || service.cluster_arn.as_deref() != Some(self.request.cluster.as_str())
            || service.task_definition.as_deref() != Some(self.request.task_definition.as_str())
            || service.launch_type.as_deref() != Some("FARGATE")
            || service.platform_version.as_deref() != Some(self.request.platform_version.as_str())
        {
            return Err(invalid("Gateway service identity or task changed"));
        }
        let tags: BTreeMap<_, _> = service
            .tags
            .iter()
            .map(|tag| (&tag.key, &tag.value))
            .collect();
        if tags.len() != service.tags.len()
            || self
                .request
                .tags
                .iter()
                .any(|tag| tags.get(&tag.key) != Some(&&tag.value))
        {
            return Err(invalid("Gateway service has foreign generation tags"));
        }
        let Some(network) = &service.network_configuration else {
            return Err(invalid("Gateway service network is missing"));
        };
        let actual = &network.awsvpc_configuration;
        let expected = &self.request.network_configuration.awsvpc_configuration;
        let same_set = |left: &[String], right: &[String]| {
            left.len() == right.len()
                && left.iter().collect::<BTreeSet<_>>() == right.iter().collect::<BTreeSet<_>>()
        };
        if !same_set(&actual.subnets, &expected.subnets)
            || !same_set(&actual.security_groups, &expected.security_groups)
            || actual.assign_public_ip != expected.assign_public_ip
            || service.load_balancers.len() != 1
        {
            return Err(invalid("Gateway service network or target group changed"));
        }
        let actual = &service.load_balancers[0];
        let expected = &self.request.load_balancers[0];
        if actual.target_group_arn != expected.target_group_arn
            || actual.container_name != expected.container_name
            || actual.container_port != expected.container_port
        {
            return Err(invalid("Gateway service target registration changed"));
        }
        Ok(())
    }

    async fn inventory_tasks(&mut self, ecs: &dyn EcsApi) -> Result<bool> {
        let mut stopped = true;
        let mut tasks = BTreeSet::new();
        for status in ["RUNNING", "STOPPED"] {
            let mut cursor = None;
            let mut cursors = BTreeSet::new();
            loop {
                let response = ecs
                    .list_tasks(
                        ListTasksRequest::builder()
                            .cluster(self.request.cluster.clone())
                            .service_name(self.request.service_name.clone())
                            .desired_status(status.into())
                            .maybe_next_token(cursor)
                            .build(),
                    )
                    .await
                    .context(cloud("Inventory gateway service tasks"))?;
                tasks.extend(response.task_arns);
                cursor = response.next_token;
                let Some(token) = &cursor else {
                    break;
                };
                if token.is_empty() || !cursors.insert(token.clone()) {
                    return Err(invalid("Gateway task inventory repeated a cursor"));
                }
            }
        }
        let tasks: Vec<_> = tasks.into_iter().collect();
        for chunk in tasks.chunks(100) {
            let response = ecs
                .describe_tasks(
                    DescribeTasksRequest::builder()
                        .cluster(self.request.cluster.clone())
                        .tasks(chunk.to_vec())
                        .build(),
                )
                .await
                .context(cloud("Record gateway task network interfaces"))?;
            if !response.failures.is_empty() || response.tasks.len() != chunk.len() {
                return Err(invalid("Gateway task inventory was incomplete"));
            }
            let returned: BTreeSet<_> = response.tasks.iter().map(|task| &task.task_arn).collect();
            if returned.len() != chunk.len() || chunk.iter().any(|arn| !returned.contains(arn)) {
                return Err(invalid(
                    "Gateway task inventory returned different identities",
                ));
            }
            for task in response.tasks {
                if task.cluster_arn.as_deref() != Some(self.request.cluster.as_str())
                    || task.group.as_deref()
                        != Some(format!("service:{}", self.request.service_name).as_str())
                    || task.task_definition_arn.as_deref()
                        != Some(self.request.task_definition.as_str())
                {
                    return Err(invalid(
                        "Gateway service inventory contained a foreign task",
                    ));
                }
                for attachment in task.attachments {
                    if attachment.attachment_type.as_deref() == Some("ElasticNetworkInterface") {
                        for detail in attachment.details {
                            if detail.name == "networkInterfaceId" {
                                if !detail.value.starts_with("eni-") {
                                    return Err(invalid(
                                        "Gateway task interface identity is invalid",
                                    ));
                                }
                                self.network_interface_ids.insert(detail.value);
                            }
                        }
                    }
                }
                stopped &= task.last_status.as_deref() == Some("STOPPED");
            }
        }
        Ok(stopped)
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
        DescribeServicesResponse, ListTasksResponse, MockEcsApi, ServiceResponse,
    };
    use alien_client_core::ErrorData as ClientError;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn owner() -> GatewayService {
        GatewayService::new(serde_json::from_value(json!({
            "cluster":"arn:aws:ecs:us-east-1:123456789012:cluster/owned", "serviceName":"gateway-generation", "clientToken":"generation",
            "taskDefinition":"arn:aws:ecs:us-east-1:123456789012:task-definition/owned:1", "desiredCount":2, "launchType":"FARGATE", "platformVersion":"1.4.0",
            "networkConfiguration":{"awsvpcConfiguration":{"subnets":["subnet-a","subnet-b"],"securityGroups":["sg-owned"],"assignPublicIp":"ENABLED"}},
            "loadBalancers":[{"targetGroupArn":"owned-target","containerName":"gateway","containerPort":8080}],
            "deploymentConfiguration":{"minimumHealthyPercent":100,"maximumPercent":200},"healthCheckGracePeriodSeconds":60,
            "tags":[{"key":"CreateAttempt","value":"generation"},{"key":"managed-by","value":"runtime"},{"key":"deployment","value":"stack"},{"key":"resource","value":"worker"}]
        })).unwrap()).unwrap()
    }

    fn reload(owner: &GatewayService) -> GatewayService {
        serde_json::from_value(serde_json::to_value(owner).unwrap()).unwrap()
    }

    fn service(owner: &GatewayService) -> Service {
        let request = &owner.request;
        serde_json::from_value(json!({"serviceArn":owner.expected_arn,"serviceName":request.service_name,"clusterArn":request.cluster,
            "taskDefinition":request.task_definition,"desiredCount":2,"runningCount":2,"pendingCount":0,"status":"ACTIVE",
            "launchType":"FARGATE","platformVersion":"1.4.0","networkConfiguration":request.network_configuration,
            "loadBalancers":request.load_balancers,"tags":request.tags,"deployments":[{"rolloutState":"COMPLETED"}]})).unwrap()
    }

    #[tokio::test]
    async fn lost_create_reply_recovers_from_pre_call_checkpoint_without_duplicate_service() {
        let mut owner = owner();
        let world = Arc::new(Mutex::new(None::<Service>));
        let mut ecs = MockEcsApi::new();
        let read = world.clone();
        let arn = owner.expected_arn.clone();
        ecs.expect_describe_services().returning(move |request| {
            assert_eq!(request.services, vec![arn.clone()]);
            assert_eq!(request.include, Some(vec!["TAGS".into()]));
            match &*read.lock().unwrap() {
                Some(service) => Ok(DescribeServicesResponse {
                    services: vec![service.clone()],
                    failures: vec![],
                }),
                None => Ok(serde_json::from_value(
                    json!({"services":[],"failures":[{"arn":arn,"reason":"MISSING"}]}),
                )
                .unwrap()),
            }
        });
        let created = service(&owner);
        ecs.expect_create_service()
            .times(1)
            .return_once(move |request| {
                assert_eq!(request.client_token, "generation");
                *world.lock().unwrap() = Some(created);
                Err(AlienError::new(ClientError::Timeout {
                    message: "committed create reply lost".into(),
                }))
            });
        ecs.expect_list_tasks().times(2).returning(|_| {
            Ok(ListTasksResponse {
                task_arns: vec![],
                next_token: None,
            })
        });
        assert!(!owner.step_create(&ecs).await.unwrap());
        let checkpoint = reload(&owner);
        assert!(owner.step_create(&ecs).await.is_err());
        owner = checkpoint;
        assert!(owner.step_create(&ecs).await.unwrap());
    }

    #[tokio::test]
    async fn drain_waits_for_tasks_and_keeps_interface_ids_after_lost_delete_and_crash() {
        let mut owner = owner();
        owner.create_permitted = true;
        owner.observed = true;
        let world = Arc::new(Mutex::new((service(&owner), false)));
        let mut ecs = MockEcsApi::new();
        let read = world.clone();
        ecs.expect_describe_services().returning(move |_| {
            Ok(DescribeServicesResponse {
                services: vec![read.lock().unwrap().0.clone()],
                failures: vec![],
            })
        });
        let read = world.clone();
        ecs.expect_list_tasks().returning(move |request| {
            let stopped = read.lock().unwrap().1;
            Ok(ListTasksResponse {
                task_arns: if request.desired_status.as_deref()
                    == Some(if stopped { "STOPPED" } else { "RUNNING" })
                {
                    vec!["owned-task".into()]
                } else {
                    vec![]
                },
                next_token: None,
            })
        });
        let read = world.clone();
        let cluster = owner.request.cluster.clone();
        let definition = owner.request.task_definition.clone();
        ecs.expect_describe_tasks().returning(move |_| {
            let stopped = read.lock().unwrap().1;
            Ok(serde_json::from_value(json!({"tasks":[{"taskArn":"owned-task","clusterArn":cluster,"taskDefinitionArn":definition,
                "group":"service:gateway-generation","lastStatus":if stopped {"STOPPED"} else {"RUNNING"},
                "attachments":if stopped {json!([])} else {json!([{"type":"ElasticNetworkInterface","details":[{"name":"networkInterfaceId","value":"eni-owned"}]}])}}],"failures":[]})).unwrap())
        });
        let write = world.clone();
        ecs.expect_update_service()
            .times(1)
            .return_once(move |request| {
                assert_eq!(request.desired_count, Some(0));
                assert!(request.task_definition.is_none());
                let mut state = write.lock().unwrap();
                state.0.desired_count = Some(0);
                Ok(ServiceResponse {
                    service: state.0.clone(),
                })
            });
        let write = world.clone();
        ecs.expect_delete_service()
            .times(1)
            .return_once(move |request| {
                assert!(!request.force);
                let mut state = write.lock().unwrap();
                assert!(state.1);
                assert_eq!(state.0.running_count, Some(0));
                state.0.status = Some("INACTIVE".into());
                Err(AlienError::new(ClientError::Timeout {
                    message: "committed delete reply lost".into(),
                }))
            });
        assert!(!owner.step_delete(&ecs).await.unwrap());
        owner = reload(&owner);
        assert!(!owner.step_delete(&ecs).await.unwrap());
        {
            let mut state = world.lock().unwrap();
            state.1 = true;
            state.0.running_count = Some(0);
        }
        assert!(!owner.step_delete(&ecs).await.unwrap());
        let checkpoint = reload(&owner);
        assert!(owner.step_delete(&ecs).await.is_err());
        owner = checkpoint;
        assert!(owner.step_delete(&ecs).await.unwrap());
        assert_eq!(
            owner.network_interface_ids,
            BTreeSet::from(["eni-owned".into()])
        );
    }

    #[tokio::test]
    async fn foreign_service_tags_prevent_drain_or_delete() {
        let mut owner = owner();
        owner.create_permitted = true;
        let mut foreign = service(&owner);
        foreign.tags[0].value = "foreign".into();
        let mut ecs = MockEcsApi::new();
        ecs.expect_describe_services()
            .times(1)
            .return_once(move |_| {
                Ok(DescribeServicesResponse {
                    services: vec![foreign],
                    failures: vec![],
                })
            });
        assert!(owner.step_delete(&ecs).await.is_err());
    }
}
