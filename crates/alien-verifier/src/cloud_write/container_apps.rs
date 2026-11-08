//! The Container Apps table: every field of the app a worker controller writes, and its rule.

use std::collections::BTreeMap;
use std::time::Duration;

use alien_azure_clients::models::container_apps::ContainerApp;
use alien_error::AlienError;
use serde_json::Value;

use super::env::check_env;
use super::{check_target, refused, CloudWrite, Facts};
use crate::container_apps::AppStore;
use crate::error::{ErrorData, Result};
use crate::fields::{check, check_whole, empty, free, is_empty, parse_strict, signed, Row};
use crate::function::StillNewest;

/// Check `write`, a Container Apps write for `facts.workload`, against the table.
/// `environment` is the Container Apps environment setup runs the install's workloads in.
pub fn check_write(facts: &Facts<'_>, write: &CloudWrite, environment: Option<&str>) -> Result<()> {
    let (operation, named, app) = match write {
        CloudWrite::CreateContainerApp { name, app } => ("CreateContainerApp", name, app),
        CloudWrite::UpdateContainerApp { name, app } => ("UpdateContainerApp", name, app),
        other => {
            return Err(refused(format!(
                "Container Apps takes no {} write",
                other.operation()
            )))
        }
    };
    parse_strict::<ContainerApp>(operation, app)?;
    check_target(facts, named, operation)?;
    check_secrets(operation, app)?;
    match write {
        CloudWrite::CreateContainerApp { .. } => {
            check_whole(operation, app, &app_rows(facts, environment))
        }
        // A PATCH: what it leaves out stays as the verifier last set it.
        _ => check(operation, app, &app_rows(facts, environment)),
    }
}

