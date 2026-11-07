//! Write a planned function as an Azure Container App and check it reads
//! back the same.
//!
//! The verifier sends the whole template and identity it built, so an init
//! container, a volume, a command, a secret-backed env value or another
//! identity set behind its back is replaced, and the read-back refuses any
//! field it doesn't know.

use std::collections::BTreeMap;
use std::time::Duration;

use alien_error::AlienError;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::{ErrorData, Result};
use crate::plan::FunctionSpec;

/// The Container Apps calls the verifier makes.
#[async_trait]
pub trait AppStore: Send + Sync {
    /// GET the container app, exactly as returned.
    async fn get(&self, name: &str) -> Result<Value>;
    /// PATCH the container app with `app`.
    async fn patch(&self, name: &str, app: &Value) -> Result<()>;
}

/// The CPU (cores) and memory a Container App gets for `memory_mb`:
/// Container Apps allows only 0.5Gi per 0.25 cores, so memory is rounded up
/// to the next 0.5Gi and CPU follows it.
pub fn resources(memory_mb: u32) -> (f64, String) {
    let halves = memory_mb.div_ceil(512).clamp(1, 16);
    let cpu = f64::from(halves) * 0.25;
    let memory = format!("{}Gi", f64::from(halves) * 0.5);
    (cpu, memory)
}

/// The container app body the verifier writes for `spec`.
pub fn app_body(spec: &FunctionSpec, container_name: &str) -> Value {
    let env: Vec<Value> = spec
        .env
        .iter()
        .map(|(name, value)| json!({"name": name, "value": value}))
        .collect();
    let (cpu, memory) = resources(spec.memory_mb);
    json!({
        "identity": {
            "type": "UserAssigned",
            "userAssignedIdentities": {spec.run_as.clone(): {}},
        },
        "properties": {
            "template": {
                "containers": [{
                    "name": container_name,
                    "image": spec.image_uri,
                    "env": env,
                    "resources": {"cpu": cpu, "memory": memory},
                }],
                "initContainers": [],
                "volumes": [],
            },
        },
    })
}

const UPDATE_WAIT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_secs(3);

