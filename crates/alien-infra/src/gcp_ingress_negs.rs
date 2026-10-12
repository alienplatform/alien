use crate::{ErrorData, Result};
use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::gcp::compute::{
    ComputeApi, NetworkEndpointGroup, NetworkEndpointGroupCloudRun, NetworkEndpointType, Operation,
    OperationStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpIngressNegs {
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
    #[serde(default)]
    serverless: Option<ServerlessTarget>,
}

/// Compatibility name for callers managing zonal VM ingress.
pub type GcpVmNegs = GcpIngressNegs;

/// Immutable regional Cloud Run target. Absent in checkpoints for zonal VM NEGs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerlessTarget {
    region: String,
    service: String,
}

/// Regional ingress NEG for one Cloud Run service.
pub struct GcpServerlessNegPlan<'a> {
    pub resource_id: &'a str,
    pub project: &'a str,
    pub region: &'a str,
    pub service: &'a str,
    pub name: &'a str,
    /// An explicitly recorded child from the previous controller version.
    pub legacy_name: Option<&'a str>,
    pub legacy_description: &'a str,
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

impl GcpIngressNegs {
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
        let result = if self.serverless.is_some() {
            client
                .get_region_operation(operation.zone.clone(), operation.name.clone())
                .await
        } else {
            client
                .get_zone_operation(operation.zone.clone(), operation.name.clone())
                .await
        };
        let live = match result {
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
        if self.serverless.is_some() {
            return Err(invalid(
                plan.resource_id,
                "Cannot reconcile a regional serverless NEG as a zonal VM NEG",
            ));
        }
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
            let desired = NetworkEndpointGroup {
                name: Some(name.clone()),
                description: Some(expected),
                network: Some(plan.network.to_string()),
                subnetwork: Some(plan.subnet.to_string()),
                network_endpoint_type: Some(NetworkEndpointType::GceVmIpPort),
                default_port: Some(default_port as i32),
                ..Default::default()
            };
            if !self
                .reconcile_child(client, zone, name, desired, plan.resource_id, true)
                .await?
            {
                return Ok(false);
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

    pub async fn reconcile_serverless(
        &mut self,
        client: &dyn ComputeApi,
        plan: GcpServerlessNegPlan<'_>,
    ) -> Result<bool> {
        if plan.region.is_empty() || plan.service.is_empty() {
            return Err(invalid(
                plan.resource_id,
                "Serverless ingress requires a region and service",
            ));
        }
        if self.network.is_some() || self.subnet.is_some() || self.port.is_some() {
            return Err(invalid(
                plan.resource_id,
                "Cannot reconcile a zonal VM NEG as a regional serverless NEG",
            ));
        }
        if self
            .project
            .as_deref()
            .is_some_and(|project| project != plan.project)
        {
            return Err(invalid(
                plan.resource_id,
                "NEG reconciliation requires its original project",
            ));
        }
        if self
            .serverless
            .as_ref()
            .is_some_and(|target| target.region != plan.region || target.service != plan.service)
        {
            return Err(invalid(
                plan.resource_id,
                "Serverless NEG region and service are immutable",
            ));
        }
        self.project = Some(plan.project.to_string());
        self.serverless = Some(ServerlessTarget {
            region: plan.region.to_string(),
            service: plan.service.to_string(),
        });
        if !self.poll(client, plan.resource_id).await? {
            return Ok(false);
        }
        let token = self.token.get_or_insert_with(Uuid::new_v4);
        let description = format!("Alien serverless ingress NEG {token}");
        let name = self
            .names
            .entry(plan.region.to_string())
            .or_insert_with(|| plan.legacy_name.unwrap_or(plan.name).to_string())
            .clone();
        let description = self
            .descriptions
            .entry(plan.region.to_string())
            .or_insert_with(|| {
                if plan.legacy_name == Some(name.as_str()) {
                    plan.legacy_description.to_string()
                } else {
                    description
                }
            })
            .clone();
        let desired = NetworkEndpointGroup {
            name: Some(name.clone()),
            description: Some(description),
            network_endpoint_type: Some(NetworkEndpointType::Serverless),
            cloud_run: Some(NetworkEndpointGroupCloudRun {
                service: Some(plan.service.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        self.reconcile_child(
            client,
            plan.region,
            name,
            desired,
            plan.resource_id,
            plan.legacy_name.is_none(),
        )
        .await
    }

    async fn get_child(
        &self,
        client: &dyn ComputeApi,
        location: &str,
        name: String,
    ) -> alien_client_core::Result<NetworkEndpointGroup> {
        if self.serverless.is_some() {
            client
                .get_region_network_endpoint_group(location.to_string(), name)
                .await
        } else {
            client
                .get_network_endpoint_group(location.to_string(), name)
                .await
        }
    }

    async fn reconcile_child(
        &mut self,
        client: &dyn ComputeApi,
        location: &str,
        name: String,
        desired: NetworkEndpointGroup,
        id: &str,
        allow_create: bool,
    ) -> Result<bool> {
        match self.get_child(client, location, name).await {
            Ok(live) => self.confirm(location, &live, id)?,
            Err(error) if missing(&error) => {
                if self.provider_ids.contains_key(location) || !allow_create {
                    return Err(invalid(id, "Previously confirmed ingress NEG is missing"));
                }
                let operation = if self.serverless.is_some() {
                    client
                        .insert_region_network_endpoint_group(location.to_string(), desired)
                        .await
                } else {
                    client
                        .insert_network_endpoint_group(location.to_string(), desired)
                        .await
                }
                .context(cloud_error(id, "Create ingress NEG"))?;
                self.checkpoint_operation(operation, location, id)?;
                return Ok(false);
            }
            Err(error) => return Err(error.context(cloud_error(id, "Read ingress NEG"))),
        }
        Ok(true)
    }

    fn confirm(&mut self, zone: &str, live: &NetworkEndpointGroup, id: &str) -> Result<()> {
        if live.description.as_ref() != self.descriptions.get(zone) {
            return Err(invalid(id, "Refusing a foreign ingress NEG"));
        }
        let target_matches = if let Some(target) = &self.serverless {
            zone == target.region
                && live.network_endpoint_type == Some(NetworkEndpointType::Serverless)
                && live.network.is_none()
                && live.subnetwork.is_none()
                && live.default_port.is_none()
                && live.app_engine.is_none()
                && live.cloud_function.is_none()
                && live.cloud_run.as_ref().is_some_and(|run| {
                    run.service.as_deref() == Some(target.service.as_str())
                        && run.tag.is_none()
                        && run.url_mask.is_none()
                })
        } else {
            live.network.as_deref().map(relative) == self.network.as_deref().map(relative)
                && live.subnetwork.as_deref().map(relative) == self.subnet.as_deref().map(relative)
                && live.network_endpoint_type == Some(NetworkEndpointType::GceVmIpPort)
                && live.default_port == self.port.map(i32::from)
                && live.cloud_run.is_none()
                && live.app_engine.is_none()
                && live.cloud_function.is_none()
        };
        if !target_matches {
            return Err(invalid(
                id,
                "Recorded NEG has an unexpected target, network, type, or port",
            ));
        }
        let provider_id = live
            .id
            .as_ref()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid(id, "Ingress NEG has no provider ID"))?;
        if self
            .provider_ids
            .get(zone)
            .is_some_and(|previous| previous != provider_id)
        {
            return Err(invalid(id, "Refusing a replaced ingress NEG"));
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
        match self.get_child(client, &zone, name.clone()).await {
            Ok(live) => self.confirm(&zone, &live, id)?,
            Err(error) if missing(&error) => {
                self.forget(&zone);
                return Ok(false);
            }
            Err(error) => {
                return Err(error.context(cloud_error(id, "Inspect ingress NEG before cleanup")))
            }
        }
        let result = if self.serverless.is_some() {
            client
                .delete_region_network_endpoint_group(zone.clone(), name)
                .await
        } else {
            client
                .delete_network_endpoint_group(zone.clone(), name)
                .await
        };
        match result {
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
        scoped_provider(cloud, false)
    }

    fn scoped_provider(cloud: Arc<Mutex<Cloud>>, regional: bool) -> MockComputeApi {
        let mut client = MockComputeApi::new();
        let store = cloud.clone();
        let get = move |location, name| {
            store
                .lock()
                .unwrap()
                .objects
                .get(&(location, name))
                .cloned()
                .ok_or_else(not_found)
        };
        let store = cloud.clone();
        let insert = move |location, mut neg: NetworkEndpointGroup| {
            let mut store = store.lock().unwrap();
            let key = (location, neg.name.clone().unwrap());
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
        };
        let delete = move |location, name| {
            let mut store = cloud.lock().unwrap();
            if std::mem::take(&mut store.deny_delete) {
                return Err(AlienError::new(ClientErrorData::AuthenticationError {
                    message: "permission revoked".to_string(),
                }));
            }
            assert!(store.objects.remove(&(location, name)).is_some());
            store.mutations += 1;
            Ok(operation())
        };
        if regional {
            client
                .expect_get_region_network_endpoint_group()
                .returning(get);
            client
                .expect_insert_region_network_endpoint_group()
                .returning(insert);
            client
                .expect_delete_region_network_endpoint_group()
                .returning(delete);
            client
                .expect_get_region_operation()
                .returning(|_, _| Ok(operation()));
        } else {
            client.expect_get_network_endpoint_group().returning(get);
            client
                .expect_insert_network_endpoint_group()
                .returning(insert);
            client
                .expect_delete_network_endpoint_group()
                .returning(delete);
            client
                .expect_get_zone_operation()
                .returning(|_, _| Ok(operation()));
        }
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

    fn serverless_plan() -> GcpServerlessNegPlan<'static> {
        GcpServerlessNegPlan {
            resource_id: "api",
            project: "test-project",
            region: "us-central1",
            service: "api-service",
            name: "test-api-neg",
            legacy_name: None,
            legacy_description: "Serverless NEG for worker api",
        }
    }

    #[tokio::test]
    async fn serverless_neg_recovers_lost_create_and_refuses_replacement_or_changed_target() {
        let cloud = Arc::new(Mutex::new(Cloud {
            lose_insert: true,
            ..Default::default()
        }));
        let client = scoped_provider(cloud.clone(), true);
        let mut state = GcpVmNegs::default();
        assert!(state
            .reconcile_serverless(&client, serverless_plan())
            .await
            .is_err());
        state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        assert!(state
            .reconcile_serverless(&client, serverless_plan())
            .await
            .unwrap());
        assert_eq!(cloud.lock().unwrap().mutations, 1);
        let key = ("us-central1".to_string(), "test-api-neg".to_string());
        let original = cloud.lock().unwrap().objects[&key].clone();
        for change in 0..4 {
            let mut changed = original.clone();
            match change {
                0 => changed.id = Some("replaced".to_string()),
                1 => changed.cloud_run.as_mut().unwrap().service = Some("foreign".to_string()),
                2 => changed.cloud_run.as_mut().unwrap().url_mask = Some("<service>".to_string()),
                _ => changed.description = Some("foreign owner".to_string()),
            }
            cloud.lock().unwrap().objects.insert(key.clone(), changed);
            assert!(state
                .reconcile_serverless(&client, serverless_plan())
                .await
                .is_err());
            assert!(state.delete(&client, "test-project", "api").await.is_err());
            assert_eq!(cloud.lock().unwrap().mutations, 1);
        }
        cloud.lock().unwrap().objects.insert(key.clone(), original);
        let mut changed = serverless_plan();
        changed.region = "europe-west1";
        assert!(state.reconcile_serverless(&client, changed).await.is_err());
        let mut changed = serverless_plan();
        changed.service = "other-service";
        assert!(state.reconcile_serverless(&client, changed).await.is_err());
        cloud.lock().unwrap().deny_delete = true;
        assert!(state.delete(&client, "test-project", "api").await.is_err());
        assert_eq!(state.names.len(), 1);
        for _ in 0..10 {
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
        assert_eq!(cloud.lock().unwrap().mutations, 2);
    }

    #[tokio::test]
    async fn serverless_neg_polls_regional_operations_and_never_recreates_a_confirmed_child() {
        let cloud = Arc::new(Mutex::new(Cloud::default()));
        let client = scoped_provider(cloud.clone(), true);
        let mut state = GcpVmNegs::default();
        let mut ready = false;
        for _ in 0..10 {
            state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
            let before = cloud.lock().unwrap().mutations;
            ready = state
                .reconcile_serverless(&client, serverless_plan())
                .await
                .unwrap();
            assert!(cloud.lock().unwrap().mutations - before <= 1);
            if ready {
                break;
            }
        }
        assert!(ready);
        cloud.lock().unwrap().objects.clear();
        assert!(state
            .reconcile_serverless(&client, serverless_plan())
            .await
            .unwrap_err()
            .message
            .contains("missing"));
        assert_eq!(cloud.lock().unwrap().mutations, 1);
        assert!(!state.delete(&client, "test-project", "api").await.unwrap());
        assert!(state.delete(&client, "test-project", "api").await.unwrap());
    }

    #[tokio::test]
    async fn recorded_serverless_neg_is_verified_without_recreating_a_missing_legacy_child() {
        let cloud = Arc::new(Mutex::new(Cloud::default()));
        let client = scoped_provider(cloud.clone(), true);
        let mut state = GcpIngressNegs::default();
        let mut plan = serverless_plan();
        plan.legacy_name = Some("legacy-neg");
        assert!(state.reconcile_serverless(&client, plan).await.is_err());
        assert_eq!(cloud.lock().unwrap().mutations, 0);
        let key = ("us-central1".to_string(), "legacy-neg".to_string());
        cloud.lock().unwrap().objects.insert(
            key,
            NetworkEndpointGroup {
                id: Some("legacy-id".to_string()),
                name: Some("legacy-neg".to_string()),
                description: Some("Serverless NEG for worker api".to_string()),
                network_endpoint_type: Some(NetworkEndpointType::Serverless),
                cloud_run: Some(NetworkEndpointGroupCloudRun {
                    service: Some("api-service".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        let mut plan = serverless_plan();
        plan.legacy_name = Some("legacy-neg");
        assert!(state.reconcile_serverless(&client, plan).await.unwrap());
        assert_eq!(state.provider_ids["us-central1"], "legacy-id");
        assert_eq!(cloud.lock().unwrap().mutations, 0);
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
