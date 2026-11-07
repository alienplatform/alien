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
    /// GET the container app, or `None` when it doesn't exist.
    async fn find(&self, name: &str) -> Result<Option<Value>>;
    /// PUT a new container app `app`.
    async fn create(&self, name: &str, app: &Value) -> Result<()>;
    /// The install's Container Apps environment: its resource id and location.
    async fn environment(&self) -> Result<(String, String)>;
}

/// The Container App secret holding the registry password.
const REGISTRY_SECRET: &str = "registry-password";

/// The container app the verifier creates for `spec` in `environment` at `location`: its
/// signed template and identity, ingress only when the signed stack gives the workload a public
/// endpoint, Dapr when it has triggers or commands, and the image's registry credentials when
/// the request carried them.
pub fn create_body(
    spec: &FunctionSpec,
    environment: &str,
    location: &str,
    tags: &BTreeMap<String, String>,
) -> Value {
    let mut app = app_body(spec, "main");
    app["location"] = json!(location);
    app["tags"] = json!(tags);
    let properties = &mut app["properties"];
    properties["managedEnvironmentId"] = json!(environment);
    let mut configuration = json!({"activeRevisionsMode": "Single"});
    if spec.public {
        configuration["ingress"] = json!({
            "external": true,
            "targetPort": 8080,
            "transport": "auto",
            "traffic": [{"latestRevision": true, "weight": 100}],
        });
    }
    if spec.dapr {
        configuration["dapr"] = json!({
            "enabled": true,
            "appId": spec.name,
            "appPort": 8080,
            "appProtocol": "http",
        });
    }
    if let Some(registry) = &spec.registry {
        configuration["secrets"] = json!([{"name": REGISTRY_SECRET, "value": registry.password}]);
        configuration["registries"] = json!([{
            "server": registry.server,
            "username": registry.username,
            "passwordSecretRef": REGISTRY_SECRET,
        }]);
    }
    properties["configuration"] = configuration;
    app
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

/// Write `spec` and read it back, creating the app with `tags` in the install's environment when
/// it doesn't exist: setup renders no workload, and management may not create one on a protected
/// install.
pub async fn apply(
    store: &dyn AppStore,
    spec: &FunctionSpec,
    tags: &BTreeMap<String, String>,
) -> Result<Value> {
    if store.find(&spec.name).await?.is_none() {
        let (environment, location) = store.environment().await?;
        store
            .create(
                &spec.name,
                &create_body(spec, &environment, &location, tags),
            )
            .await?;
    } else {
        store.patch(&spec.name, &app_body(spec, "main")).await?;
    }
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
    // and no other user-assigned one. Azure resource ids are case-insensitive, and Azure
    // answers in its own casing.
    let identity = &app["identity"];
    let identities: Vec<&String> = identity["userAssignedIdentities"]
        .as_object()
        .map(|identities| identities.keys().collect())
        .unwrap_or_default();
    let signed = matches!(identities.as_slice(), [only] if only.eq_ignore_ascii_case(&spec.run_as));
    if identity["type"].as_str() != Some("UserAssigned") || !signed {
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
            dapr: false,
            registry: None,
            reserved_concurrency: None,
            invoke_grants: Vec::new(),
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

    /// Azure answers with resource ids in its own casing (`resourcegroups`); an id is the same
    /// resource whatever its case.
    #[test]
    fn the_identity_reads_back_in_azures_casing() {
        let mut back = app(&spec());
        back["identity"]["userAssignedIdentities"] = json!({
            IDENTITY.replace("resourceGroups", "resourcegroups"): {"clientId": "c", "principalId": "p"}
        });
        check_read_back(&spec(), &back).unwrap();
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
        app: Mutex<Option<Value>>,
        created: Mutex<Option<Value>>,
    }

    impl FakeApps {
        fn holding(app: Value) -> Self {
            Self {
                app: Mutex::new(Some(app)),
                created: Mutex::new(None),
            }
        }

        fn empty() -> Self {
            Self {
                app: Mutex::new(None),
                created: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl AppStore for FakeApps {
        async fn get(&self, _name: &str) -> Result<Value> {
            Ok(self.app.lock().unwrap().clone().unwrap())
        }
        async fn patch(&self, _name: &str, body: &Value) -> Result<()> {
            let mut held = self.app.lock().unwrap();
            let app = held.as_mut().unwrap();
            app["identity"] = body["identity"].clone();
            app["properties"]["template"] = body["properties"]["template"].clone();
            Ok(())
        }
        async fn find(&self, _name: &str) -> Result<Option<Value>> {
            Ok(self.app.lock().unwrap().clone())
        }
        async fn create(&self, _name: &str, body: &Value) -> Result<()> {
            *self.created.lock().unwrap() = Some(body.clone());
            let mut created = app(&spec());
            created["identity"] = body["identity"].clone();
            created["properties"]["template"] = body["properties"]["template"].clone();
            *self.app.lock().unwrap() = Some(created);
            Ok(())
        }
        async fn environment(&self) -> Result<(String, String)> {
            Ok((ENVIRONMENT.to_string(), "eastus".to_string()))
        }
    }

    const ENVIRONMENT: &str = "/subscriptions/s/resourceGroups/acme-prod/providers/Microsoft.App/managedEnvironments/acme-prod-env";

    fn tags() -> BTreeMap<String, String> {
        BTreeMap::from([("deployment".to_string(), "acme-prod".to_string())])
    }

    #[tokio::test]
    async fn apply_replaces_the_template_and_identity() {
        let mut drifted = app(&spec());
        drifted["properties"]["template"]["initContainers"] =
            json!([{"name": "x", "image": "evil"}]);
        drifted["identity"]["type"] = json!("SystemAssigned,UserAssigned");
        let apps = FakeApps::holding(drifted);
        apply(&apps, &spec(), &tags()).await.unwrap();
        check_read_back(&spec(), apps.app.lock().unwrap().as_ref().unwrap()).unwrap();
        assert!(
            apps.created.lock().unwrap().is_none(),
            "an existing app is patched"
        );
    }

    #[tokio::test]
    async fn a_missing_private_app_is_created_in_the_install_environment_without_ingress() {
        // Setup renders no workload, and management may not create one on a protected install.
        let private = FunctionSpec {
            registry: Some(crate::plan::RegistryCredentials {
                server: "manager.example.com".to_string(),
                username: "deployment".to_string(),
                password: "token".to_string(),
            }),
            ..spec()
        };
        let apps = FakeApps::empty();
        apply(&apps, &private, &tags()).await.unwrap();
        let created = apps.created.lock().unwrap().clone().unwrap();
        assert_eq!(created["location"], json!("eastus"));
        assert_eq!(created["tags"], json!({"deployment": "acme-prod"}));
        assert_eq!(
            created["properties"]["managedEnvironmentId"],
            json!(ENVIRONMENT)
        );
        let configuration = &created["properties"]["configuration"];
        assert!(configuration.get("ingress").is_none(), "{configuration}");
        assert!(configuration.get("dapr").is_none(), "{configuration}");
        assert_eq!(
            configuration["registries"],
            json!([{"server": "manager.example.com", "username": "deployment", "passwordSecretRef": "registry-password"}])
        );
        assert_eq!(
            configuration["secrets"],
            json!([{"name": "registry-password", "value": "token"}])
        );
        assert_eq!(created["identity"], app_body(&private, "main")["identity"]);
        check_read_back(&private, apps.app.lock().unwrap().as_ref().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn a_missing_public_app_with_triggers_gets_ingress_and_dapr() {
        let public = FunctionSpec {
            public: true,
            dapr: true,
            ..spec()
        };
        let apps = FakeApps::empty();
        apply(&apps, &public, &tags()).await.unwrap();
        let created = apps.created.lock().unwrap().clone().unwrap();
        let configuration = &created["properties"]["configuration"];
        assert_eq!(configuration["ingress"]["external"], json!(true));
        assert_eq!(configuration["ingress"]["targetPort"], json!(8080));
        assert_eq!(
            configuration["dapr"],
            json!({"enabled": true, "appId": public.name, "appPort": 8080, "appProtocol": "http"})
        );
        assert!(configuration.get("registries").is_none());
    }
}