/// Write `spec` and read it back. The app must already exist: setup creates
/// the apps the verifier owns.
pub async fn apply(store: &dyn AppStore, spec: &FunctionSpec) -> Result<Value> {
    store.patch(&spec.name, &app_body(spec, "main")).await?;
    let deadline = tokio::time::Instant::now() + UPDATE_WAIT;
    loop {
        let app = store.get(&spec.name).await?;
        let state = app["properties"]["provisioningState"]
            .as_str()
            .unwrap_or_default();
        if state != "InProgress" && state != "Updating" {
            if state == "Failed" {
                return Err(AlienError::new(ErrorData::CloudCallFailed {
                    operation: format!("updating container app {}", spec.name),
                }));
            }
            check_read_back(spec, &app)?;
            return Ok(app);
        }
        if tokio::time::Instant::now() > deadline {
            return Err(AlienError::new(ErrorData::CloudCallFailed {
                operation: format!("waiting for container app {} to finish updating", spec.name),
            }));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Template keys the verifier understands.
const KNOWN_TEMPLATE: &[&str] = &[
    "revisionSuffix",
    "terminationGracePeriodSeconds",
    "containers",
    "initContainers",
    "scale",
    "volumes",
    "serviceBinds",
];
/// Container keys the verifier understands.
const KNOWN_CONTAINER: &[&str] = &[
    "name",
    "image",
    "imageType",
    "command",
    "args",
    "env",
    "resources",
    "probes",
    "volumeMounts",
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

/// Check that a container app is the one `spec` describes.
pub fn check_read_back(spec: &FunctionSpec, app: &Value) -> Result<()> {
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
    let template = &app["properties"]["template"];
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
    let (cpu, memory) = resources(spec.memory_mb);
    if container["resources"]["cpu"].as_f64() != Some(cpu)
        || container["resources"]["memory"].as_str() != Some(memory.as_str())
    {
        return Err(mismatch(format!(
            "resources are {}",
            container["resources"]
        )));
    }
    let mut env = BTreeMap::new();
    for item in container["env"].as_array().cloned().unwrap_or_default() {
        let name = text(&item["name"]);
        // A value read from a secret is code the release didn't sign.
        if !is_empty(&item["secretRef"]) {
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
    for field in ["command", "args", "volumeMounts"] {
        if !is_empty(&container[field]) {
            return Err(mismatch(format!("its container sets {field}")));
        }
    }
    for field in ["initContainers", "volumes", "serviceBinds"] {
        if !is_empty(&template[field]) {
            return Err(mismatch(format!("its template has {field}")));
        }
    }
    // It runs as exactly the signed identity: no system-assigned identity,
    // and no other user-assigned one.
    let identity = &app["identity"];
    let identities: Vec<&String> = identity["userAssignedIdentities"]
        .as_object()
        .map(|identities| identities.keys().collect())
        .unwrap_or_default();
    if identity["type"].as_str() != Some("UserAssigned") || identities != [&spec.run_as] {
        return Err(mismatch(format!("it runs as {}", identity)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::DIGEST;
    use std::sync::Mutex;

    const IDENTITY: &str = "/subscriptions/s/resourceGroups/acme/providers/Microsoft.ManagedIdentity/userAssignedIdentities/acme-execution";

    fn spec() -> FunctionSpec {
        FunctionSpec {
            workload: "api".into(),
            vpc: None,
            name: "acme-fn-api".into(),
            image_uri: format!("acme.azurecr.io/api@{DIGEST}"),
            digest: DIGEST.into(),
            env: BTreeMap::from([("MODE".into(), "signed".into())]),
            memory_mb: 600,
            timeout_seconds: 30,
            architecture: "x86_64".into(),
            run_as: IDENTITY.into(),
            public: false,
        }
    }

    /// A container app as Azure returns it once the verifier's body is applied.
    fn app(spec: &FunctionSpec) -> Value {
        let mut app = app_body(spec, "main");
        app["identity"]["userAssignedIdentities"][IDENTITY] =
            json!({"principalId": "p", "clientId": "c"});
        app["properties"]["provisioningState"] = json!("Succeeded");
        app["properties"]["template"]["scale"] = json!({"minReplicas": 0, "maxReplicas": 10});
        app["properties"]["template"]["revisionSuffix"] = json!("");
        app["properties"]["template"]["containers"][0]["resources"]["ephemeralStorage"] =
            json!("2Gi");
        app["properties"]["template"]["containers"][0]["probes"] = json!([]);
        app
    }

    #[test]
    fn resources_follow_the_allowed_pairs() {
        assert_eq!(resources(128), (0.25, "0.5Gi".to_string()));
        assert_eq!(resources(512), (0.25, "0.5Gi".to_string()));
        assert_eq!(resources(600), (0.5, "1Gi".to_string()));
        assert_eq!(resources(4096), (2.0, "4Gi".to_string()));
        assert_eq!(resources(100_000), (4.0, "8Gi".to_string()));
    }

    #[test]
    fn the_app_as_built_reads_back() {
        check_read_back(&spec(), &app(&spec())).unwrap();
    }

    #[test]
    fn an_app_changed_behind_the_verifier_is_caught() {
        let reason = |change: &dyn Fn(&mut Value)| {
            let mut changed = app(&spec());
            change(&mut changed);
            check_read_back(&spec(), &changed)
                .expect_err("must not match")
                .to_string()
        };
        let in_template = |change: &dyn Fn(&mut Value)| {
            reason(&|app: &mut Value| change(&mut app["properties"]["template"]))
        };
        assert!(
            in_template(&|t| t["containers"][0]["env"][0]["value"] = json!("attacker"))
                .contains("env differs")
        );
        assert!(in_template(
            &|t| t["containers"][0]["env"][0] = json!({"name": "MODE", "secretRef": "stolen"})
        )
        .contains("read from a secret"));
        assert!(
            in_template(&|t| t["initContainers"] = json!([{"name": "x", "image": "evil"}]))
                .contains("initContainers")
        );
        assert!(
            in_template(&|t| t["containers"][0]["command"] = json!(["/bin/sh"]))
                .contains("sets command")
        );
        assert!(
            in_template(&|t| t["volumes"] = json!([{"name": "v", "storageType": "AzureFile"}]))
                .contains("volumes")
        );
        assert!(
            reason(&|a| a["identity"]["type"] = json!("SystemAssigned,UserAssigned"))
                .contains("runs as")
        );
        assert!(reason(
            &|a| a["identity"]["userAssignedIdentities"]["/subscriptions/s/other"] = json!({})
        )
        .contains("runs as"));
        assert!(
            in_template(&|t| t["containers"][0]["image"] = json!("acme.azurecr.io/api:latest"))
                .contains("runs image")
        );
    }

    #[test]
    fn a_field_the_verifier_does_not_know_refuses_the_app() {
        let mut app = app(&spec());
        app["properties"]["template"]["containers"][0]["sidecarMagic"] = json!({"enabled": true});
        let error = check_read_back(&spec(), &app).unwrap_err().to_string();
        assert!(
            error.contains("sidecarMagic, which the verifier doesn't know"),
            "{error}"
        );
    }

    struct FakeApps {
        app: Mutex<Value>,
    }

    #[async_trait]
    impl AppStore for FakeApps {
        async fn get(&self, _name: &str) -> Result<Value> {
            Ok(self.app.lock().unwrap().clone())
        }
        async fn patch(&self, _name: &str, body: &Value) -> Result<()> {
            let mut app = self.app.lock().unwrap();
            app["identity"] = body["identity"].clone();
            app["properties"]["template"] = body["properties"]["template"].clone();
            Ok(())
        }
    }

    #[tokio::test]
    async fn apply_replaces_the_template_and_identity() {
        let mut drifted = app(&spec());
        drifted["properties"]["template"]["initContainers"] =
            json!([{"name": "x", "image": "evil"}]);
        drifted["identity"]["type"] = json!("SystemAssigned,UserAssigned");
        let apps = FakeApps {
            app: Mutex::new(drifted),
        };
        apply(&apps, &spec()).await.unwrap();
        check_read_back(&spec(), &apps.app.lock().unwrap()).unwrap();
    }
}
