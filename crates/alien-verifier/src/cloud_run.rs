//! Cloud Run as the verifier sees it: the calls it makes, the network setup gives functions, and
//! whether a service serves what its template asked.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::Result;
use crate::plan::SetupNetwork;

/// The Cloud Run calls the verifier makes.
#[async_trait]
pub trait ServiceStore: Send + Sync {
    /// GET the service, exactly as returned.
    async fn get(&self, name: &str) -> Result<Value>;
    /// PATCH the service with `service`, replacing its template.
    async fn replace(&self, name: &str, service: &Value) -> Result<()>;
    /// Create service `name` from `service`.
    async fn create(&self, name: &str, service: &Value) -> Result<()>;
    /// The service's IAM policy.
    async fn policy(&self, name: &str) -> Result<Value>;
    /// Replace the service's IAM policy with `policy`, which carries the etag it was read at.
    async fn set_policy(&self, name: &str, policy: &Value) -> Result<()>;
}

/// Direct VPC egress through the subnet setup gives functions.
pub(crate) fn vpc_access(vpc: &SetupNetwork) -> Value {
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
pub(crate) fn network_of(vpc_access: &Value) -> Option<(Vec<String>, Vec<String>, String)> {
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

/// The last path segment of a revision name.
fn short_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// Refuse a service that doesn't serve all traffic from the revision its
/// template created, or whose revision isn't ready: the template reading
/// back right says nothing about what answers requests.
pub(crate) fn check_serving(service: &Value) -> std::result::Result<(), String> {
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

    /// A service as Cloud Run answers once revision 2 serves everything.
    fn serving() -> Value {
        let revisions = "projects/p/locations/us-central1/services/acme-fn-api/revisions";
        json!({
            "latestCreatedRevision": format!("{revisions}/acme-fn-api-00002-x7k"),
            "latestReadyRevision": format!("{revisions}/acme-fn-api-00002-x7k"),
            "terminalCondition": {"type": "Ready", "state": "CONDITION_SUCCEEDED"},
            "trafficStatuses": [{
                "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST",
                "revision": "acme-fn-api-00002-x7k",
                "percent": 100,
            }],
        })
    }

    #[test]
    fn only_the_created_and_ready_revision_may_serve() {
        check_serving(&serving()).unwrap();
        let reason = |change: &dyn Fn(&mut Value)| {
            let mut changed = serving();
            change(&mut changed);
            check_serving(&changed).expect_err("must not serve")
        };
        // Attack: keep 90% of requests on an older revision that runs
        // another image.
        assert_eq!(
            reason(&|s| s["trafficStatuses"] = json!([
                {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "acme-fn-api-00001-old", "percent": 90},
                {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "revision": "acme-fn-api-00002-x7k", "percent": 10},
            ])),
            "traffic is split or tagged over 2 targets"
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
    /// after a PATCH (us-central1, 2026-11-05).
    #[test]
    fn real_traffic_reports_read_as_cloud_run_returns_them() {
        let revisions = "projects/p/locations/us-central1/services/acme-fn-api/revisions";
        let mut split = serving();
        split["latestCreatedRevision"] = json!(format!("{revisions}/acme-fn-api-00002-cqd"));
        split["latestReadyRevision"] = split["latestCreatedRevision"].clone();
        split["terminalCondition"] = json!({"type": "Ready", "state": "CONDITION_SUCCEEDED", "lastTransitionTime": "2026-11-05T14:28:54.447929Z"});
        split["trafficStatuses"] = json!([
            {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "acme-fn-api-00001-k95", "percent": 90},
            {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "revision": "acme-fn-api-00002-cqd", "percent": 10},
        ]);
        assert!(check_serving(&split).is_err());

        let mut patched = split;
        patched["latestCreatedRevision"] = json!(format!("{revisions}/acme-fn-api-00003-bj6"));
        patched["latestReadyRevision"] = patched["latestCreatedRevision"].clone();
        patched["trafficStatuses"] = json!([
            {"type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "revision": "acme-fn-api-00003-bj6", "percent": 100},
        ]);
        check_serving(&patched).unwrap();
    }

    /// Setup's subnet, as setup names it and as Cloud Run reads it back, is the same network;
    /// another subnet or narrower egress is not.
    #[test]
    fn the_network_setup_gives_functions_compares_as_cloud_run_reads_it() {
        let setup = SetupNetwork {
            subnet_ids: vec![
                "https://www.googleapis.com/compute/v1/projects/p/regions/us-central1/subnetworks/acme-workload"
                    .into(),
            ],
            security_group_ids: Vec::new(),
            identity: None,
            egress: Some("ALL_TRAFFIC".into()),
        };
        let sent = vpc_access(&setup);
        assert_eq!(
            sent,
            json!({
                "networkInterfaces": [{
                    "subnetwork": "projects/p/regions/us-central1/subnetworks/acme-workload"
                }],
                "egress": "ALL_TRAFFIC",
            })
        );
        let read = json!({
            "networkInterfaces": [{"network": "acme-net", "subnetwork": "acme-workload"}],
            "egress": "ALL_TRAFFIC",
        });
        assert_eq!(network_of(&read), network_of(&sent));
        assert!(network_of(&read).is_some());
        let mut moved = read.clone();
        moved["networkInterfaces"][0]["subnetwork"] = json!("elsewhere");
        assert_ne!(network_of(&moved), network_of(&sent));
        let mut narrowed = read;
        narrowed["egress"] = json!("PRIVATE_RANGES_ONLY");
        assert_ne!(network_of(&narrowed), network_of(&sent));
        assert_eq!(network_of(&json!({})), None);
    }
}
