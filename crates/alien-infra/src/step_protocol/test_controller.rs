//! A controller that speaks the step protocol, for tests of the host side.
//!
//! It answers steps for an `acme/dns-zone` resource, the way an extension's controller binary
//! does: it rebuilds a controller from the request, applies the pending transitions, steps it,
//! and answers with the view. Its cloud is a JSON file, so a test can see each call it made and
//! tell it to fail, stall or hang once.

use super::host::{self, step_client_config};
use super::protocol::{Credentials, LoadRequest, LoadResponse, StepRequest, StepResponse};
use crate::core::{MockPlatformServiceProvider, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_core::{
    AwsClientConfig, AwsCredentials, ClientConfig, CustomHeartbeat, CustomHeartbeatData,
    CustomHeartbeatStatus, HeartbeatBackend, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceDefinition, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceOutputsDefinition, ResourceRef, ResourceStatus, ResourceType,
};
use alien_error::AlienError;
use alien_macros::controller;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::any::Any;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// The custom type this controller serves.
pub const DNS_ZONE: &str = "acme/dns-zone";

/// The config of an `acme/dns-zone` resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsZone {
    pub id: String,
    pub domain: String,
}

impl ResourceDefinition for DnsZone {
    fn get_resource_type(&self) -> ResourceType {
        ResourceType::from(DNS_ZONE)
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn get_dependencies(&self) -> Vec<ResourceRef> {
        Vec::new()
    }
    fn validate_update(&self, _new_config: &dyn ResourceDefinition) -> alien_core::Result<()> {
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }
    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<DnsZone>() == Some(self)
    }
    fn to_json_value(&self) -> serde_json::Result<Value> {
        serde_json::to_value(self)
    }
}

/// The outputs of an `acme/dns-zone` resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsZoneOutputs {
    pub zone_id: String,
    pub domain: String,
}

impl ResourceOutputsDefinition for DnsZoneOutputs {
    fn get_resource_type(&self) -> ResourceType {
        ResourceType::from(DNS_ZONE)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }
    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<DnsZoneOutputs>() == Some(self)
    }
    fn to_json_value(&self) -> serde_json::Result<Value> {
        serde_json::to_value(self)
    }
}

// ---------------------------------------------------------------- the mock cloud

/// A zone in the mock cloud.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockZone {
    pub id: String,
    pub domain: String,
    /// How many times the controller polled it; it is active after the first poll.
    pub polls: u32,
    pub deleted: bool,
}

/// What the mock cloud holds, and every call the controller made to it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockCloudState {
    /// Keyed by caller reference, which makes create idempotent.
    pub zones: BTreeMap<String, MockZone>,
    pub calls: Vec<String>,
}

/// What a test tells the controller to do, each once.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockControl {
    /// "retryable" or "fatal": the next create fails that way.
    pub fail_once: Option<String>,
    /// The next refresh of a ready zone fails, not retryable.
    pub fail_refresh_once: bool,
    /// The next create blocks its thread forever after the cloud call: only killing the
    /// process ends it.
    pub hang_once: bool,
    /// The next create blocks its thread this long after the cloud call, so its reply comes
    /// after the host gave up.
    pub block_once_ms: Option<u64>,
    /// The next create waits this long after the cloud call, so the controller's own deadline
    /// cuts it short.
    pub slow_once_ms: Option<u64>,
}

/// The mock cloud in one directory: `zones.json` and `control.json`.
pub struct MockCloud {
    dir: PathBuf,
}

tokio::task_local! {
    /// The cloud a step in this process uses. Set per step, so tests in one process each
    /// have their own.
    static CLOUD_DIR: PathBuf;
}

fn cloud_error(message: impl Into<String>, retryable: bool) -> AlienError<ErrorData> {
    let mut error = AlienError::new(ErrorData::CloudPlatformError {
        message: message.into(),
        resource_id: None,
    });
    error.retryable = retryable;
    error
}

impl MockCloud {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The step's cloud: the scope `ControllerHost::handle` sets, or a process's own,
    /// `MOCK_CLOUD_DIR`, when it serves stdin and stdout.
    fn current() -> Result<Self> {
        CLOUD_DIR
            .try_with(|dir| Self::at(dir.clone()))
            .or_else(|_| std::env::var("MOCK_CLOUD_DIR").map(Self::at))
            .map_err(|_| cloud_error("no mock cloud for this step", false))
    }

    fn read<T: Default + serde::de::DeserializeOwned>(&self, name: &str) -> T {
        std::fs::read_to_string(self.dir.join(name))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) {
        let json = serde_json::to_string_pretty(value).expect("mock cloud state serializes");
        std::fs::write(self.dir.join(name), json).expect("mock cloud state is writable");
    }

    pub fn state(&self) -> MockCloudState {
        self.read("zones.json")
    }

    pub fn control(&self) -> MockControl {
        self.read("control.json")
    }

    pub fn set_control(&self, control: &MockControl) {
        self.write("control.json", control)
    }

