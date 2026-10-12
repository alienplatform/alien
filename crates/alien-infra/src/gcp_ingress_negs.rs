use crate::{ErrorData, Result};
use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::gcp::compute::{
    ComputeApi, NetworkEndpointGroup, NetworkEndpointType, Operation, OperationStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpVmNegs {
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    token: Option<Uuid>,
    #[serde(default)]
    network: Option<String>,
    #[serde(default)]
    subnet: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    descriptions: BTreeMap<String, String>,
    #[serde(default)]
    provider_ids: BTreeMap<String, String>,
    #[serde(default)]
    operation: Option<NegOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NegOperation {
    zone: String,
    name: String,
    #[serde(default)]
    polls: u32,
}

pub struct GcpVmNegPlan<'a> {
    pub resource_id: &'a str,
    pub project: &'a str,
    pub network: &'a str,
    pub subnet: &'a str,
    pub name: &'a str,
    pub zones: &'a BTreeSet<String>,
    pub port: u16,
    /// Only the recorded legacy NEG may use its pre-token description.
    pub legacy_zone: Option<&'a str>,
    /// Names persisted by a controller before token-based NEG ownership.
    pub legacy_names: Option<&'a BTreeMap<String, String>>,
    pub legacy_description: &'a str,
}

impl GcpVmNegs {
    pub fn initialized(&self) -> bool {
        self.project.is_some()
    }

    pub fn reset_poll_count(&mut self) {
        if let Some(operation) = &mut self.operation {
            operation.polls = 0;
        }
    }

    async fn poll(&mut self, client: &dyn ComputeApi, id: &str) -> Result<bool> {
        let Some(operation) = &mut self.operation else {
            return Ok(true);
        };
        if operation.polls >= 180 {
            return Err(invalid(id, "NEG operation polling timed out"));
        }
        let live = match client
            .get_zone_operation(operation.zone.clone(), operation.name.clone())
            .await
        {
            Ok(live) => live,
            Err(error) if missing(&error) => {
                self.operation = None;
                return Ok(false);
            }
            Err(error) => return Err(error.context(cloud_error(id, "Poll NEG operation"))),
        };
        operation.polls += 1;
        if live.has_error() {
            self.operation = None;
            return Err(invalid(
                id,
                "NEG operation failed; retry will inspect the recorded resource",
            ));
        }
        if live.status == Some(OperationStatus::Done) {
            self.operation = None;
        }
        // Persist completion before another mutation.
        Ok(false)
    }

    fn checkpoint_operation(&mut self, operation: Operation, zone: &str, id: &str) -> Result<()> {
        let name = operation
            .name
            .ok_or_else(|| invalid(id, "NEG command returned no operation name"))?;
        self.operation = Some(NegOperation {
            zone: zone.to_string(),
            name,
            polls: 0,
        });
        Ok(())
    }

    pub async fn reconcile(
        &mut self,
        client: &dyn ComputeApi,
        plan: GcpVmNegPlan<'_>,
    ) -> Result<bool> {
        match &self.project {
            Some(project) if project != plan.project => {
                return Err(invalid(
                    plan.resource_id,
                    "NEG reconciliation requires its original project",
                ))
            }
            None => self.project = Some(plan.project.to_string()),
            _ => (),
        }
        if self
            .network
            .as_deref()
            .is_some_and(|network| relative(network) != relative(plan.network))
            || self
                .subnet
                .as_deref()
                .is_some_and(|subnet| relative(subnet) != relative(plan.subnet))
        {
            return Err(invalid(
                plan.resource_id,
                "Ingress NEG network and subnet are immutable",
            ));
        }
        self.network = Some(plan.network.to_string());
        self.subnet = Some(plan.subnet.to_string());
        // Horizon registers an explicit VM port for each endpoint. The NEG's
        // fallback port is only a creation default; retain it across workload
        // port changes rather than replacing a shared, live NEG.
        let default_port = *self.port.get_or_insert(plan.port);
        if !self.poll(client, plan.resource_id).await? {
            return Ok(false);
        }
        let token = self.token.get_or_insert_with(Uuid::new_v4);
        let description = format!("Alien VM ingress NEG {token}");
        for zone in plan.zones {
            // Save identity and ownership before the request, including a lost response.
            let name = self
                .names
                .entry(zone.clone())
                .or_insert_with(|| plan.name.to_string())
                .clone();
            let expected = self
                .descriptions
                .entry(zone.clone())
                .or_insert_with(|| {
                    if plan.legacy_zone == Some(zone.as_str())
                        || plan.legacy_names.and_then(|names| names.get(zone)) == Some(&name)
                    {
                        plan.legacy_description.to_string()
                    } else {
                        description.clone()
                    }
                })
                .clone();
            match client
                .get_network_endpoint_group(zone.clone(), name.clone())
                .await
            {
                Ok(live) => {
                    self.confirm(zone, &live, plan.resource_id)?;
                }
                Err(error) if missing(&error) => {
                    if self.provider_ids.contains_key(zone) {
                        return Err(invalid(
                            plan.resource_id,
                            "Previously confirmed zonal ingress NEG is missing",
                        ));
                    }
                    let operation = client
                        .insert_network_endpoint_group(
                            zone.clone(),
                            NetworkEndpointGroup {
                                name: Some(name),
                                description: Some(expected),
                                network: Some(plan.network.to_string()),
                                subnetwork: Some(plan.subnet.to_string()),
                                network_endpoint_type: Some(NetworkEndpointType::GceVmIpPort),
                                default_port: Some(default_port as i32),
                                ..Default::default()
                            },
                        )
                        .await
                        .context(cloud_error(plan.resource_id, "Create zonal ingress NEG"))?;
                    self.checkpoint_operation(operation, zone, plan.resource_id)?;
                    return Ok(false);
                }
                Err(error) => {
                    return Err(
                        error.context(cloud_error(plan.resource_id, "Read zonal ingress NEG"))
                    )
                }
            }
        }
        if plan.zones.is_empty() {
            return Err(invalid(
                plan.resource_id,
                "Ingress requires at least one compute zone",
            ));
        }
        Ok(true)
    }

    fn confirm(&mut self, zone: &str, live: &NetworkEndpointGroup, id: &str) -> Result<()> {
        if live.description.as_ref() != self.descriptions.get(zone) {
            return Err(invalid(id, "Refusing a foreign zonal ingress NEG"));
        }
        if live.network.as_deref().map(relative) != self.network.as_deref().map(relative)
            || live.subnetwork.as_deref().map(relative) != self.subnet.as_deref().map(relative)
            || live.network_endpoint_type != Some(NetworkEndpointType::GceVmIpPort)
            || live.default_port != self.port.map(i32::from)
        {
            return Err(invalid(
                id,
                "Recorded NEG has unexpected network, subnet, type, or port",
            ));
        }
        let provider_id = live
            .id
            .as_ref()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid(id, "Zonal ingress NEG has no provider ID"))?;
        if self
            .provider_ids
            .get(zone)
            .is_some_and(|previous| previous != provider_id)
        {
            return Err(invalid(id, "Refusing a replaced zonal ingress NEG"));
        }
        self.provider_ids
            .insert(zone.to_string(), provider_id.clone());
        Ok(())
    }

    pub async fn delete(
        &mut self,
        client: &dyn ComputeApi,
        project: &str,
        id: &str,
    ) -> Result<bool> {
        if self
            .project
            .as_deref()
            .is_some_and(|original| original != project)
        {
            return Err(invalid(id, "NEG cleanup requires its original project"));
        }
        if !self.poll(client, id).await? {
            return Ok(false);
        }
        let Some((zone, name)) = self
            .names
            .last_key_value()
            .map(|(zone, name)| (zone.clone(), name.clone()))
        else {
            return Ok(true);
        };
        match client
            .get_network_endpoint_group(zone.clone(), name.clone())
            .await
        {
            Ok(live) => self.confirm(&zone, &live, id)?,
            Err(error) if missing(&error) => {
                self.forget(&zone);
                return Ok(false);
            }
            Err(error) => {
                return Err(error.context(cloud_error(id, "Inspect ingress NEG before cleanup")))
            }
        }
        match client
            .delete_network_endpoint_group(zone.clone(), name)
            .await
        {
            Ok(operation) => self.checkpoint_operation(operation, &zone, id)?,
            Err(error) if missing(&error) => self.forget(&zone),
            Err(error) => return Err(error.context(cloud_error(id, "Delete ingress NEG"))),
        }
        Ok(false)
    }

    fn forget(&mut self, zone: &str) {
        self.names.remove(zone);
        self.descriptions.remove(zone);
        self.provider_ids.remove(zone);
    }
}

