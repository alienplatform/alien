//! A release of a dependency chain of Kubernetes Containers, stepped by the stack executor the
//! way a deployment update steps it, against an in-process Kubernetes API that answers like a
//! real API server: a create of an existing name is 409, a read of a missing object is 404, and
//! an update of a workload rolls it out at once.
//!
//! The planner puts every Container of the release into its update flow in the first step, and
//! each dependent then waits (`DependencyNotReady`) until the Container it links is Running
//! again. The test checks what that wait costs: no request answered 404 or 409 is sent again,
//! and a dependent that is waiting writes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::{Json, Router};
use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::core::{DefaultPlatformServiceProvider, RunningResourcePolicy, StackExecutor};
use alien_core::{
    kubernetes_resource_name, ClientConfig, Container, ContainerCode, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, KubernetesClientConfig, ManagementPermissions,
    PermissionProfile, PermissionsConfig, Platform, ResourceLifecycle, ResourceSpec,
    ResourceStatus, Stack, StackSettings, StackState,
};

/// One request the fake API answered.
#[derive(Debug, Clone)]
struct Call {
    /// The executor step it was sent in.
    step: usize,
    method: Method,
    path: String,
    body: String,
    status: u16,
}

/// The objects of one namespace, stored as the API server would return them.
#[derive(Default)]
struct FakeApi {
    objects: BTreeMap<String, Value>,
    calls: Vec<Call>,
    step: usize,
    /// A workload path whose next update finds the object changed since the controller read it,
    /// as when the workload controller writes its status in between.
    changed_before_next_update: Option<String>,
    /// Workload paths whose pods crash-loop: no replica is ready until the pod template changes.
    crash_looping: BTreeSet<String>,
    /// Pods that are Ready for a moment after each start and then crash again, as a container
    /// without a readiness probe is: the workload reports ready, and the restart count of the
    /// ready pod grows with every pod list.
    restart_blip: bool,
    pod_lists: i64,
}

fn status(code: u16, reason: &str, name: &str) -> Value {
    json!({
        "kind": "Status",
        "apiVersion": "v1",
        "status": "Failure",
        "reason": reason,
        "code": code,
        "message": format!("{name}: {reason}"),
        "details": {"name": name},
    })
}

fn is_workload(path: &str) -> bool {
    path.starts_with("/apis/apps/v1/")
        && (path.contains("/deployments") || path.contains("/statefulsets"))
}

/// A workload whose pods for `generation` are all ready, as the workload controller reports it
/// once a rollout is done.
fn rolled_out(mut object: Value, generation: i64) -> Value {
    let replicas = object["spec"]["replicas"].as_i64().unwrap_or(1);
    object["metadata"]["generation"] = generation.into();
    object["status"] = json!({
        "observedGeneration": generation,
        "replicas": replicas,
        "updatedReplicas": replicas,
        "readyReplicas": replicas,
        "availableReplicas": replicas,
        "currentRevision": format!("rev-{generation}"),
        "updateRevision": format!("rev-{generation}"),
    });
    object
}

