//! Durable ownership of a worker's private API interface endpoint.

use std::collections::{BTreeMap, BTreeSet};

use alien_aws_clients::ec2::{
    CreateInterfaceVpcEndpointRequest, DescribeNetworkInterfacesRequest,
    DescribeVpcEndpointsRequest, Ec2Api, Filter, Tag, TagSpecification, VpcEndpoint,
};
use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::aws_tag_scoped::{
    is_remote_not_found, preserve_composite_cleanup_error, CREATE_ATTEMPT_TAG,
};
use crate::{ErrorData, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EndpointPlan {
    pub account_id: String,
    pub vpc_id: String,
    pub service_name: String,
    pub subnet_ids: Vec<String>,
    pub security_group_ids: Vec<String>,
    pub policy: serde_json::Value,
    pub stack_prefix: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Reserved,
    Creating,
    Waiting,
    Ready,
    Deleting,
    WaitingInterfaces,
    Deleted,
}

/// Checkpoint every pending result. The plan and token never change during this lifetime.
/// A parent must retire private routing/domain associations before deleting this child.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PrivateEndpoint {
    plan: EndpointPlan,
    token: String,
    phase: Phase,
    endpoint_id: Option<String>,
    observed: bool,
    delete_requested: bool,
    network_interface_ids: BTreeSet<String>,
}

impl PrivateEndpoint {
    pub fn new(mut plan: EndpointPlan) -> Result<Self> {
        for value in [
            &plan.account_id,
            &plan.vpc_id,
            &plan.service_name,
            &plan.stack_prefix,
            &plan.resource_id,
        ] {
            if value.is_empty() {
                return Err(invalid(
                    "Private endpoint ownership and network fields are required",
                ));
            }
        }
        for values in [&mut plan.subnet_ids, &mut plan.security_group_ids] {
            values.sort();
            if values.is_empty()
                || values.iter().any(String::is_empty)
                || values.windows(2).any(|pair| pair[0] == pair[1])
            {
                return Err(invalid(
                    "Private endpoint attachments must be nonempty and unique",
                ));
            }
        }
        if !plan.policy.is_object() {
            return Err(invalid("Private endpoint policy must be an object"));
        }
        Ok(Self {
            plan,
            token: Uuid::new_v4().to_string(),
            phase: Phase::Reserved,
            endpoint_id: None,
            observed: false,
            delete_requested: false,
            network_interface_ids: BTreeSet::new(),
        })
    }

    pub async fn step_create(&mut self, ec2: &dyn Ec2Api) -> Result<Option<VpcEndpoint>> {
        match self.phase {
            Phase::Reserved => {
                // Persist permission to issue the create before the call can commit remotely.
                self.phase = Phase::Creating;
                return Ok(None);
            }
            Phase::Deleting | Phase::WaitingInterfaces | Phase::Deleted => {
                return Err(invalid("A retiring private endpoint cannot be reused"));
            }
            _ => {}
        }
        if let Some(endpoint) = self.find(ec2).await? {
            self.verify(&endpoint, true)?;
            self.record(&endpoint);
            self.observed = true;
            match endpoint.state.as_str() {
                "available" => {
                    self.phase = Phase::Ready;
                    return Ok(Some(endpoint));
                }
                "pending" | "pendingAcceptance" => {
                    self.phase = Phase::Waiting;
                    return Ok(None);
                }
                _ => return Err(invalid("Private endpoint did not reach an available state")),
            }
        }
        if self.endpoint_id.is_some() {
            if self.observed {
                return Err(invalid(
                    "Recorded private endpoint disappeared; refusing replacement",
                ));
            }
            return Ok(None); // A newly returned ID may not yet be visible to Describe.
        }
        let endpoint = ec2
            .create_interface_vpc_endpoint(self.request()?)
            .await
            .context(cloud("Create private API endpoint"))?
            .vpc_endpoint;
        self.record_create(&endpoint)?;
        self.phase = Phase::Waiting;
        Ok(None)
    }