    /// Puts a ready zone in the cloud, as a setup template would have made it.
    pub fn seed_zone(&self, caller_reference: &str, zone_id: &str, domain: &str) {
        let mut state = self.state();
        state.zones.insert(
            caller_reference.to_string(),
            MockZone {
                id: zone_id.to_string(),
                domain: domain.to_string(),
                polls: 2,
                deleted: false,
            },
        );
        self.write("zones.json", &state);
    }

    /// Written when a create starts to hang, so a test knows when to kill the process.
    pub fn hanging_marker(&self) -> PathBuf {
        self.dir.join("hanging")
    }

    /// The calls whose text starts with `prefix`.
    pub fn calls(&self, prefix: &str) -> Vec<String> {
        self.state()
            .calls
            .into_iter()
            .filter(|call| call.starts_with(prefix))
            .collect()
    }

    fn record(&self, call: String) {
        let mut state = self.state();
        state.calls.push(call);
        self.write("zones.json", &state);
    }

    fn take_fail_once(&self) -> Result<()> {
        let mut control = self.control();
        if let Some(kind) = control.fail_once.take() {
            self.set_control(&control);
            self.record(format!("create failed ({kind})"));
            return Err(cloud_error(
                format!("the mock cloud refused the create ({kind})"),
                kind == "retryable",
            ));
        }
        Ok(())
    }

    fn take_fail_refresh_once(&self) -> Result<()> {
        let mut control = self.control();
        if control.fail_refresh_once {
            control.fail_refresh_once = false;
            self.set_control(&control);
            self.record("refresh failed".to_string());
            return Err(cloud_error("the mock cloud refused the refresh", false));
        }
        Ok(())
    }

    async fn take_stall(&self) {
        let mut control = self.control();
        if let Some(ms) = control.slow_once_ms.take() {
            self.set_control(&control);
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
        if let Some(ms) = control.block_once_ms.take() {
            self.set_control(&control);
            std::thread::sleep(Duration::from_millis(ms));
        }
        if control.hang_once {
            control.hang_once = false;
            self.set_control(&control);
            std::fs::write(self.hanging_marker(), "").expect("marker is writable");
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
    }

    fn create_zone(&self, caller_reference: &str, domain: &str) -> MockZone {
        let mut state = self.state();
        let count = state.zones.len();
        let zone = state
            .zones
            .entry(caller_reference.to_string())
            .or_insert_with(|| MockZone {
                id: format!("Z{count}{}", caller_reference.len()),
                domain: domain.to_string(),
                polls: 0,
                deleted: false,
            })
            .clone();
        state
            .calls
            .push(format!("create {caller_reference} {domain}"));
        self.write("zones.json", &state);
        zone
    }

    fn zone_mut<'a>(state: &'a mut MockCloudState, zone_id: &str) -> Result<&'a mut MockZone> {
        state
            .zones
            .values_mut()
            .find(|zone| zone.id == zone_id)
            .ok_or_else(|| cloud_error(format!("no zone {zone_id}"), false))
    }

    fn poll_active(&self, zone_id: &str) -> Result<bool> {
        let mut state = self.state();
        let zone = Self::zone_mut(&mut state, zone_id)?;
        zone.polls += 1;
        let active = zone.polls > 1;
        state.calls.push(format!("poll {zone_id}"));
        self.write("zones.json", &state);
        Ok(active)
    }

    fn get_zone(&self, zone_id: &str) -> Result<MockZone> {
        let mut state = self.state();
        let zone = Self::zone_mut(&mut state, zone_id)?.clone();
        if zone.deleted {
            return Err(cloud_error(format!("zone {zone_id} is deleted"), false));
        }
        state.calls.push(format!("get {zone_id}"));
        self.write("zones.json", &state);
        Ok(zone)
    }

    fn update_zone(&self, zone_id: &str, from: &str, to: &str) -> Result<()> {
        let mut state = self.state();
        Self::zone_mut(&mut state, zone_id)?.domain = to.to_string();
        state.calls.push(format!("update {zone_id} {from} -> {to}"));
        self.write("zones.json", &state);
        Ok(())
    }

    fn delete_zone(&self, zone_id: &str) -> Result<()> {
        let mut state = self.state();
        Self::zone_mut(&mut state, zone_id)?.deleted = true;
        state.calls.push(format!("delete {zone_id}"));
        self.write("zones.json", &state);
        Ok(())
    }
}

// ---------------------------------------------------------------- the controller

#[controller]
pub struct DnsZoneController {
    pub zone_id: Option<String>,
    pub domain: Option<String>,
}