/// Every secret is a registry's pull credential, and nothing else reads one.
fn check_secrets(operation: &str, app: &Value) -> Result<()> {
    let configuration = &app["properties"]["configuration"];
    let pulls: Vec<&str> = configuration["registries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|registry| registry["passwordSecretRef"].as_str())
        .collect();
    for secret in configuration["secrets"].as_array().into_iter().flatten() {
        let name = secret["name"].as_str().unwrap_or_default();
        if !pulls.contains(&name) {
            return Err(refused(format!(
                "{operation} field /properties/configuration/secrets must match the signed release: secret {name} is no registry's pull credential"
            )));
        }
    }
    Ok(())
}

fn is_public(facts: &Facts<'_>) -> std::result::Result<bool, String> {
    crate::plan::signed_worker(
        &facts.accepted.release.body,
        facts.place,
        &facts.workload.name,
    )
    .map(|worker| worker.is_some_and(|worker| !worker.public_endpoints.is_empty()))
    .map_err(|error| error.to_string())
}

fn app_rows<'a>(facts: &'a Facts<'a>, environment: Option<&'a str>) -> Vec<Row<'a>> {
    vec![
        // It runs as the workload's identity and no other: the install's other identities (the
        // verifier's, setup's) carry grants a worker must never have.
        signed("/identity", move |value| {
            if value["type"].as_str() != Some("UserAssigned") {
                return Err(format!("its identity type is {}", value["type"]));
            }
            let profile = facts
                .workload
                .identity
                .as_deref()
                .ok_or_else(|| "the workload names no permission profile".to_string())?;
            let own = facts.place.run_as(profile);
            // A linked service account would add an identity here; the signed release gives a
            // protected worker its profile's identity only, so any other is refused.
            let ids: Vec<&String> = value["userAssignedIdentities"]
                .as_object()
                .map(|ids| ids.keys().collect())
                .unwrap_or_default();
            if !ids.iter().any(|id| id.eq_ignore_ascii_case(&own)) {
                return Err(format!("it doesn't run as {own}"));
            }
            match ids.iter().find(|id| !id.eq_ignore_ascii_case(&own)) {
                Some(other) => Err(format!(
                    "it also runs as {other}, which isn't an identity the workload's permission profile names"
                )),
                None => Ok(()),
            }
        }),
        signed("/properties/managedEnvironmentId", move |value| {
            let expected = environment.ok_or_else(|| {
                "setup named no Container Apps environment for the install's workloads".to_string()
            })?;
            value
                .as_str()
                .is_some_and(|id| id.eq_ignore_ascii_case(expected))
                .then_some(())
                .ok_or_else(|| format!("it is {value}, not {expected}"))
        }),
        signed("/properties/configuration/activeRevisionsMode", |value| {
            (value == "Single")
                .then_some(())
                .ok_or_else(|| format!("it is {value}: one revision serves"))
        }),
        signed("/properties/configuration/ingress", move |value| {
            let public = is_public(facts)?;
            if is_empty(value) {
                return (!public)
                    .then_some(())
                    .ok_or_else(|| "a public worker has no ingress".to_string());
            }
            if value["external"].as_bool().unwrap_or(false) != public {
                return Err(format!(
                    "its ingress external is {}, the worker is {}",
                    value["external"],
                    if public { "public" } else { "private" }
                ));
            }
            for item in value["traffic"].as_array().into_iter().flatten() {
                if item["latestRevision"].as_bool() != Some(true) {
                    return Err(format!("traffic goes to {item}, not the latest revision"));
                }
            }
            if !is_empty(&value["ipSecurityRestrictions"])
                || !is_empty(&value["additionalPortMappings"])
            {
                return Err(
                    "its ingress has restrictions or ports the release didn't sign".to_string(),
                );
            }
            Ok(())
        }),
        // The registry the signed image comes from, with a pull credential: the image is
        // pinned by digest, so a credential can't change what runs.
        signed("/properties/configuration/registries", move |value| {
            let host = facts.workload.image.split('/').next().unwrap_or_default();
            for registry in value.as_array().into_iter().flatten() {
                if registry["server"].as_str() != Some(host) {
                    return Err(format!(
                        "it pulls from {}, the signed image comes from {host}",
                        registry["server"]
                    ));
                }
            }
            Ok(())
        }),
        // Only pull credentials, each one a registry's (see `check_secrets`); env may not read
        // a secret.
        signed("/properties/configuration/secrets", |value| {
            for secret in value.as_array().into_iter().flatten() {
                if !is_empty(&secret["keyVaultUrl"]) || !is_empty(&secret["identity"]) {
                    return Err(format!("secret {} is read from a vault", secret["name"]));
                }
            }
            Ok(())
        }),
        signed("/properties/configuration/dapr", move |value| {
            if is_empty(value) || !value["enabled"].as_bool().unwrap_or(false) {
                return Ok(());
            }
            (value["appId"].as_str() == Some(facts.own_name.as_str()))
                .then_some(())
                .ok_or_else(|| {
                    format!(
                        "its Dapr app id is {}, not {}",
                        value["appId"], facts.own_name
                    )
                })
        }),
        free("/properties/configuration/identitySettings"),
        signed("/properties/template/containers", move |value| {
            check_containers(facts, value)
        }),
        empty("/properties/template/initContainers"),
        empty("/properties/template/volumes"),
        empty("/properties/template/serviceBinds"),
        free("/properties/template/scale"),
        free("/properties/template/revisionSuffix"),
        free("/properties/template/terminationGracePeriodSeconds"),
        free("/properties/workloadProfileName"),
        free("/location"),
        free("/tags"),
        free("/name"),
    ]
}

/// Exactly one container: the signed image, no command of its own, the signed env.
fn check_containers(facts: &Facts<'_>, value: &Value) -> std::result::Result<(), String> {
    let containers = value.as_array().cloned().unwrap_or_default();
    let [container] = containers.as_slice() else {
        return Err(format!(
            "it has {} containers, a worker runs one",
            containers.len()
        ));
    };
    let rows = vec![
        signed("/image", |value| {
            let digest = facts
                .workload
                .image
                .rsplit_once('@')
                .map(|(_, digest)| digest.to_string())
                .unwrap_or_default();
            match value.as_str().and_then(|image| image.rsplit_once('@')) {
                Some((repository, sent)) if !repository.is_empty() && sent == digest => Ok(()),
                _ => Err(format!("image {value} is not the signed digest {digest}")),
            }
        }),
        signed("/env", |value| {
            let mut env = BTreeMap::new();
            for item in value.as_array().into_iter().flatten() {
                let name = item["name"].as_str().unwrap_or_default().to_string();
                if !is_empty(&item["secretRef"]) {
                    return Err(format!("env {name} is read from a secret"));
                }
                env.insert(name, item["value"].as_str().unwrap_or_default().to_string());
            }
            check_env(
                facts.place,
                facts.accepted,
                facts.workload,
                facts.reads,
                &facts.own_name,
                &env,
            )
            .map_err(|error| error.to_string())
        }),
        empty("/command"),
        empty("/args"),
        empty("/volumeMounts"),
        free("/name"),
        free("/resources"),
        free("/probes"),
    ];
    check_whole("Container Apps container", container, &rows).map_err(|error| error.to_string())
}