impl FakeApi {
    fn answer(&mut self, method: &Method, path: &str, body: &str) -> (u16, Value) {
        let name = path.rsplit('/').next().unwrap_or_default().to_string();
        match *method {
            // The health check lists pods and events: the fake cluster has none.
            Method::GET if self.restart_blip && path.ends_with("/pods") => {
                self.pod_lists += 1;
                (
                    200,
                    json!({"apiVersion": "v1", "kind": "PodList", "metadata": {}, "items": [{
                        "metadata": {"name": "platform-abc"},
                        "status": {
                            "conditions": [{"type": "Ready", "status": "True"}],
                            "containerStatuses": [{
                                "name": "platform", "ready": true, "image": "i", "imageID": "i",
                                "restartCount": self.pod_lists,
                            }],
                        },
                    }]}),
                )
            }
            Method::GET if path.ends_with("/pods") || path.ends_with("/events") => {
                let kind = if path.ends_with("/pods") {
                    "PodList"
                } else {
                    "EventList"
                };
                (
                    200,
                    json!({"apiVersion": "v1", "kind": kind, "metadata": {}, "items": []}),
                )
            }
            Method::GET => match self.objects.get(path) {
                Some(object) if self.crash_looping.contains(path) => {
                    let mut object = object.clone();
                    object["status"]["readyReplicas"] = 0.into();
                    object["status"]["availableReplicas"] = 0.into();
                    (200, object)
                }
                Some(object) => (200, object.clone()),
                None => (404, status(404, "NotFound", &name)),
            },
            Method::POST => {
                let mut object: Value = serde_json::from_str(body).expect("create body is JSON");
                let name = object["metadata"]["name"]
                    .as_str()
                    .expect("created object has a name")
                    .to_string();
                let key = format!("{path}/{name}");
                if self.objects.contains_key(&key) {
                    return (409, status(409, "AlreadyExists", &name));
                }
                object["metadata"]["resourceVersion"] = "1".into();
                if is_workload(path) {
                    object = rolled_out(object, 1);
                }
                self.objects.insert(key, object.clone());
                (201, object)
            }
            Method::PUT => {
                if self.changed_before_next_update.as_deref() == Some(path) {
                    self.changed_before_next_update = None;
                    let stored = self.objects.get_mut(path).expect("changed object exists");
                    let version = stored["metadata"]["resourceVersion"]
                        .as_str()
                        .expect("stored object has a resourceVersion")
                        .parse::<u64>()
                        .expect("numeric resourceVersion");
                    stored["metadata"]["resourceVersion"] = (version + 1).to_string().into();
                }
                let Some(stored) = self.objects.get(path) else {
                    return (404, status(404, "NotFound", &name));
                };
                let mut object: Value = serde_json::from_str(body).expect("update body is JSON");
                let stored_version = stored["metadata"]["resourceVersion"]
                    .as_str()
                    .expect("stored object has a resourceVersion")
                    .parse::<u64>()
                    .expect("numeric resourceVersion");
                if let Some(sent) = object["metadata"]["resourceVersion"].as_str() {
                    if sent != stored_version.to_string() {
                        return (409, status(409, "Conflict", &name));
                    }
                }
                let generation = stored["metadata"]["generation"].as_i64().unwrap_or(1);
                object["metadata"]["resourceVersion"] = (stored_version + 1).to_string().into();
                if is_workload(path) {
                    // As Kubernetes does: an identical pod template keeps the generation and
                    // creates no pods, so crash-looping pods keep crash-looping. A changed
                    // template is a new generation whose pods replace them.
                    if object["spec"]["template"] == stored["spec"]["template"] {
                        object = rolled_out(object, generation);
                    } else {
                        object = rolled_out(object, generation + 1);
                        self.crash_looping.remove(path);
                    }
                }
                self.objects.insert(path.to_string(), object.clone());
                (200, object)
            }
            Method::DELETE => match self.objects.remove(path) {
                Some(_) => (
                    200,
                    json!({"kind": "Status", "apiVersion": "v1", "status": "Success"}),
                ),
                None => (404, status(404, "NotFound", &name)),
            },
            _ => (405, status(405, "MethodNotAllowed", &name)),
        }
    }

    /// The name of the object a request reads or writes.
    fn object_name(call: &Call) -> String {
        if call.method == Method::POST {
            serde_json::from_str::<Value>(&call.body)
                .ok()
                .and_then(|body| body["metadata"]["name"].as_str().map(str::to_string))
                .unwrap_or_default()
        } else {
            call.path.rsplit('/').next().unwrap_or_default().to_string()
        }
    }
}

async fn serve(
    State(api): State<Arc<Mutex<FakeApi>>>,
    method: Method,
    uri: Uri,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let body = String::from_utf8(body.to_vec()).expect("request body is UTF-8");
    let path = uri.path().to_string();
    let mut api = api.lock().unwrap();
    let (code, response) = api.answer(&method, &path, &body);
    let step = api.step;
    api.calls.push(Call {
        step,
        method,
        path,
        body,
        status: code,
    });
    (StatusCode::from_u16(code).unwrap(), Json(response))
}

