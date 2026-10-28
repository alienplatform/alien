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
            Method::GET => match self.objects.get(path) {
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
                    object = rolled_out(object, generation + 1);
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
    // Each Container takes three steps: submit its rollout, see it complete, publish its
    // endpoints (Running). Its dependent submits in the step after.
    assert_eq!(release.steps, 3 * CHAIN.len());
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
    assert_eq!(release.steps, 3 * CHAIN.len() + 1);
}