const UPDATE_WAIT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_secs(3);

/// Send `write`, already checked, to Container Apps as it is, checking `newest` right before;
/// wait for it to provision and read back what decides what runs.
pub async fn forward(
    store: &dyn AppStore,
    facts: &Facts<'_>,
    write: &CloudWrite,
    newest: &dyn StillNewest,
) -> Result<Value> {
    let app = match write {
        CloudWrite::CreateContainerApp { app, .. } => {
            newest.check().await?;
            store.create(&facts.own_name, app).await?;
            app
        }
        CloudWrite::UpdateContainerApp { app, .. } => {
            newest.check().await?;
            store.patch(&facts.own_name, app).await?;
            app
        }
        other => {
            return Err(refused(format!(
                "Container Apps takes no {} write",
                other.operation()
            )))
        }
    };
    let deadline = tokio::time::Instant::now() + UPDATE_WAIT;
    loop {
        let read = store.get(&facts.own_name).await?;
        let state = read["properties"]["provisioningState"]
            .as_str()
            .unwrap_or_default();
        if !matches!(state, "InProgress" | "Updating" | "Creating") {
            check_written(facts, app, &read)?;
            return Ok(serde_json::json!({
                "fqdn": read["properties"]["configuration"]["ingress"]["fqdn"],
            }));
        }
        if tokio::time::Instant::now() > deadline {
            return Err(AlienError::new(ErrorData::CloudCallFailed {
                operation: format!(
                    "waiting for container app {} to finish updating",
                    facts.own_name
                ),
            }));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Check that `read`, the app as Azure answers after the write, runs what `sent` asked: the
/// image, env and identities.
pub fn check_written(facts: &Facts<'_>, sent: &Value, read: &Value) -> Result<()> {
    let mismatch = |reason: String| {
        AlienError::new(ErrorData::ReadBackMismatch {
            function: facts.own_name.clone(),
            reason,
        })
    };
    let container = |app: &Value| app["properties"]["template"]["containers"][0].clone();
    for field in ["image", "env", "command", "args"] {
        let (a, b) = (&container(sent)[field], &container(read)[field]);
        if a != b && !(is_empty(a) && is_empty(b)) {
            return Err(mismatch(format!("its container's {field} is {b}")));
        }
    }
    // Azure answers resource ids in its own casing.
    let ids = |app: &Value| -> Vec<String> {
        let mut ids: Vec<String> = app["identity"]["userAssignedIdentities"]
            .as_object()
            .map(|ids| ids.keys().map(|id| id.to_lowercase()).collect())
            .unwrap_or_default();
        ids.sort();
        ids
    };
    if ids(sent) != ids(read) || read["identity"]["type"] != sent["identity"]["type"] {
        return Err(mismatch(format!("it runs as {}", read["identity"])));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::plan::{functions, Place, Reads};
    use crate::trust::tests::{found, MemoryStore};
    use crate::trust::{accept, Accepted};
    use alien_signing::{InstallId, SignedStatement};
    use serde_json::json;

    /// A change to a request, for a refusal case.
    type Change = Box<dyn Fn(&mut Value)>;

    const GROUP: &str = "/subscriptions/0000/resourceGroups/acme-rg";
    const ENVIRONMENT: &str =
        "/subscriptions/0000/resourceGroups/acme-rg/providers/Microsoft.App/managedEnvironments/acme-prod-default-container-env";
    const IMAGE_HOST: &str = "acmeprod.azurecr.io";

    fn place() -> Place {
        Place {
            install: InstallId::Azure {
                subscription: "0000".to_string(),
                resource_group: "acme-rg".to_string(),
            },
            prefix: PREFIX.to_string(),
            vpc: None,
            machine_network: None,
        }
    }

    fn own() -> String {
        format!("{PREFIX}-api")
    }

    fn azure_release() -> SignedStatement {
        let mut workload = function_workload_with(&format!("{IMAGE_HOST}/api@{DIGEST}"));
        workload
            .provided_env
            .push("ALIEN_PUBLIC_ENDPOINTS_JSON".to_string());
        let mut body = release_body(vec![workload]);
        body.platform = "azure".to_string();
        signed_release(1, &body)
    }

    async fn accepted(release: SignedStatement) -> Accepted {
        let install = found(&MemoryStore::protected(&settings())).await.unwrap();
        accept(&install, &[signer_list()], &release, None, "azure", now()).unwrap()
    }

    /// The app as the controller builds it for the signed private `api` workload.
    fn app() -> Value {
        let identity = place().run_as("execution");
        json!({
            "location": "eastus",
            "tags": {"resource-type": "worker", "resource": "api", "deployment": PREFIX},
            "identity": {"type": "UserAssigned", "userAssignedIdentities": {identity.clone(): {}}},
            "properties": {
                "managedEnvironmentId": ENVIRONMENT,
                "configuration": {
                    "activeRevisionsMode": "Single",
                    "identitySettings": [{"identity": identity, "lifecycle": "All"}],
                    "secrets": [{"name": "registry-proxy-password", "value": "deployment-token"}],
                    "registries": [{"server": IMAGE_HOST, "username": "deployment", "passwordSecretRef": "registry-proxy-password"}],
                },
                "template": {
                    "containers": [{
                        "name": "main",
                        "image": format!("{IMAGE_HOST}/api@{DIGEST}"),
                        "resources": {"cpu": 0.25, "memory": "0.5Gi"},
                        "env": [
                            {"name": "ALIEN_DEPLOYMENT_ID", "value": "dep_1"},
                            {"name": "MODE", "value": "signed"},
                        ],
                    }],
                    "scale": {"maxReplicas": 10, "minReplicas": 0},
                },
            },
        })
    }

    async fn checked(write: CloudWrite) -> std::result::Result<(), String> {
        checked_reading(write, Reads::default()).await
    }

    async fn checked_reading(write: CloudWrite, reads: Reads) -> std::result::Result<(), String> {
        let accepted = accepted(azure_release()).await;
        let place = place();
        let workload = functions(&accepted.release.body).next().unwrap();
        let facts = Facts {
            place: &place,
            accepted: &accepted,
            workload,
            reads: &reads,
            own_name: own(),
        };
        check_write(&facts, &write, Some(ENVIRONMENT)).map_err(|error| error.to_string())
    }

    fn create(app: Value) -> CloudWrite {
        CloudWrite::CreateContainerApp { name: own(), app }
    }

    #[tokio::test]
    async fn the_controllers_app_passes() {
        checked(create(app())).await.unwrap();
        let mut scaled = app();
        scaled["properties"]["template"]["scale"] = json!({"maxReplicas": 30, "minReplicas": 1});
        checked(CloudWrite::UpdateContainerApp {
            name: own(),
            app: scaled,
        })
        .await
        .unwrap();
        // Azure's casing of resource ids is the same id.
        let mut cased = app();
        cased["properties"]["managedEnvironmentId"] =
            json!(ENVIRONMENT.replace("resourceGroups", "resourcegroups"));
        checked(create(cased)).await.unwrap();
        let mut upper = app();
        upper["identity"]["userAssignedIdentities"] = json!({
            place().run_as("execution").to_uppercase(): {}
        });
        checked(create(upper)).await.unwrap();
    }

    /// The install's subscription and the Container Apps transport, which the deployment
    /// provides, are what the verifier knows them to be.
    #[tokio::test]
    async fn the_provided_subscription_and_transport_are_the_installs() {
        let mut body = release_body(vec![alien_signing::Workload {
            provided_env: [
                "ALIEN_DEPLOYMENT_ID",
                "ALIEN_TRANSPORT",
                "AZURE_SUBSCRIPTION_ID",
            ]
            .map(str::to_string)
            .to_vec(),
            ..function_workload_with(&format!("{IMAGE_HOST}/api@{DIGEST}"))
        }]);
        body.platform = "azure".to_string();
        let accepted = accepted(signed_release(1, &body)).await;
        let place = place();
        let reads = Reads::default();
        let workload = functions(&accepted.release.body).next().unwrap();
        let facts = Facts {
            place: &place,
            accepted: &accepted,
            workload,
            reads: &reads,
            own_name: own(),
        };
        let with = |name: &str, value: &str| {
            let mut app = app();
            let env = app["properties"]["template"]["containers"][0]["env"]
                .as_array_mut()
                .unwrap();
            for (key, derived) in [
                ("ALIEN_TRANSPORT", "container-app"),
                ("AZURE_SUBSCRIPTION_ID", "0000"),
            ] {
                let value = if key == name { value } else { derived };
                env.push(json!({"name": key, "value": value}));
            }
            create(app)
        };
        check_write(&facts, &with("", ""), Some(ENVIRONMENT)).expect("the derived values pass");
        for (name, value) in [
            ("ALIEN_TRANSPORT", "http"),
            ("AZURE_SUBSCRIPTION_ID", "attacker-subscription"),
        ] {
            let error = check_write(&facts, &with(name, value), Some(ENVIRONMENT))
                .expect_err(name)
                .to_string();
            assert!(
                error.contains(&format!(
                    "env {name} of workload 'api' is {value}, not the value the verifier derives"
                )),
                "{name}: {error}"
            );
        }
    }

    const APP_URL: &str =
        "https://acme-prod-api.internal.happyfield-1.eastus.azurecontainerapps.io";

    fn own_binding() -> Value {
        json!({
            "service": "containerapp",
            "subscriptionId": "0000",
            "resourceGroupName": "acme-rg",
            "containerAppName": own(),
            "privateUrl": APP_URL,
            "publicUrl": APP_URL,
        })
    }

    /// The app's own binding tells its code where to reach itself, so each field is the
    /// install's or what Azure reports for the app, and a binding that names the app but
    /// points `privateUrl` anywhere else is refused.
    #[tokio::test]
    async fn the_own_binding_is_the_installs_and_the_apps() {
        let with = |binding: &Value| {
            let mut app = app();
            let env = app["properties"]["template"]["containers"][0]["env"]
                .as_array_mut()
                .unwrap();
            env.push(json!({"name": "ALIEN_API_BINDING", "value": binding.to_string()}));
            // The control plane's own word for the app's public endpoints.
            env.push(json!({
                "name": "ALIEN_PUBLIC_ENDPOINTS_JSON",
                "value": json!({"api": {"url": "https://api.acme.example"}}).to_string(),
            }));
            CloudWrite::UpdateContainerApp { name: own(), app }
        };
        let reads = || Reads {
            own: crate::plan::OwnResource {
                urls: vec![APP_URL.to_string()],
                location: None,
            },
            ..Reads::default()
        };
        checked_reading(with(&own_binding()), reads())
            .await
            .expect("the binding the controller writes once the app has its address");
        let mut custom_domain = own_binding();
        custom_domain["publicUrl"] = json!("https://api.acme.example");
        checked_reading(with(&custom_domain), reads())
            .await
            .expect("a public endpoint the deployment provides is what the app displays");
        let cases: Vec<(Box<dyn Fn(&mut Value)>, &str)> = vec![
            (
                // The same word cannot also say where requests go.
                Box::new(|b| b["privateUrl"] = json!("https://api.acme.example")),
                "its privateUrl is https://api.acme.example, which is no URL the cloud reports",
            ),
            (
                Box::new(|b| b["privateUrl"] = json!("https://collector.attacker.example")),
                "its privateUrl is https://collector.attacker.example",
            ),
            (
                Box::new(|b| b["publicUrl"] = json!("https://collector.attacker.example")),
                "its publicUrl is https://collector.attacker.example",
            ),
            (
                Box::new(|b| {
                    b.as_object_mut().unwrap().remove("privateUrl");
                }),
                "it has no privateUrl",
            ),
            (
                Box::new(|b| b["resourceGroupName"] = json!("other-rg")),
                "its resourceGroupName is \"other-rg\", not acme-rg",
            ),
            (
                Box::new(|b| b["subscriptionId"] = json!("9999")),
                "its subscriptionId is \"9999\", not 0000",
            ),
            (
                Box::new(|b| b["containerAppName"] = json!("acme-prod-other")),
                "its containerAppName is \"acme-prod-other\"",
            ),
            (
                Box::new(|b| b["service"] = json!("lambda")),
                "its service is \"lambda\", not containerapp",
            ),
            (
                Box::new(|b| b["callback"] = json!("https://collector.attacker.example")),
                "it has a field callback the platform's binding doesn't",
            ),
        ];
        for (change, says) in cases {
            let mut binding = own_binding();
            change(&mut binding);
            let error = checked_reading(with(&binding), reads())
                .await
                .expect_err(says);
            assert!(error.contains(says), "{says}: {error}");
        }
        // Before the app has an address, the cloud reports none, and no URL can pass.
        let error = checked(with(&own_binding())).await.unwrap_err();
        assert!(error.contains("its privateUrl is"), "{error}");
    }

    /// One refusal per row of the Container Apps table, naming the row.
    #[tokio::test]
    async fn each_app_row_refuses_what_breaks_it() {
        let cases: Vec<(Change, &str)> = vec![
            (
                Box::new(|a| a["identity"]["type"] = json!("SystemAssigned,UserAssigned")),
                "field /identity must match",
            ),
            (
                Box::new(|a| {
                    a["identity"]["userAssignedIdentities"][format!(
                        "{GROUP}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/admin"
                    )] = json!({});
                }),
                "which isn't an identity the workload's permission profile names",
            ),
            (
                // The verifier's own identity: it reads the signing vault and writes the apps.
                Box::new(|a| {
                    a["identity"]["userAssignedIdentities"][format!(
                        "{GROUP}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{PREFIX}-verifier"
                    )] = json!({});
                }),
                "isn't an identity the workload's permission profile names",
            ),
            (
                // Another permission profile's identity.
                Box::new(|a| {
                    a["identity"]["userAssignedIdentities"][place().run_as("reader")] = json!({});
                }),
                "isn't an identity the workload's permission profile names",
            ),
            (
                // Setup's management identity.
                Box::new(|a| {
                    a["identity"]["userAssignedIdentities"][format!(
                        "{GROUP}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{PREFIX}-management-sa"
                    )] = json!({});
                }),
                "isn't an identity the workload's permission profile names",
            ),
            (
                Box::new(|a| {
                    a["properties"]["managedEnvironmentId"] = json!(format!(
                        "{GROUP}/providers/Microsoft.App/managedEnvironments/acme-prod-verifier"
                    ))
                }),
                "field /properties/managedEnvironmentId must match",
            ),
            (
                Box::new(|a| {
                    a["properties"]["configuration"]["activeRevisionsMode"] = json!("Multiple")
                }),
                "field /properties/configuration/activeRevisionsMode must match",
            ),
            (
                Box::new(|a| {
                    a["properties"]["configuration"]["ingress"] =
                        json!({"external": true, "targetPort": 8080})
                }),
                "field /properties/configuration/ingress must match",
            ),
            (
                Box::new(|a| {
                    a["properties"]["configuration"]["registries"][0]["server"] =
                        json!("evil.azurecr.io")
                }),
                "field /properties/configuration/registries must match",
            ),
            (
                Box::new(
                    |a| a["properties"]["configuration"]["secrets"] = json!([{"name": "registry-proxy-password", "value": "t"}, {"name": "stolen", "value": "x"}]),
                ),
                "secret stolen is no registry's pull credential",
            ),
            (
                Box::new(|a| {
                    a["properties"]["configuration"]["dapr"] =
                        json!({"enabled": true, "appId": "other-app", "appPort": 8080})
                }),
                "field /properties/configuration/dapr must match",
            ),
            (
                Box::new(|a| {
                    a["properties"]["template"]["initContainers"] =
                        json!([{"name": "x", "image": "evil"}])
                }),
                "field /properties/template/initContainers must be empty",
            ),
            (
                Box::new(|a| {
                    a["properties"]["template"]["volumes"] =
                        json!([{"name": "v", "storageType": "EmptyDir"}])
                }),
                "field /properties/template/volumes must be empty",
            ),
            (
                Box::new(|a| {
                    a["properties"]["template"]["containers"][0]["image"] =
                        json!(format!("{IMAGE_HOST}/api@sha256:{}", "5".repeat(64)))
                }),
                "is not the signed digest",
            ),
            (
                Box::new(|a| {
                    a["properties"]["template"]["containers"][0]["command"] = json!(["/bin/sh"])
                }),
                "field /command must be empty",
            ),
            (
                Box::new(|a| {
                    a["properties"]["template"]["containers"][0]["env"][1] =
                        json!({"name": "MODE", "secretRef": "registry-proxy-password"})
                }),
                "env MODE is read from a secret",
            ),
            (
                Box::new(|a| {
                    a["properties"]["template"]["containers"][0]["env"][1]["value"] =
                        json!("attacker")
                }),
                "differs from its signed",
            ),
            (
                Box::new(|a| {
                    let c = a["properties"]["template"]["containers"][0].clone();
                    a["properties"]["template"]["containers"] = json!([c.clone(), c]);
                }),
                "it has 2 containers",
            ),
        ];
        for (change, says) in cases {
            let mut app = app();
            change(&mut app);
            let error = checked(create(app)).await.expect_err(says);
            assert!(error.contains(says), "{says}: {error}");
        }
        let error = checked(CloudWrite::CreateContainerApp {
            name: format!("{PREFIX}-verifier"),
            app: app(),
        })
        .await
        .unwrap_err();
        assert!(
            error.contains("names function acme-prod-verifier"),
            "{error}"
        );
    }
}