    pub async fn step_delete(&mut self, ec2: &dyn Ec2Api) -> Result<bool> {
        self.delete_inner(ec2)
            .await
            .map_err(preserve_composite_cleanup_error)
    }

    async fn delete_inner(&mut self, ec2: &dyn Ec2Api) -> Result<bool> {
        if self.phase == Phase::Deleted {
            return Ok(true);
        }
        if self.phase == Phase::Reserved {
            self.phase = Phase::Deleted;
            return Ok(true);
        }
        if self.phase == Phase::WaitingInterfaces {
            return self.wait_interfaces(ec2).await;
        }
        let Some(endpoint) = self.find(ec2).await? else {
            if self.observed && !self.delete_requested {
                return Err(invalid(
                    "Private endpoint disappeared before deletion; retry ownership read",
                ));
            }
            if self.observed {
                self.phase = Phase::WaitingInterfaces;
                return Ok(false);
            }
            if self.endpoint_id.is_some() {
                return Ok(false);
            }
            // An uncertain create must not be forgotten because a discovery read is empty.
            // Replaying its recorded token recovers the exact ID (or creates that generation
            // if the first call never committed), which subsequent steps verify and delete.
            let endpoint = ec2
                .create_interface_vpc_endpoint(self.request()?)
                .await
                .context(cloud("Recover private endpoint create before cleanup"))?
                .vpc_endpoint;
            self.record_create(&endpoint)?;
            self.phase = Phase::Waiting;
            return Ok(false);
        };
        let retiring = matches!(endpoint.state.as_str(), "deleting" | "deleted");
        self.verify(&endpoint, !retiring)?;
        self.record(&endpoint);
        self.observed = true;
        if retiring {
            self.delete_requested = true;
        }
        if endpoint.state == "deleted" {
            self.phase = Phase::WaitingInterfaces;
            return Ok(false);
        }
        if endpoint.state == "deleting" {
            self.phase = Phase::Deleting;
            return Ok(false);
        }
        if self.phase != Phase::Deleting {
            self.phase = Phase::Deleting;
            return Ok(false);
        }
        // Persisted even on a lost response, so a subsequent absence can finish cleanup.
        self.delete_requested = true;
        let result = ec2
            .delete_vpc_endpoints(&[endpoint.vpc_endpoint_id.clone()])
            .await
            .context(cloud("Delete owned private API endpoint"))?;
        for failure in result.unsuccessful.items {
            if failure.resource_id != endpoint.vpc_endpoint_id
                || failure.error.code != "InvalidVpcEndpointId.NotFound"
            {
                return Err(invalid(&format!(
                    "Private endpoint deletion rejected: {}",
                    failure.error.code
                )));
            }
        }
        Ok(false)
    }

    async fn wait_interfaces(&mut self, ec2: &dyn Ec2Api) -> Result<bool> {
        for id in &self.network_interface_ids {
            match ec2
                .describe_network_interfaces(
                    DescribeNetworkInterfacesRequest::builder()
                        .network_interface_ids(vec![id.clone()])
                        .build(),
                )
                .await
            {
                Err(error) if is_remote_not_found(&error) => {}
                Err(error) => {
                    return Err(error.context(cloud("Confirm private endpoint interface absence")))
                }
                Ok(_) => return Ok(false),
            }
        }
        self.phase = Phase::Deleted;
        Ok(true)
    }

    async fn find(&self, ec2: &dyn Ec2Api) -> Result<Option<VpcEndpoint>> {
        let request = match &self.endpoint_id {
            Some(id) => DescribeVpcEndpointsRequest::builder()
                .vpc_endpoint_ids(vec![id.clone()])
                .build(),
            None => DescribeVpcEndpointsRequest::builder()
                .filters(vec![Filter::builder()
                    .name(format!("tag:{CREATE_ATTEMPT_TAG}"))
                    .values(vec![self.token.clone()])
                    .build()])
                .build(),
        };
        let response = match ec2.describe_vpc_endpoints(request).await {
            Ok(response) => response,
            Err(error) if is_remote_not_found(&error) => return Ok(None),
            Err(error) => return Err(error.context(cloud("Read private API endpoint ownership"))),
        };
        if response.next_token.is_some() || response.vpc_endpoint_set.items.len() > 1 {
            return Err(invalid(
                "Private endpoint discovery is incomplete or ambiguous",
            ));
        }
        Ok(response.vpc_endpoint_set.items.into_iter().next())
    }