fn relative(reference: &str) -> &str {
    reference
        .find("projects/")
        .map_or(reference, |index| &reference[index..])
}

fn missing(error: &alien_client_core::Error) -> bool {
    matches!(
        error.error,
        Some(ClientErrorData::RemoteResourceNotFound { .. })
    )
}

fn invalid(id: &str, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.to_string(),
        resource_id: Some(id.to_string()),
    })
}

fn cloud_error(id: &str, message: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: message.to_string(),
        resource_id: Some(id.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_gcp_clients::gcp::compute::MockComputeApi;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Cloud {
        objects: BTreeMap<(String, String), NetworkEndpointGroup>,
        mutations: usize,
        lose_insert: bool,
        deny_delete: bool,
    }

    fn not_found() -> alien_client_core::Error {
        AlienError::new(ClientErrorData::RemoteResourceNotFound {
            resource_type: "NEG".to_string(),
            resource_name: "test".to_string(),
        })
    }

    fn operation() -> Operation {
        Operation {
            name: Some("operation".to_string()),
            status: Some(OperationStatus::Done),
            ..Default::default()
        }
    }

    fn provider(cloud: Arc<Mutex<Cloud>>) -> MockComputeApi {
        let mut client = MockComputeApi::new();
        let store = cloud.clone();
        client
            .expect_get_network_endpoint_group()
            .returning(move |zone, name| {
                store
                    .lock()
                    .unwrap()
                    .objects
                    .get(&(zone, name))
                    .cloned()
                    .ok_or_else(not_found)
            });
        let store = cloud.clone();
        client
            .expect_insert_network_endpoint_group()
            .returning(move |zone, mut neg| {
                let mut store = store.lock().unwrap();
                let key = (zone, neg.name.clone().unwrap());
                assert!(!store.objects.contains_key(&key), "duplicate create");
                store.mutations += 1;
                neg.id = Some(store.mutations.to_string());
                store.objects.insert(key, neg);
                if std::mem::take(&mut store.lose_insert) {
                    return Err(AlienError::new(ClientErrorData::AuthenticationError {
                        message: "response lost after creation".to_string(),
                    }));
                }
                Ok(operation())
            });
        client
            .expect_get_zone_operation()
            .returning(|_, _| Ok(operation()));
        client
            .expect_delete_network_endpoint_group()
            .returning(move |zone, name| {
                let mut store = cloud.lock().unwrap();
                if std::mem::take(&mut store.deny_delete) {
                    return Err(AlienError::new(ClientErrorData::AuthenticationError {
                        message: "permission revoked".to_string(),
                    }));
                }
                assert!(store.objects.remove(&(zone, name)).is_some());
                store.mutations += 1;
                Ok(operation())
            });
        client
    }

    fn plan(zones: &BTreeSet<String>) -> GcpVmNegPlan<'_> {
        GcpVmNegPlan {
            resource_id: "api",
            project: "test-project",
            network: "projects/test-project/global/networks/vpc",
            subnet: "projects/test-project/regions/us-central1/subnetworks/nodes",
            name: "test-api-neg",
            zones,
            port: 8080,
            legacy_zone: None,
            legacy_names: None,
            legacy_description: "NEG for daemon api",
        }
    }

    #[tokio::test]
    async fn zonal_negs_resume_lost_response_and_retain_ownership_on_failed_cleanup() {
        let cloud = Arc::new(Mutex::new(Cloud {
            lose_insert: true,
            ..Default::default()
        }));
        let client = provider(cloud.clone());
        let zones = BTreeSet::from(["us-central1-a".to_string(), "us-central1-b".to_string()]);
        let mut state = GcpVmNegs::default();
        assert!(state.reconcile(&client, plan(&zones)).await.is_err());
        assert_eq!(
            state.names.len(),
            1,
            "create intent survives a lost response"
        );
        for _ in 0..20 {
            state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
            let before = cloud.lock().unwrap().mutations;
            let ready = state.reconcile(&client, plan(&zones)).await.unwrap();
            assert!(cloud.lock().unwrap().mutations - before <= 1);
            if ready {
                break;
            }
        }
        assert_eq!(state.names.len(), 2);
        assert_eq!(cloud.lock().unwrap().mutations, 2);
        let mut changed_port = plan(&zones);
        changed_port.port = 9090;
        assert!(state.reconcile(&client, changed_port).await.unwrap());
        assert_eq!(
            cloud.lock().unwrap().mutations,
            2,
            "explicit endpoint port changes must preserve shared NEGs"
        );
        assert!(cloud
            .lock()
            .unwrap()
            .objects
            .values()
            .all(|neg| neg.default_port == Some(8080)));
        let key = ("us-central1-a".to_string(), "test-api-neg".to_string());
        let original = cloud.lock().unwrap().objects[&key].id.clone();
        cloud.lock().unwrap().objects.get_mut(&key).unwrap().id = Some("replacement".to_string());
        assert!(state
            .reconcile(&client, plan(&zones))
            .await
            .unwrap_err()
            .to_string()
            .contains("replaced"));
        cloud.lock().unwrap().objects.get_mut(&key).unwrap().id = original;
        cloud.lock().unwrap().objects.remove(&key).unwrap();
        let before = cloud.lock().unwrap().mutations;
        assert!(state
            .reconcile(&client, plan(&zones))
            .await
            .unwrap_err()
            .message
            .contains("missing"));
        assert_eq!(
            cloud.lock().unwrap().mutations,
            before,
            "a missing confirmed NEG must not be recreated before its immutable identity check"
        );
        cloud.lock().unwrap().deny_delete = true;
        assert!(state.delete(&client, "test-project", "api").await.is_err());
        assert_eq!(
            state.names.len(),
            2,
            "permission failure must retain cleanup ownership"
        );
        for _ in 0..20 {
            state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
            let before = cloud.lock().unwrap().mutations;
            let deleted = state.delete(&client, "test-project", "api").await.unwrap();
            assert!(cloud.lock().unwrap().mutations - before <= 1);
            if deleted {
                break;
            }
        }
        assert!(state.names.is_empty());
        assert!(cloud.lock().unwrap().objects.is_empty());
    }

    #[tokio::test]
    async fn matching_legacy_name_and_description_cannot_adopt_a_foreign_network() {
        let zones = BTreeSet::from(["us-central1-a".to_string()]);
        let cloud = Arc::new(Mutex::new(Cloud::default()));
        cloud.lock().unwrap().objects.insert(
            ("us-central1-a".to_string(), "test-api-neg".to_string()),
            NetworkEndpointGroup {
                name: Some("test-api-neg".to_string()),
                description: Some("NEG for daemon api".to_string()),
                id: Some("foreign".to_string()),
                network: Some("projects/test-project/global/networks/other".to_string()),
                subnetwork: Some(plan(&zones).subnet.to_string()),
                default_port: Some(8080),
                network_endpoint_type: Some(NetworkEndpointType::GceVmIpPort),
                ..Default::default()
            },
        );
        let client = provider(cloud.clone());
        let mut state = GcpVmNegs::default();
        let mut plan = plan(&zones);
        plan.legacy_zone = Some("us-central1-a");
        assert!(state.reconcile(&client, plan).await.is_err());
        assert!(state.delete(&client, "test-project", "api").await.is_err());
        assert_eq!(cloud.lock().unwrap().mutations, 0);
        assert_eq!(cloud.lock().unwrap().objects.len(), 1);
    }
}