async fn start_fake_api() -> (String, Arc<Mutex<FakeApi>>) {
    let api = Arc::new(Mutex::new(FakeApi::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().fallback(serve).with_state(api.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, api)
}

/// The chain, each Container linking the one before it.
const CHAIN: [&str; 5] = ["setup", "auth", "platform", "worker", "portal"];

fn chain(release: &str) -> Vec<Container> {
    let mut containers: Vec<Container> = Vec::new();
    for id in CHAIN {
        let mut builder = Container::new(id.to_string())
            .code(ContainerCode::Image {
                image: format!("registry.example.com/{id}:{release}"),
            })
            .cpu(ResourceSpec {
                min: "100m".to_string(),
                desired: "500m".to_string(),
            })
            .memory(ResourceSpec {
                min: "128Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .port(8080)
            .permissions("default".to_string());
        if let Some(dependency) = containers.last() {
            builder = builder.link(dependency);
        }
        containers.push(builder.build());
    }
    containers
}

fn stack(release: &str) -> Stack {
    let mut stack = Stack::new("test".to_string());
    for container in chain(release) {
        stack = stack.add(container, ResourceLifecycle::Live);
    }
    let mut stack = stack.build();
    let mut profiles = IndexMap::new();
    profiles.insert("default".to_string(), PermissionProfile::new());
    stack.permissions = PermissionsConfig {
        profiles,
        management: ManagementPermissions::Auto,
    };
    stack
}

fn deployment_config() -> DeploymentConfig {
    let mut config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: "test-hash".to_string(),
            created_at: String::new(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    // A connected deployment: images are pulled with the deployment token.
    config.deployment_token = Some("deployment-token".to_string());
    config
}

fn client_config(server_url: &str) -> ClientConfig {
    ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::Manual {
        server_url: server_url.to_string(),
        certificate_authority_data: None,
        insecure_skip_tls_verify: Some(true),
        client_certificate_data: None,
        client_key_data: None,
        token: None,
        username: None,
        password: None,
        namespace: Some("test-ns".to_string()),
        additional_headers: Default::default(),
    }))
}

/// Steps `executor` until it is synced, recording each Container's status before every step.
async fn run_steps(
    executor: &StackExecutor,
    api: &Arc<Mutex<FakeApi>>,
    mut state: StackState,
    first_step: usize,
    statuses: &mut BTreeMap<usize, BTreeMap<String, ResourceStatus>>,
) -> (StackState, usize) {
    let mut step = first_step;
    while !executor.is_synced(&state) {
        assert!(step < first_step + 60, "the chain does not converge");
        api.lock().unwrap().step = step;
        statuses.insert(
            step,
            state
                .resources
                .iter()
                .map(|(id, resource)| (id.clone(), resource.status))
                .collect(),
        );
        state = executor
            .step(state)
            .await
            .expect("executor step")
            .next_state;
        for (id, resource) in &state.resources {
            assert!(
                !resource.status.is_terminal() || resource.status == ResourceStatus::Running,
                "'{id}' failed: {:?}",
                resource.error
            );
        }
        step += 1;
    }
    (state, step)
}

/// What one release of the chain did, from the first update step on.
struct Release {
    state: StackState,
    calls: Vec<Call>,
    objects: BTreeMap<String, Value>,
    /// Each Container's status at the start of each step.
    statuses: BTreeMap<usize, BTreeMap<String, ResourceStatus>>,
    steps: usize,
    elapsed: Duration,
}

fn deployment_path(id: &str) -> String {
    format!(
        "/apis/apps/v1/namespaces/test-ns/deployments/{}",
        kubernetes_resource_name("test", id)
    )
}

/// Installs release 1, then steps release 2 with the options a deployment update uses. When
/// `changed_before_update` names a Container, its workload changes between the controller's
/// read and its update.
async fn install_and_release(changed_before_update: Option<&str>) -> Release {
    let (url, api) = start_fake_api().await;
    let config = deployment_config();
    let mut statuses = BTreeMap::new();

    let installed = stack("1");
    let install = StackExecutor::builder(&installed, client_config(&url))
        .deployment_config(&config)
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("install executor");
    let (state, update_start) = run_steps(
        &install,
        &api,
        StackState::with_resource_prefix(Platform::Kubernetes, "test".to_string()),
        0,
        &mut statuses,
    )
    .await;
    api.lock().unwrap().changed_before_next_update = changed_before_update.map(deployment_path);

    let release = stack("2");
    let update = StackExecutor::builder(&release, client_config(&url))
        .deployment_config(&config)
        .resume_unchanged_failed_resources(true)
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .lifecycle_filter(vec![ResourceLifecycle::Live, ResourceLifecycle::Frozen])
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("update executor");
    let started = Instant::now();
    let (state, update_end) = run_steps(&update, &api, state, update_start, &mut statuses).await;
    let elapsed = started.elapsed();

    let api = api.lock().unwrap();
    assert_eq!(
        api.changed_before_next_update, None,
        "the changed workload was updated"
    );
    // Every Container runs release 2, and the API holds it.
    for id in CHAIN {
        assert_eq!(
            state.resources[id].status,
            ResourceStatus::Running,
            "'{id}'"
        );
        assert_eq!(
            api.objects[&deployment_path(id)]["spec"]["template"]["spec"]["containers"][0]["image"],
            format!("registry.example.com/{id}:2"),
            "'{id}' runs release 2"
        );
    }
    Release {
        state,
        calls: api
            .calls
            .iter()
            .filter(|call| call.step >= update_start)
            .cloned()
            .collect(),
        objects: api.objects.clone(),
        statuses: statuses.split_off(&update_start),
        steps: update_end - update_start,
        elapsed,
    }
}

/// Requests answered 404 or 409 that were sent again in the same step. Within a step each
/// Container steps once, so the same request twice there is a resend of the transport.
fn resends(calls: &[Call]) -> usize {
    let mut resends = 0;
    let mut answered = BTreeSet::new();
    for call in calls {
        let key = (
            call.step,
            call.method.clone(),
            call.path.clone(),
            call.body.clone(),
        );
        if answered.contains(&key) {
            resends += 1;
        }
        if matches!(call.status, 404 | 409) {
            answered.insert(key);
        }
    }
    resends
}

#[tokio::test]
async fn a_release_of_a_container_chain_waits_on_dependencies_without_api_calls() {
    let release = install_and_release(None).await;
    let calls = &release.calls;
    let resends = resends(calls);

    // A Container rolls its workload out only once the Container it links is Running, and in
    // the steps it waits it writes nothing: no Secret, Service or workload of it changes ahead
    // of its release.
    let mut waiting_writes = Vec::new();
    for (index, id) in CHAIN.iter().enumerate().skip(1) {
        let dependency = CHAIN[index - 1];
        let workload = kubernetes_resource_name("test", id);
        let names = [
            workload.clone(),
            format!("{workload}-registry"),
            format!("{workload}-env"),
        ];
        let rollout = calls
            .iter()
            .find(|call| call.method == Method::PUT && call.path == deployment_path(id))
            .unwrap_or_else(|| panic!("'{id}' is rolled out"));
        assert_eq!(
            release.statuses[&rollout.step][dependency],
            ResourceStatus::Running,
            "'{id}' rolls out after '{dependency}' is Running"
        );
        for call in calls {
            let waiting = release.statuses[&call.step][dependency] != ResourceStatus::Running;
            if waiting && call.method != Method::GET && names.contains(&FakeApi::object_name(call))
            {
                waiting_writes.push(format!(
                    "step {}: '{id}' {} {}",
                    call.step, call.method, call.path
                ));
            }
        }
    }

    let errors = calls
        .iter()
        .filter(|call| matches!(call.status, 404 | 409))
        .count();
    eprintln!(
        "update: {} executor steps, {} API requests, {} answered 404/409, {} resent, {} writes while waiting, {:.1}s",
        release.steps,
        calls.len(),
        errors,
        resends,
        waiting_writes.len(),
        release.elapsed.as_secs_f64()
    );
    assert_eq!(resends, 0, "a 404 or 409 answer was resent {resends} times");
    assert!(
        waiting_writes.is_empty(),
        "waiting dependents wrote: {waiting_writes:#?}"
    );
    // Each Container takes five steps: submit its rollout, see it complete on three polls in a
    // row (readiness must hold), publish its endpoints (Running). Its dependent submits in the
    // step after.
    assert_eq!(release.steps, 5 * CHAIN.len());
}

/// An update carries the `resourceVersion` the controller read. When the workload changed in
/// between, the API server answers 409, and sending the same update again gets 409 again. The
/// conflict goes to the executor, which runs the update step again in its next step: it reads
/// the workload afresh and the update goes through.
#[tokio::test]
async fn an_update_of_a_workload_changed_since_it_was_read_is_retried_by_the_executor() {
    let release = install_and_release(Some("platform")).await;
    let path = deployment_path("platform");
    let updates: Vec<&Call> = release
        .calls
        .iter()
        .filter(|call| call.method == Method::PUT && call.path == path)
        .collect();
    assert_eq!(
        updates.iter().map(|call| call.status).collect::<Vec<_>>(),
        [409, 200],
        "one stale update, then one fresh one"
    );
    assert_eq!(
        updates[1].step,
        updates[0].step + 1,
        "the executor retries in its next step"
    );
    let reads_before_retry = release
        .calls
        .iter()
        .filter(|call| {
            call.method == Method::GET && call.path == path && call.step == updates[1].step
        })
        .count();
    assert!(
        reads_before_retry >= 1,
        "the retry reads the workload again"
    );
    assert_eq!(resends(&release.calls), 0);
    assert_eq!(release.state.resources["platform"].retry_attempt, 0);
    assert_eq!(
        release.objects[&path]["metadata"]["generation"], 2,
        "rolled out once"
    );
    // One step more than a release without the conflict.
    assert_eq!(release.steps, 5 * CHAIN.len() + 1);
}

/// Installs release 1, marks `failed` as a Container whose readiness check failed (its refresh
/// failed), then steps `release_to_apply` the way a deployment update does. Returns the
/// requests sent after the failure and the final state.
async fn release_over_a_refresh_failed_container(
    failed: &str,
    release_to_apply: &str,
) -> (Vec<Call>, StackState, BTreeMap<String, Value>) {
    use crate::container::kubernetes::{KubernetesContainerController, KubernetesContainerState};
    use crate::core::state_utils::StackResourceStateExt;
    use crate::core::ResourceController;

    let (url, api) = start_fake_api().await;
    let config = deployment_config();
    let mut statuses = BTreeMap::new();
    let installed = stack("1");
    let install = StackExecutor::builder(&installed, client_config(&url))
        .deployment_config(&config)
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("install executor");
    let (mut state, first_step) = run_steps(
        &install,
        &api,
        StackState::with_resource_prefix(Platform::Kubernetes, "test".to_string()),
        0,
        &mut statuses,
    )
    .await;

    // The Ready step failed: the controller sits in RefreshFailed, as a saved state has it.
    let record = state.resources.get_mut(failed).unwrap();
    let mut controller = record
        .get_internal_controller()
        .unwrap()
        .unwrap()
        .as_any()
        .downcast_ref::<KubernetesContainerController>()
        .unwrap()
        .clone();
    controller.state = KubernetesContainerState::RefreshFailed;
    record.status = ResourceStatus::RefreshFailed;
    record.error = Some(crate::core::state_utils::saved_refresh_error(failed, true));
    record
        .set_internal_controller(Some(Box::new(controller)))
        .unwrap();
    assert_eq!(controller_status(record), ResourceStatus::RefreshFailed);

    let release = stack(release_to_apply);
    let update = StackExecutor::builder(&release, client_config(&url))
        .deployment_config(&config)
        .resume_unchanged_failed_resources(true)
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .lifecycle_filter(vec![ResourceLifecycle::Live, ResourceLifecycle::Frozen])
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("update executor");
    // A refresh-failed Container is not Running until its update completes, so the stepping
    // here does not treat that status as a failure.
    let mut state = state;
    for step in first_step..first_step + 60 {
        api.lock().unwrap().step = step;
        state = update.step(state).await.expect("executor step").next_state;
        if state
            .resources
            .values()
            .all(|resource| resource.status == ResourceStatus::Running)
        {
            break;
        }
    }
    let api = api.lock().unwrap();
    let calls = api
        .calls
        .iter()
        .filter(|call| call.step >= first_step)
        .cloned()
        .collect();
    (calls, state, api.objects.clone())
}

fn controller_status(record: &alien_core::StackResourceState) -> ResourceStatus {
    use crate::core::state_utils::StackResourceStateExt;
    record
        .get_internal_controller()
        .unwrap()
        .unwrap()
        .get_status()
}

/// A release with an unchanged config over a Container that failed its readiness check applies
/// the pod spec again, instead of re-running the check that read the same pods.
#[tokio::test]
async fn an_unchanged_release_applies_the_spec_of_a_container_that_failed_readiness() {
    let (calls, state, _) = release_over_a_refresh_failed_container("platform", "1").await;
    assert_eq!(state.resources["platform"].status, ResourceStatus::Running);
    assert!(
        calls
            .iter()
            .any(|call| call.method == Method::PUT && call.path == deployment_path("platform")),
        "the workload is updated: {calls:#?}"
    );
}

/// A changed config is still planned as an update of the failed Container.
#[tokio::test]
async fn a_changed_release_updates_a_container_that_failed_readiness() {
    let (calls, state, objects) = release_over_a_refresh_failed_container("platform", "2").await;
    assert_eq!(state.resources["platform"].status, ResourceStatus::Running);
    assert!(calls
        .iter()
        .any(|call| call.method == Method::PUT && call.path == deployment_path("platform")));
    assert_eq!(
        objects[&deployment_path("platform")]["spec"]["template"]["spec"]["containers"][0]["image"],
        "registry.example.com/platform:2"
    );
}

/// Installs release 1, makes the workload of `failed` crash-loop, and steps the executor until the
/// Container's own Ready step fails its retries and the record is RefreshFailed: the record the
/// executor writes, with its `lastFailedState`.
async fn crash_loop_until_refresh_failed(
    failed: &str,
) -> (String, Arc<Mutex<FakeApi>>, StackState, usize) {
    let path = deployment_path(failed);
    refresh_failed_after(failed, move |api| {
        api.crash_looping.insert(path);
    })
    .await
}

/// Like `crash_loop_until_refresh_failed`, with `fault` applied to the cluster after the install.
async fn refresh_failed_after(
    failed: &str,
    fault: impl FnOnce(&mut FakeApi),
) -> (String, Arc<Mutex<FakeApi>>, StackState, usize) {
    let (url, api) = start_fake_api().await;
    let config = deployment_config();
    let mut statuses = BTreeMap::new();
    let installed = stack("1");
    let install = StackExecutor::builder(&installed, client_config(&url))
        .deployment_config(&config)
        .running_resource_policy(RunningResourcePolicy::All)
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("install executor");
    let (mut state, mut step) = run_steps(
        &install,
        &api,
        StackState::with_resource_prefix(Platform::Kubernetes, "test".to_string()),
        0,
        &mut statuses,
    )
    .await;
    fault(&mut api.lock().unwrap());

    let observe = StackExecutor::builder(&installed, client_config(&url))
        .deployment_config(&config)
        .running_resource_policy(RunningResourcePolicy::All)
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("observing executor");
    let limit = step + 200;
    while state.resources[failed].status != ResourceStatus::RefreshFailed {
        assert!(step < limit, "the faulty container never fails its check");
        api.lock().unwrap().step = step;
        state = observe.step(state).await.expect("executor step").next_state;
        step += 1;
    }
    assert!(
        state.resources[failed].last_failed_state.is_some(),
        "the executor saves the controller that failed"
    );
    (url, api, state, step)
}

/// Steps `executor` until `platform` is Running or has failed an update, and returns the state.
async fn step_until_settled(
    executor: &StackExecutor,
    api: &Arc<Mutex<FakeApi>>,
    mut state: StackState,
    first_step: usize,
) -> (StackState, usize) {
    let mut step = first_step;
    while step < first_step + 80 {
        api.lock().unwrap().step = step;
        state = executor
            .step(state)
            .await
            .expect("executor step")
            .next_state;
        step += 1;
        if matches!(
            state.resources["platform"].status,
            ResourceStatus::Running | ResourceStatus::UpdateFailed
        ) {
            break;
        }
    }
    (state, step - first_step)
}

fn put_calls(api: &Arc<Mutex<FakeApi>>, first_step: usize) -> usize {
    api.lock()
        .unwrap()
        .calls
        .iter()
        .filter(|call| {
            call.step >= first_step
                && call.method == Method::PUT
                && call.path == deployment_path("platform")
        })
        .count()
}

/// A retry of a Container whose real Ready step failed applies the CURRENT pod spec again. The
/// spec has not changed, so no pods roll (the fake API, like Kubernetes, creates none for an
/// identical template) and the crash loop continues: the update waits only the short bound for
/// readiness and fails with the not-ready error, instead of the 30-minute rollout limit.
#[tokio::test]
async fn a_retry_of_a_crash_looping_container_applies_the_spec_and_fails_fast_when_nothing_rolls() {
    use crate::container::kubernetes::{KubernetesContainerController, KubernetesContainerState};
    use crate::core::state_utils::{StackResourceStateExt, StackStateExt};

    let (url, api, mut state, first_step) = crash_loop_until_refresh_failed("platform").await;
    assert_eq!(
        state.resources["platform"].error.as_ref().unwrap().code,
        "WORKLOAD_NOT_READY"
    );
    assert!(state
        .retry_failed()
        .unwrap()
        .contains(&"platform".to_string()));
    let resumed = state.resources["platform"]
        .get_internal_controller()
        .unwrap()
        .unwrap();
    assert_eq!(
        resumed
            .as_any()
            .downcast_ref::<KubernetesContainerController>()
            .unwrap()
            .state,
        KubernetesContainerState::UpdateStart
    );

    let config = deployment_config();
    let observe = StackExecutor::builder(&stack("1"), client_config(&url))
        .deployment_config(&config)
        .running_resource_policy(RunningResourcePolicy::All)
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("executor");
    let (state, steps) = step_until_settled(&observe, &api, state, first_step).await;
    assert_eq!(put_calls(&api, first_step), 1, "the spec is applied once");
    assert_eq!(
        state.resources["platform"].status,
        ResourceStatus::UpdateFailed
    );
    assert_eq!(
        state.resources["platform"].error.as_ref().unwrap().code,
        "WORKLOAD_NOT_READY"
    );
    // The short bound (12 polls) plus the steps that start, apply and fail the update; the
    // rollout limit would be 360 polls.
    assert!(
        steps < 40,
        "the wait is bounded by the short limit: {steps} steps"
    );
}

/// An unchanged release over the same record goes through `resume_unchanged_failed_resources`:
/// the same short bound applies.
#[tokio::test]
async fn an_unchanged_release_over_a_crash_looping_container_fails_fast_when_nothing_rolls() {
    let (url, api, state, first_step) = crash_loop_until_refresh_failed("platform").await;
    let config = deployment_config();
    let update = StackExecutor::builder(&stack("1"), client_config(&url))
        .deployment_config(&config)
        .resume_unchanged_failed_resources(true)
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .lifecycle_filter(vec![ResourceLifecycle::Live, ResourceLifecycle::Frozen])
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("update executor");
    let (state, steps) = step_until_settled(&update, &api, state, first_step).await;
    assert_eq!(put_calls(&api, first_step), 1);
    assert_eq!(
        state.resources["platform"].status,
        ResourceStatus::UpdateFailed
    );
    assert_eq!(
        state.resources["platform"].error.as_ref().unwrap().code,
        "WORKLOAD_NOT_READY"
    );
    assert!(
        steps < 40,
        "the wait is bounded by the short limit: {steps} steps"
    );
}

/// A re-apply whose pod template changed (an operator change, or a changed config) rolls new
/// pods, and the container heals.
#[tokio::test]
async fn a_changed_release_over_a_crash_looping_container_rolls_new_pods_and_heals() {
    let (url, api, state, first_step) = crash_loop_until_refresh_failed("platform").await;
    let config = deployment_config();
    let update = StackExecutor::builder(&stack("2"), client_config(&url))
        .deployment_config(&config)
        .resume_unchanged_failed_resources(true)
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .lifecycle_filter(vec![ResourceLifecycle::Live, ResourceLifecycle::Frozen])
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("update executor");
    let (state, _) = step_until_settled(&update, &api, state, first_step).await;
    assert_eq!(state.resources["platform"].status, ResourceStatus::Running);
    assert_eq!(put_calls(&api, first_step), 1);
}

/// A Deployment deleted out of band is not a readiness problem: the refresh failure is not the
/// not-ready error, and a retry does not apply the spec (which would silently recreate nothing
/// and hide the loss). It runs the check again and reports the missing Deployment as before.
#[tokio::test]
async fn a_retry_over_a_deleted_deployment_is_not_applied_and_reports_it_again() {
    use crate::container::kubernetes::{KubernetesContainerController, KubernetesContainerState};
    use crate::core::state_utils::{StackResourceStateExt, StackStateExt};

    let path = deployment_path("platform");
    let (url, api, mut state, first_step) = refresh_failed_after("platform", |api| {
        api.objects.remove(&path);
    })
    .await;
    let error = state.resources["platform"].error.clone().unwrap();
    assert_ne!(error.code, "WORKLOAD_NOT_READY", "{error:?}");

    assert!(state
        .retry_failed()
        .unwrap()
        .contains(&"platform".to_string()));
    let resumed = state.resources["platform"]
        .get_internal_controller()
        .unwrap()
        .unwrap();
    assert_ne!(
        resumed
            .as_any()
            .downcast_ref::<KubernetesContainerController>()
            .unwrap()
            .state,
        KubernetesContainerState::UpdateStart
    );

    let config = deployment_config();
    let observe = StackExecutor::builder(&stack("1"), client_config(&url))
        .deployment_config(&config)
        .running_resource_policy(RunningResourcePolicy::All)
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("executor");
    let mut step = first_step;
    while state.resources["platform"].status != ResourceStatus::RefreshFailed {
        assert!(
            step < first_step + 200,
            "the missing Deployment is reported again"
        );
        api.lock().unwrap().step = step;
        state = observe.step(state).await.expect("executor step").next_state;
        step += 1;
    }
    assert_eq!(put_calls(&api, first_step), 0, "nothing is applied");
    assert_ne!(
        state.resources["platform"].error.as_ref().unwrap().code,
        "WORKLOAD_NOT_READY"
    );
}

/// A pod that is ready for part of each crash cycle still fails the refresh: a ready check in
/// between does not reset the count of not-ready observations (three in ten minutes). Without
/// the bound it would stay Running forever.
#[tokio::test]
async fn a_flapping_container_reaches_refresh_failed() {
    let (url, api) = start_fake_api().await;
    let config = deployment_config();
    let mut statuses = BTreeMap::new();
    let installed = stack("1");
    let install = StackExecutor::builder(&installed, client_config(&url))
        .deployment_config(&config)
        .running_resource_policy(RunningResourcePolicy::All)
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("install executor");
    let (mut state, first_step) = run_steps(
        &install,
        &api,
        StackState::with_resource_prefix(Platform::Kubernetes, "test".to_string()),
        0,
        &mut statuses,
    )
    .await;

    let path = deployment_path("platform");
    let mut step = first_step;
    while state.resources["platform"].status != ResourceStatus::RefreshFailed {
        assert!(
            step < first_step + 60,
            "a flapping container stays Running: {:?}",
            state.resources["platform"].status
        );
        {
            // Not ready on every other step, ready in between.
            let mut api = api.lock().unwrap();
            api.step = step;
            if step % 2 == 0 {
                api.crash_looping.insert(path.clone());
            } else {
                api.crash_looping.remove(&path);
            }
        }
        state = install.step(state).await.expect("executor step").next_state;
        step += 1;
    }
    assert_eq!(
        state.resources["platform"].error.as_ref().unwrap().code,
        "WORKLOAD_NOT_READY"
    );
}

/// A pod that is Ready for a moment after each restart makes the workload look ready on every
/// poll. The rollout never completes, because the restart count of the ready pod grows: the
/// update fails at its bound with the not-ready error instead of reporting a finished rollout.
#[tokio::test]
async fn a_crash_looping_pod_that_is_briefly_ready_on_each_restart_never_completes_the_rollout() {
    let (url, api, state, first_step) = crash_loop_until_refresh_failed("platform").await;
    {
        let mut api = api.lock().unwrap();
        api.crash_looping.clear();
        api.restart_blip = true;
    }
    let config = deployment_config();
    let update = StackExecutor::builder(&stack("1"), client_config(&url))
        .deployment_config(&config)
        .resume_unchanged_failed_resources(true)
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .lifecycle_filter(vec![ResourceLifecycle::Live, ResourceLifecycle::Frozen])
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .expect("update executor");
    let (state, steps) = step_until_settled(&update, &api, state, first_step).await;
    assert_eq!(
        state.resources["platform"].status,
        ResourceStatus::UpdateFailed
    );
    assert_eq!(
        state.resources["platform"].error.as_ref().unwrap().code,
        "WORKLOAD_NOT_READY"
    );
    assert!(steps < 40, "the short bound applies: {steps} steps");
}