#[controller]
impl DnsZoneController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning, progress = "Creating the hosted zone")]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let zone = ctx.desired_resource_config::<DnsZone>()?;
        let cloud = MockCloud::current()?;
        // A dependency's outputs are what a controller may read about it.
        for (id, dependency) in &ctx.state.resources {
            if id != &zone.id {
                let outputs = serde_json::to_string(&dependency.outputs).unwrap_or_default();
                cloud.record(format!("read {id} {outputs}"));
            }
        }
        cloud.take_fail_once()?;
        // Unique per deployment and resource, so a create that runs again after a lost reply
        // finds the zone it made instead of making a second one.
        let created = cloud.create_zone(
            &format!("{}-{}", ctx.resource_prefix, zone.id),
            &zone.domain,
        );
        self.zone_id = Some(created.id);
        self.domain = Some(zone.domain.clone());
        cloud.take_stall().await;
        Ok(HandlerAction::Continue {
            state: CreateWait,
            suggested_delay: None,
        })
    }

    #[handler(state = CreateWait, on_failure = CreateFailed, status = ResourceStatus::Provisioning, progress = "Waiting for the zone to become active")]
    async fn create_wait(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cloud = MockCloud::current()?;
        let zone_id = self.zone_id()?;
        if cloud.poll_active(&zone_id)? {
            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Stay {
                max_times: Some(5),
                suggested_delay: Some(Duration::from_secs(2)),
            })
        }
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cloud = MockCloud::current()?;
        cloud.take_fail_refresh_once()?;
        let zone = cloud.get_zone(&self.zone_id()?)?;
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: ctx.desired_config.id().to_string(),
            resource_type: ResourceType::from(DNS_ZONE),
            controller_platform: ctx.platform,
            backend: HeartbeatBackend::Test,
            observed_at: chrono::Utc::now(),
            data: ResourceHeartbeatData::Custom(CustomHeartbeat {
                resource_type: ResourceType::from(DNS_ZONE),
                data: CustomHeartbeatData {
                    status: CustomHeartbeatStatus {
                        health: ObservedHealth::Healthy,
                        lifecycle: ProviderLifecycleState::Running,
                        message: None,
                        stale: false,
                        partial: false,
                        collection_issues: Vec::new(),
                    },
                    details: BTreeMap::from([("domain".to_string(), json!(zone.domain))]),
                },
            }),
            raw: Vec::new(),
        });
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating, progress = "Changing the zone's domain")]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let previous = ctx.previous_resource_config::<DnsZone>()?;
        let next = ctx.desired_resource_config::<DnsZone>()?;
        let cloud = MockCloud::current()?;
        cloud.update_zone(&self.zone_id()?, &previous.domain, &next.domain)?;
        self.domain = Some(next.domain.clone());
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting, progress = "Deleting the zone")]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cloud = MockCloud::current()?;
        if let Some(zone_id) = &self.zone_id {
            cloud.delete_zone(zone_id)?;
        }
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        Some(ResourceOutputs::new(DnsZoneOutputs {
            zone_id: self.zone_id.clone()?,
            domain: self.domain.clone()?,
        }))
    }
}

impl DnsZoneController {
    fn zone_id(&self) -> Result<String> {
        self.zone_id
            .clone()
            .ok_or_else(|| cloud_error("the controller has no zone id", false))
    }
}

// ---------------------------------------------------------------- the protocol side

/// The `ClientConfig` of a step: the step's own credentials, or mock AWS credentials when it
/// carries none, since the mock cloud needs none.
fn client_config(
    credentials: Option<&Credentials>,
    platform: Platform,
) -> std::result::Result<ClientConfig, String> {
    match (credentials, platform) {
        (Some(_), Platform::Aws) => step_client_config(credentials, platform),
        (None, Platform::Aws) => Ok(ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIATESTCONTROLLER".to_string(),
                secret_access_key: "test".to_string(),
                session_token: None,
            },
            service_overrides: None,
        }))),
        (None, Platform::Test) => Ok(ClientConfig::Test),
        (credentials, platform) => Err(format!(
            "this controller takes no {credentials:?} credentials on {platform:?}"
        )),
    }
}

/// Answers steps and `load` messages for `acme/dns-zone` on AWS and the test platform, with
/// the mock cloud in one directory.
pub struct ControllerHost {
    host: host::ControllerHost,
}

impl ControllerHost {
    /// A controller whose cloud is the directory `cloud_dir`.
    pub fn new(cloud_dir: impl Into<PathBuf>, log: Option<PathBuf>) -> Self {
        let mut host = host::ControllerHost::new(
            client_config,
            Arc::new(MockPlatformServiceProvider::new()),
            log,
        );
        for platform in [Platform::Aws, Platform::Test] {
            host.register::<DnsZone, _>(
                ResourceType::from(DNS_ZONE),
                platform,
                DnsZoneController::default(),
            )
            .expect("one controller per platform");
        }
        // A step runs as a task of its own, so its cloud goes with it.
        let cloud_dir: PathBuf = cloud_dir.into();
        host.scope_runs(Arc::new(move |run| {
            use futures::FutureExt as _;
            CLOUD_DIR.scope(cloud_dir.clone(), run).boxed()
        }));
        Self { host }
    }

    /// Answers one step in this controller's mock cloud.
    pub async fn handle(&self, request: StepRequest) -> StepResponse {
        self.host.handle(request).await
    }

    /// Loads an importer's fields without stepping and without a cloud call.
    pub fn load(&self, request: LoadRequest) -> LoadResponse {
        self.host.load(request)
    }

    /// Serves stdin and stdout. The process has one mock cloud, which `MOCK_CLOUD_DIR` names.
    pub fn serve_stdio(&self, runtime: &tokio::runtime::Runtime) -> std::io::Result<()> {
        self.host.serve_stdio(runtime)
    }
}