    fn record_create(&mut self, endpoint: &VpcEndpoint) -> Result<()> {
        if endpoint.vpc_endpoint_id.is_empty() {
            return Err(invalid("Create private endpoint returned no ID"));
        }
        // Create responses may omit optional metadata. Retain the returned identity
        // now; a separate Describe must prove ownership before serving or deleting.
        self.record(endpoint);
        Ok(())
    }

    fn record(&mut self, endpoint: &VpcEndpoint) {
        self.endpoint_id = Some(endpoint.vpc_endpoint_id.clone());
        self.network_interface_ids.extend(
            endpoint
                .network_interface_id_set
                .iter()
                .flat_map(|ids| ids.items.iter().cloned()),
        );
    }

    fn tags(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (ALIEN_STACK_TAG_KEY.into(), self.plan.stack_prefix.clone()),
            (ALIEN_RESOURCE_TAG_KEY.into(), self.plan.resource_id.clone()),
            (ALIEN_MANAGED_BY_TAG_KEY.into(), "runtime".into()),
            (CREATE_ATTEMPT_TAG.into(), self.token.clone()),
        ])
    }

    fn request(&self) -> Result<CreateInterfaceVpcEndpointRequest> {
        let policy = serde_json::to_string(&self.plan.policy)
            .into_alien_error()
            .context(cloud("Encode private endpoint policy"))?;
        Ok(CreateInterfaceVpcEndpointRequest::builder()
            .vpc_id(self.plan.vpc_id.clone())
            .service_name(self.plan.service_name.clone())
            .subnet_ids(self.plan.subnet_ids.clone())
            .security_group_ids(self.plan.security_group_ids.clone())
            .client_token(self.token.clone())
            .private_dns_enabled(false)
            .policy_document(policy)
            .tag_specifications(vec![TagSpecification::builder()
                .resource_type("vpc-endpoint".into())
                .tags(
                    self.tags()
                        .into_iter()
                        .map(|(key, value)| Tag { key, value })
                        .collect(),
                )
                .build()])
            .build())
    }

    fn verify(&self, endpoint: &VpcEndpoint, serving: bool) -> Result<()> {
        if endpoint.vpc_endpoint_id.is_empty()
            || self
                .endpoint_id
                .as_ref()
                .is_some_and(|id| id != &endpoint.vpc_endpoint_id)
            || endpoint.vpc_id != self.plan.vpc_id
            || endpoint.service_name != self.plan.service_name
            || endpoint.vpc_endpoint_type.as_deref() != Some("Interface")
            || endpoint.owner_id.as_deref() != Some(self.plan.account_id.as_str())
            || endpoint.requester_managed != Some(false)
        {
            return Err(invalid("Private endpoint identity or ownership changed"));
        }
        for (key, value) in self.tags() {
            let matches = endpoint
                .tag_set
                .iter()
                .flat_map(|set| &set.items)
                .filter(|tag| tag.key == key)
                .collect::<Vec<_>>();
            if matches.len() != 1 || matches[0].value != value {
                return Err(invalid("Private endpoint generation tags changed"));
            }
        }
        if !serving {
            return Ok(());
        } // EC2 clears attachments during native deletion.
        let mut subnets = endpoint
            .subnet_id_set
            .as_ref()
            .map(|set| set.items.clone())
            .unwrap_or_default();
        subnets.sort();
        let mut groups = endpoint
            .group_set
            .iter()
            .flat_map(|set| &set.items)
            .map(|group| group.group_id.clone())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| invalid("Private endpoint security group ID is missing"))?;
        groups.sort();
        let policy: serde_json::Value = serde_json::from_str(
            endpoint
                .policy_document
                .as_deref()
                .ok_or_else(|| invalid("Private endpoint policy is missing"))?,
        )
        .into_alien_error()
        .context(cloud("Decode private endpoint policy"))?;
        if subnets != self.plan.subnet_ids
            || groups != self.plan.security_group_ids
            || endpoint.private_dns_enabled != Some(false)
            || policy != self.plan.policy
        {
            return Err(invalid("Private endpoint network or policy changed"));
        }
        Ok(())
    }
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(cloud(message))
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
    use alien_aws_clients::ec2::{
        CreateVpcEndpointResponse, DescribeVpcEndpointsResponse, MockEc2Api, VpcEndpointSet,
    };
    use alien_client_core::ErrorData as ClientError;
    use serde_json::json;

    fn owner() -> PrivateEndpoint {
        PrivateEndpoint::new(EndpointPlan {
            account_id: "123456789012".into(),
            vpc_id: "vpc-test".into(),
            service_name: "com.amazonaws.us-east-1.execute-api".into(),
            subnet_ids: vec!["subnet-a".into()],
            security_group_ids: vec!["sg-test".into()],
            policy: json!({"Statement": []}),
            stack_prefix: "test".into(),
            resource_id: "worker".into(),
        })
        .unwrap()
    }

    fn reload(owner: &PrivateEndpoint) -> PrivateEndpoint {
        serde_json::from_value(serde_json::to_value(owner).unwrap()).unwrap()
    }

    fn endpoint(owner: &PrivateEndpoint) -> VpcEndpoint {
        serde_json::from_value(json!({
            "vpcEndpointId": "vpce-test", "vpcEndpointType": "Interface", "vpcId": owner.plan.vpc_id,
            "serviceName": owner.plan.service_name, "state": "available", "ownerId": owner.plan.account_id,
            "requesterManaged": false, "privateDnsEnabled": false, "policyDocument": owner.plan.policy.to_string(),
            "subnetIdSet": {"item": owner.plan.subnet_ids}, "groupSet": {"item": [{"groupId": "sg-test"}]},
            "networkInterfaceIdSet": {"item": ["eni-test"]},
            "tagSet": {"item": owner.tags().into_iter().map(|(key,value)| json!({"key": key, "value": value})).collect::<Vec<_>>()}
        })).unwrap()
    }

    fn describe(ec2: &mut MockEc2Api, value: Option<VpcEndpoint>) {
        ec2.expect_describe_vpc_endpoints()
            .times(1)
            .return_once(move |_| {
                Ok(DescribeVpcEndpointsResponse {
                    vpc_endpoint_set: VpcEndpointSet {
                        items: value.into_iter().collect(),
                    },
                    next_token: None,
                })
            });
    }

    #[tokio::test]
    async fn incomplete_create_response_retains_id_until_verified_after_reload() {
        let mut owner = owner();
        let mut ec2 = MockEc2Api::new();
        assert!(owner.step_create(&ec2).await.unwrap().is_none());
        owner = reload(&owner);
        describe(&mut ec2, None);
        let mut created = endpoint(&owner);
        created.owner_id = None;
        created.tag_set = None;
        ec2.expect_create_interface_vpc_endpoint()
            .times(1)
            .return_once(move |_| {
                Ok(CreateVpcEndpointResponse {
                    vpc_endpoint: created,
                    client_token: None,
                })
            });
        assert!(owner.step_create(&ec2).await.unwrap().is_none());
        assert_eq!(owner.endpoint_id.as_deref(), Some("vpce-test"));
        assert!(!owner.observed);
        ec2.checkpoint();
        owner = reload(&owner);
        describe(&mut ec2, Some(endpoint(&owner)));
        assert_eq!(
            owner
                .step_create(&ec2)
                .await
                .unwrap()
                .unwrap()
                .vpc_endpoint_id,
            "vpce-test"
        );
        assert!(owner.observed);
        ec2.checkpoint();
        describe(&mut ec2, None);
        assert!(owner.step_delete(&ec2).await.is_err());
        assert_ne!(owner.phase, Phase::Deleted);
    }

    #[tokio::test]
    async fn foreign_generation_never_serves_or_deletes() {
        let mut owner = owner();
        owner.phase = Phase::Creating;
        let mut foreign = endpoint(&owner);
        foreign.owner_id = Some("999999999999".into());
        let mut ec2 = MockEc2Api::new();
        describe(&mut ec2, Some(foreign.clone()));
        assert!(owner.step_create(&ec2).await.is_err());
        ec2.checkpoint();
        describe(&mut ec2, Some(foreign));
        assert!(owner.step_delete(&ec2).await.is_err());
        assert!(owner.endpoint_id.is_none());
    }

    #[tokio::test]
    async fn denied_delete_survives_reload_and_waits_for_recorded_interfaces() {
        let mut owner = owner();
        owner.phase = Phase::Creating;
        let mut ec2 = MockEc2Api::new();
        describe(&mut ec2, Some(endpoint(&owner)));
        owner.step_create(&ec2).await.unwrap().unwrap();
        ec2.checkpoint();
        describe(&mut ec2, Some(endpoint(&owner)));
        assert!(!owner.step_delete(&ec2).await.unwrap());
        ec2.checkpoint();
        describe(&mut ec2, Some(endpoint(&owner)));
        ec2.expect_delete_vpc_endpoints()
            .times(1)
            .return_once(|ids| {
                assert_eq!(ids, &["vpce-test".to_string()]);
                Err(AlienError::new(ClientError::RemoteAccessDenied {
                    resource_type: "endpoint".into(),
                    resource_name: "vpce-test".into(),
                }))
            });
        let error = owner.step_delete(&ec2).await.unwrap_err();
        assert_ne!(error.http_status_code, Some(403));
        owner = reload(&owner);
        assert!(owner.delete_requested);
        ec2.checkpoint();
        let mut retiring = endpoint(&owner);
        retiring.state = "deleted".into();
        retiring.subnet_id_set = None;
        retiring.group_set = None;
        retiring.network_interface_id_set = None;
        describe(&mut ec2, Some(retiring));
        assert!(!owner.step_delete(&ec2).await.unwrap());
        assert!(owner.network_interface_ids.contains("eni-test"));
        ec2.checkpoint();
        ec2.expect_describe_network_interfaces()
            .times(1)
            .return_once(|request| {
                assert_eq!(request.network_interface_ids.unwrap(), vec!["eni-test"]);
                Err(AlienError::new(ClientError::RemoteResourceNotFound {
                    resource_type: "interface".into(),
                    resource_name: "eni-test".into(),
                }))
            });
        owner = reload(&owner);
        assert!(owner.step_delete(&ec2).await.unwrap());
        assert_eq!(owner.phase, Phase::Deleted);
    }
    #[tokio::test]
    async fn uncertain_create_cleanup_replays_the_persisted_token_after_empty_discovery() {
        let mut owner = owner();
        let mut ec2 = MockEc2Api::new();
        owner.step_create(&ec2).await.unwrap();
        let token = owner.token.clone();
        describe(&mut ec2, None);
        let expected_token = token.clone();
        ec2.expect_create_interface_vpc_endpoint()
            .times(1)
            .return_once(move |request| {
                assert_eq!(request.client_token, expected_token);
                Err(AlienError::new(ClientError::Timeout {
                    message: "reply lost after commit".into(),
                }))
            });
        assert!(owner.step_create(&ec2).await.is_err());
        assert!(owner.endpoint_id.is_none());
        owner = reload(&owner);
        ec2.checkpoint();
        describe(&mut ec2, None);
        let created = endpoint(&owner);
        ec2.expect_create_interface_vpc_endpoint()
            .times(1)
            .return_once(move |request| {
                assert_eq!(request.client_token, token);
                assert!(!request.private_dns_enabled);
                Ok(CreateVpcEndpointResponse {
                    vpc_endpoint: created,
                    client_token: None,
                })
            });
        assert!(!owner.step_delete(&ec2).await.unwrap());
        assert_eq!(owner.endpoint_id.as_deref(), Some("vpce-test"));
        owner = reload(&owner);
        ec2.checkpoint();
        describe(&mut ec2, Some(endpoint(&owner)));
        assert!(!owner.step_delete(&ec2).await.unwrap());
        ec2.checkpoint();
        describe(&mut ec2, Some(endpoint(&owner)));
        ec2.expect_delete_vpc_endpoints().times(1).return_once(|_| {
            Err(AlienError::new(ClientError::Timeout {
                message: "delete committed, reply lost".into(),
            }))
        });
        assert!(owner.step_delete(&ec2).await.is_err());
        owner = reload(&owner);
        ec2.checkpoint();
        describe(&mut ec2, None);
        assert!(!owner.step_delete(&ec2).await.unwrap());
        ec2.checkpoint();
        ec2.expect_describe_network_interfaces()
            .times(1)
            .return_once(|_| {
                Err(AlienError::new(ClientError::RemoteResourceNotFound {
                    resource_type: "interface".into(),
                    resource_name: "eni-test".into(),
                }))
            });
        assert!(owner.step_delete(&ec2).await.unwrap());
    }
    #[tokio::test]
    async fn partial_delete_failure_keeps_receipt_and_does_not_complete_cleanup() {
        let mut owner = owner();
        owner.phase = Phase::Creating;
        let mut ec2 = MockEc2Api::new();
        describe(&mut ec2, Some(endpoint(&owner)));
        owner.step_create(&ec2).await.unwrap().unwrap();
        ec2.checkpoint();
        describe(&mut ec2, Some(endpoint(&owner)));
        assert!(!owner.step_delete(&ec2).await.unwrap());
        ec2.checkpoint();
        describe(&mut ec2, Some(endpoint(&owner)));
        ec2.expect_delete_vpc_endpoints().times(1).return_once(|_| Ok(
            serde_json::from_value(json!({"unsuccessful": {"item": [{
                "resourceId": "vpce-test", "error": {"code": "UnauthorizedOperation", "message": "denied"}
            }]}})).unwrap()
        ));
        assert!(owner
            .step_delete(&ec2)
            .await
            .unwrap_err()
            .message
            .contains("UnauthorizedOperation"));
        owner = reload(&owner);
        assert_eq!(owner.endpoint_id.as_deref(), Some("vpce-test"));
        assert_ne!(owner.phase, Phase::Deleted);
    }

    #[tokio::test]
    async fn changed_tags_policy_and_attachments_refuse_cloud_mutations() {
        let original = owner();
        for field in ["generation", "policy", "subnets", "groups", "dns", "id"] {
            let mut owner = reload(&original);
            owner.phase = Phase::Waiting;
            owner.endpoint_id = Some("vpce-test".into());
            owner.observed = true;
            let mut changed = endpoint(&owner);
            match field {
                "generation" => changed.tag_set = None,
                "policy" => changed.policy_document = Some("{}".into()),
                "subnets" => changed.subnet_id_set = None,
                "groups" => changed.group_set = None,
                "dns" => changed.private_dns_enabled = Some(true),
                "id" => changed.vpc_endpoint_id = "vpce-other".into(),
                _ => unreachable!(),
            }
            let mut ec2 = MockEc2Api::new();
            describe(&mut ec2, Some(changed.clone()));
            assert!(owner.step_create(&ec2).await.is_err(), "{field}");
            ec2.checkpoint();
            describe(&mut ec2, Some(changed));
            assert!(owner.step_delete(&ec2).await.is_err(), "{field}");
            assert_eq!(owner.endpoint_id.as_deref(), Some("vpce-test"));
        }
    }
}
