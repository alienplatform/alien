use acme_types::{DnsZone, DnsZoneOutputs, GcpDnsZoneFields};
use alien_extension_sdk::controller_prelude::*;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

/// The zone on Google Cloud. Alien has no Cloud DNS client, so the controller writes the
/// requests, and Alien sends each to `dns.googleapis.com` as the deployment.
#[controller]
pub struct GcpDnsZoneController {
    #[serde(flatten)]
    fields: GcpDnsZoneFields,
    /// The label value this zone's create writes. A create that finds the name taken adopts the
    /// zone only when it carries this value, so the delete never empties a zone it didn't make.
    /// A zone the importer read from the setup template has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    create_reference: Option<String>,
    /// The domain the zone was made for. A rename the update refused still leaves the new domain
    /// in the stack, so the delete reads the zone's own. A zone the importer read has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
}

#[controller]
impl GcpDnsZoneController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // Saved by this step, so a CreateZone that runs again after a lost reply writes the same
        // value and recognizes the zone it made.
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "The clock is before 1970, so the create can't pick a reference"
                    .to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?
            .as_millis();
        self.create_reference = Some(millis.to_string());
        self.domain = Some(ctx.desired_resource_config::<DnsZone>()?.domain.clone());
        Ok(HandlerAction::Continue {
            state: CreateZone,
            suggested_delay: None,
        })
    }

    #[handler(state = CreateZone, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_zone(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let zone = ctx.desired_resource_config::<DnsZone>()?;
        let reference = self.create_reference.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "The zone has no create reference to create it with".to_string(),
                resource_id: Some(zone.id.clone()),
            })
        })?;
        // The name the Terraform module gives the zone, so both setup paths save the same one.
        let name = format!("{}-{}", ctx.resource_prefix, zone.id);
        let request = CloudRequest::gcp("dns", "POST", &zones_path(ctx)?).json(&json!({
            "name": name,
            "dnsName": format!("{}.", zone.domain),
            "description": format!("The {} zone", zone.id),
            "labels": { CREATE_LABEL: reference },
        }))?;
        let response = ctx.cloud().send(&request)?;
        if response.status == 409 {
            // Taken: by this step's own earlier run, or by a zone someone else made.
            let path = format!("{}/{name}", zones_path(ctx)?);
            let existing: ManagedZone = ctx
                .cloud()
                .send(&CloudRequest::gcp("dns", "GET", &path))?
                .ok("Reading the Cloud DNS zone that has the name")?
                .json()?;
            if existing.labels.get(CREATE_LABEL) != Some(&reference) {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "A Cloud DNS zone named {name} already exists and this resource didn't create it"
                    ),
                    resource_id: Some(zone.id.clone()),
                }));
            }
        } else {
            response.ok("Creating the Cloud DNS zone")?;
        }
        self.fields.zone_name = Some(name);
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let previous = ctx.previous_resource_config::<DnsZone>()?;
        let zone = ctx.desired_resource_config::<DnsZone>()?;
        if previous.domain != zone.domain {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "A Cloud DNS zone can't change its domain from {} to {}",
                    previous.domain, zone.domain
                ),
                resource_id: Some(zone.id.clone()),
            }));
        }
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let zone = ctx.desired_resource_config::<DnsZone>()?;
        // The name both setup paths give the zone; the delete touches no other.
        let expected = format!("{}-{}", ctx.resource_prefix, zone.id);
        let refuse = |message: String| {
            AlienError::new(ErrorData::CloudPlatformError {
                message,
                resource_id: Some(zone.id.clone()),
            })
        };
        let path = format!("{}/{expected}", zones_path(ctx)?);
        let delete = match (&self.fields.zone_name, &self.create_reference) {
            (Some(name), _) if *name != expected => {
                return Err(refuse(format!(
                    "The zone to delete is named {name}, not {expected}, so it was not made for this resource"
                )))
            }
            // Read from the setup template, which names the zone `expected`.
            (Some(_), None) => true,
            // Made by this resource, or maybe by a create whose reply was lost before it saved
            // the name: deleted only while it carries the create's label.
            (saved, Some(reference)) => {
                let response = ctx
                    .cloud()
                    .send(&CloudRequest::gcp("dns", "GET", &path))?;
                if response.status == 404 {
                    false
                } else {
                    let existing: ManagedZone = response
                        .ok("Reading the Cloud DNS zone to delete")?
                        .json()?;
                    let labelled = existing.labels.get(CREATE_LABEL) == Some(reference);
                    if !labelled && saved.is_some() {
                        return Err(refuse(format!(
                            "The Cloud DNS zone {expected} no longer carries the label its create wrote, so it is left for you to delete"
                        )));
                    }
                    labelled
                }
            }
            (None, None) => false,
        };
        if delete {
            let domain = self.domain.as_deref().unwrap_or(&zone.domain);
            // A zone with more record sets than a step deletes takes more steps.
            if !delete_record_sets(ctx, &path, domain)? {
                return Ok(HandlerAction::Continue {
                    state: DeleteStart,
                    suggested_delay: None,
                });
            }
            let response = ctx
                .cloud()
                .send(&CloudRequest::gcp("dns", "DELETE", &path))?;
            // A second run of this step gets 404: the first one deleted the zone.
            if response.status != 404 {
                response.ok("Deleting the Cloud DNS zone")?;
            }
        }
        self.fields.zone_name = None;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let zone_id = self.fields.zone_name.clone()?;
        Some(ResourceOutputs::new(DnsZoneOutputs { zone_id }))
    }
}

/// The path of the managed zones of the project the step is for.
fn zones_path(ctx: &ResourceControllerContext<'_>) -> Result<String> {
    match ctx.cloud().target() {
        Some(CloudTarget::Gcp { project_id, .. }) => {
            Ok(format!("/dns/v1/projects/{project_id}/managedZones"))
        }
        _ => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "The step is not for a Google Cloud project".to_string(),
            resource_id: Some(ctx.desired_config.id().to_string()),
        })),
    }
}

/// The label a zone's create writes its reference under.
const CREATE_LABEL: &str = "create-reference";

/// The part of a managed zone the create and the delete read.
#[derive(Deserialize)]
struct ManagedZone {
    #[serde(default)]
    labels: std::collections::BTreeMap<String, String>,
}

/// One page of a zone's record sets.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordSets {
    #[serde(default)]
    rrsets: Vec<RecordSet>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct RecordSet {
    name: String,
    #[serde(rename = "type")]
    record_type: String,
}

/// The record sets one step deletes. Alien sends at most 50 cloud calls for a step, which leaves
/// room for reading the zone, listing its record sets and deleting the zone.
const DELETES_PER_STEP: usize = 40;

/// Deletes up to [`DELETES_PER_STEP`] of the record sets of the zone at `zone_path` for `domain`
/// that Cloud DNS lets it delete: all but the SOA and NS at the zone's apex, which go with the
/// zone. Returns whether none is left. A record set or a zone that is already gone is deleted, so
/// a step that runs again after a lost reply succeeds.
fn delete_record_sets(
    ctx: &ResourceControllerContext<'_>,
    zone_path: &str,
    domain: &str,
) -> Result<bool> {
    let rrsets_path = format!("{zone_path}/rrsets");
    let mut batch = Vec::new();
    let mut more = false;
    let mut page_token: Option<String> = None;
    loop {
        let mut request = CloudRequest::gcp("dns", "GET", &rrsets_path);
        if let Some(token) = &page_token {
            request = request.query("pageToken", token);
        }
        let response = ctx.cloud().send(&request)?;
        if response.status == 404 {
            return Ok(true);
        }
        let page: RecordSets = response
            .ok("Listing the Cloud DNS zone's record sets")?
            .json()?;
        for set in page.rrsets {
            let at_apex = crate::aws::same_domain(&set.name, domain);
            if at_apex && matches!(set.record_type.as_str(), "SOA" | "NS") {
                continue;
            }
            if batch.len() == DELETES_PER_STEP {
                more = true;
                break;
            }
            batch.push(set);
        }
        match page.next_page_token {
            Some(_) if more || batch.len() == DELETES_PER_STEP => {
                more = true;
                break;
            }
            Some(token) => page_token = Some(token),
            None => break,
        }
    }
    for set in batch {
        let path = format!("{rrsets_path}/{}/{}", set.name, set.record_type);
        let response = ctx
            .cloud()
            .send(&CloudRequest::gcp("dns", "DELETE", &path))?;
        if response.status != 404 {
            response.ok("Deleting a Cloud DNS record set")?;
        }
    }
    Ok(!more)
}

#[cfg(test)]
mod tests {
    use alien_extension_sdk::controller_module::testing::answer;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    const ZONE: &str = "/dns/v1/projects/the-project/managedZones/q6aa0b26-zone";

    fn step(state: Value, transitions: Value) -> Value {
        step_for("internal.acme.dev", state, transitions)
    }

    fn step_for(domain: &str, state: Value, transitions: Value) -> Value {
        json!({
            "protocol": 1, "stepId": "step-1", "resourceType": "acme/dns-zone",
            "platform": "gcp", "resourcePrefix": "q6aa0b26",
            "resource": { "type": "acme/dns-zone", "id": "zone", "domain": domain },
            "previousResource": null, "dependencies": {}, "state": state,
            "transitions": transitions,
            "cloud": { "platform": "gcp", "projectId": "the-project", "region": "us-central1" },
            "deadline": "2099-01-01T00:00:00Z"
        })
    }

    /// The state after the create's first step, which picks the reference.
    fn started() -> Value {
        let started = answer(crate::register, &step(Value::Null, json!([])), |request| {
            panic!("the first step sends nothing: {request}")
        })
        .unwrap();
        assert_eq!(started["controller"]["status"], "provisioning", "{started}");
        started["controller"]["state"].clone()
    }

    /// The reference a started create writes as the zone's label.
    fn reference_of(state: &Value) -> String {
        state["createReference"]
            .as_str()
            .expect("the state holds the reference")
            .to_string()
    }

    /// The state of a zone the controller created and keeps.
    fn ready() -> Value {
        let created = answer(crate::register, &step(started(), json!([])), |_| {
            Ok(json!({ "status": 200, "headers": [], "body": "{}" }))
        })
        .unwrap();
        assert_eq!(created["controller"]["status"], "running", "{created}");
        created["controller"]["state"].clone()
    }

    fn reply(status: u16, body: Value) -> Result<Value, String> {
        Ok(json!({ "status": status, "headers": [], "body": body.to_string() }))
    }

    fn set(name: &str, kind: &str) -> Value {
        json!({ "kind": "dns#resourceRecordSet", "name": name, "type": kind, "ttl": 300, "rrdatas": ["x"] })
    }

    /// A Cloud DNS zone labelled with `reference`, holding `pages` of record sets. Like Cloud DNS,
    /// it refuses to delete the zone while a record set other than the apex SOA and NS is left.
    fn a_zone_holding(
        reference: &str,
        pages: Vec<Vec<Value>>,
    ) -> (
        impl FnMut(&Value) -> Result<Value, String>,
        Arc<Mutex<Vec<String>>>,
    ) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let seen = sent.clone();
        let mut deleted: Vec<String> = Vec::new();
        let label = reference.to_string();
        let host = move |request: &Value| {
            let method = request["method"].as_str().unwrap().to_string();
            let path = request["path"].as_str().unwrap().to_string();
            let token = request["query"]
                .as_array()
                .unwrap()
                .iter()
                .find(|pair| pair[0] == "pageToken")
                .map(|pair| pair[1].as_str().unwrap().to_string());
            seen.lock().unwrap().push(format!("{method} {path}"));
            let rrsets = format!("{ZONE}/rrsets");
            match method.as_str() {
                "GET" if path == ZONE => reply(
                    200,
                    json!({ "name": "q6aa0b26-zone", "labels": { "create-reference": label } }),
                ),
                "GET" if path == rrsets => {
                    let page: usize = token.map_or(0, |token| token.parse().unwrap());
                    // Like Cloud DNS, a record set deleted is no longer listed.
                    let left: Vec<&Value> = pages[page]
                        .iter()
                        .filter(|set| {
                            !deleted.contains(&format!(
                                "{rrsets}/{}/{}",
                                set["name"].as_str().unwrap(),
                                set["type"].as_str().unwrap()
                            ))
                        })
                        .collect();
                    let mut body = json!({ "rrsets": left });
                    if page + 1 < pages.len() {
                        body["nextPageToken"] = json!((page + 1).to_string());
                    }
                    reply(200, body)
                }
                "DELETE" if path.starts_with(&format!("{rrsets}/")) => {
                    deleted.push(path.clone());
                    reply(200, json!({}))
                }
                "DELETE" if path == ZONE => {
                    let left = pages.iter().flatten().any(|set| {
                        let apex = set["name"] == "internal.acme.dev."
                            && (set["type"] == "SOA" || set["type"] == "NS");
                        let path = format!(
                            "{rrsets}/{}/{}",
                            set["name"].as_str().unwrap(),
                            set["type"].as_str().unwrap()
                        );
                        !apex && !deleted.contains(&path)
                    });
                    if left {
                        reply(
                            400,
                            json!({ "error": { "code": 400, "message": "The resource named 'q6aa0b26-zone' cannot be deleted because it is not empty", "errors": [{ "reason": "containerNotEmpty" }] } }),
                        )
                    } else {
                        reply(204, json!({}))
                    }
                }
                _ => reply(
                    400,
                    json!({ "error": { "code": 400, "message": "not in this test" } }),
                ),
            }
        };
        (host, sent)
    }

    // Cloud DNS refuses to delete a zone holding records of its own, so the delete reads every
    // page and removes those records first. The apex SOA and NS go with the zone; Cloud DNS
    // refuses to delete them alone.
    #[test]
    fn delete_removes_the_records_then_the_zone() {
        let state = ready();
        let (host, sent) = a_zone_holding(
            &reference_of(&state),
            vec![
                vec![
                    set("internal.acme.dev.", "NS"),
                    set("internal.acme.dev.", "SOA"),
                    set("www.internal.acme.dev.", "A"),
                ],
                vec![set("team.internal.acme.dev.", "NS")],
            ],
        );
        let reply = answer(crate::register, &step(state, json!(["delete-start"])), host).unwrap();

        assert_eq!(reply["result"], "stepped", "{reply}");
        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(
            *sent.lock().unwrap(),
            [
                format!("GET {ZONE}"),
                format!("GET {ZONE}/rrsets"),
                format!("GET {ZONE}/rrsets"),
                format!("DELETE {ZONE}/rrsets/www.internal.acme.dev./A"),
                format!("DELETE {ZONE}/rrsets/team.internal.acme.dev./NS"),
                format!("DELETE {ZONE}"),
            ]
        );
    }

    // A delete that runs again after the zone went: nothing left to delete, and no error.
    #[test]
    fn delete_of_a_zone_that_is_gone_succeeds() {
        let reply = answer(
            crate::register,
            &step(ready(), json!(["delete-start"])),
            |_| {
                reply(
                    404,
                    json!({ "error": { "code": 404, "message": "notFound" } }),
                )
            },
        )
        .unwrap();

        assert_eq!(reply["result"], "stepped", "{reply}");
        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
    }

    // The create's reply was lost and it runs again: the name is taken by the zone it made, which
    // carries its reference, so it takes that zone.
    #[test]
    fn a_create_that_runs_again_takes_the_zone_it_made() {
        let state = started();
        let reference = reference_of(&state);
        let created = answer(
            crate::register,
            &step(state, json!([])),
            move |request| match request["method"].as_str().unwrap() {
                "POST" => reply(
                    409,
                    json!({ "error": { "code": 409, "message": "alreadyExists" } }),
                ),
                "GET" => reply(
                    200,
                    json!({ "name": "q6aa0b26-zone", "labels": { "create-reference": reference } }),
                ),
                other => panic!("{other}"),
            },
        )
        .unwrap();
        assert_eq!(created["controller"]["status"], "running", "{created}");
    }

    // A zone with the name already exists and this resource didn't make it: the create fails
    // rather than take it, so a later delete can't empty someone else's zone.
    #[test]
    fn a_zone_of_the_same_name_it_did_not_make_is_not_taken() {
        let created = answer(
            crate::register,
            &step(started(), json!([])),
            |request| match request["method"].as_str().unwrap() {
                "POST" => reply(
                    409,
                    json!({ "error": { "code": 409, "message": "alreadyExists" } }),
                ),
                "GET" => reply(
                    200,
                    json!({ "name": "q6aa0b26-zone", "labels": { "create-reference": "1" } }),
                ),
                other => panic!("{other}"),
            },
        )
        .unwrap();
        assert_eq!(created["result"], "failed", "{created}");
        assert_eq!(
            created["controller"]["onFailureStatus"], "provision-failed",
            "{created}"
        );
        // No zone name is saved: a delete from here reads the zone and finds another's label.
        assert_eq!(
            created["controller"]["state"]["zoneName"],
            Value::Null,
            "{created}"
        );
        assert!(
            created.to_string().contains("didn't create it"),
            "{created}"
        );
    }

    // The delete after that refused create reads the zone, finds another's label, and leaves the
    // zone and its records alone.
    #[test]
    fn delete_after_a_refused_create_leaves_the_other_zone() {
        let (host, sent) =
            a_zone_holding("another", vec![vec![set("www.internal.acme.dev.", "A")]]);
        let reply = answer(
            crate::register,
            &step(started(), json!(["delete-start"])),
            host,
        )
        .unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(*sent.lock().unwrap(), [format!("GET {ZONE}")]);
    }

    // The zone this resource made no longer carries its label: the delete leaves the zone and
    // fails, naming it, rather than report a zone it did not remove as deleted.
    #[test]
    fn delete_of_a_zone_that_lost_its_label_fails() {
        let (host, sent) =
            a_zone_holding("another", vec![vec![set("www.internal.acme.dev.", "A")]]);
        let reply = answer(
            crate::register,
            &step(ready(), json!(["delete-start"])),
            host,
        )
        .unwrap();

        assert_eq!(reply["result"], "failed", "{reply}");
        assert_eq!(
            reply["controller"]["onFailureStatus"], "delete-failed",
            "{reply}"
        );
        assert!(
            reply.to_string().contains("no longer carries the label"),
            "{reply}"
        );
        assert_eq!(*sent.lock().unwrap(), [format!("GET {ZONE}")]);
    }

    /// The state the importer saves for a zone the setup template made: a name, no reference.
    fn imported(name: &str) -> Value {
        let mut state = ready();
        state.as_object_mut().unwrap().remove("createReference");
        state["zoneName"] = json!(name);
        state
    }

    // A zone read from the setup template has no label to check; it has the name the template
    // gives it, so the delete removes its records and the zone.
    #[test]
    fn delete_of_an_imported_zone_removes_it() {
        let (host, sent) = a_zone_holding("", vec![vec![set("www.internal.acme.dev.", "A")]]);
        let reply = answer(
            crate::register,
            &step(imported("q6aa0b26-zone"), json!(["delete-start"])),
            host,
        )
        .unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(
            *sent.lock().unwrap(),
            [
                format!("GET {ZONE}/rrsets"),
                format!("DELETE {ZONE}/rrsets/www.internal.acme.dev./A"),
                format!("DELETE {ZONE}"),
            ]
        );
    }

    // The delete of an imported zone runs again after its zone went: every call answers 404.
    #[test]
    fn delete_of_an_imported_zone_that_is_gone_succeeds() {
        let reply = answer(
            crate::register,
            &step(imported("q6aa0b26-zone"), json!(["delete-start"])),
            |_| {
                reply(
                    404,
                    json!({ "error": { "code": 404, "message": "notFound" } }),
                )
            },
        )
        .unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
    }

    // An imported zone with another name was not made for this resource: the delete sends
    // nothing and fails.
    #[test]
    fn delete_of_an_imported_zone_with_another_name_fails() {
        let reply = answer(
            crate::register,
            &step(imported("customers-prod-zone"), json!(["delete-start"])),
            |request| panic!("the delete sends nothing: {request}"),
        )
        .unwrap();

        assert_eq!(reply["result"], "failed", "{reply}");
        assert!(
            reply.to_string().contains("not made for this resource"),
            "{reply}"
        );
    }

    // A create whose call made the zone but whose reply was lost saved no zone name; the delete
    // still finds the zone by its name and label, so nothing is left behind.
    #[test]
    fn delete_after_a_failed_create_removes_the_zone_it_made() {
        let state = started();
        let (host, sent) = a_zone_holding(&reference_of(&state), vec![vec![]]);
        let reply = answer(crate::register, &step(state, json!(["delete-start"])), host).unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(
            *sent.lock().unwrap(),
            [
                format!("GET {ZONE}"),
                format!("GET {ZONE}/rrsets"),
                format!("DELETE {ZONE}"),
            ]
        );
    }

    // The update refused a rename, and the stack still names the new domain: the delete keeps
    // the apex records of the domain the zone was made for, which go with the zone.
    #[test]
    fn delete_after_a_refused_rename_keeps_the_zone_s_own_apex() {
        let state = ready();
        let (host, sent) = a_zone_holding(
            &reference_of(&state),
            vec![vec![
                set("internal.acme.dev.", "NS"),
                set("internal.acme.dev.", "SOA"),
                set("www.internal.acme.dev.", "A"),
            ]],
        );
        let reply = answer(
            crate::register,
            &step_for("renamed.acme.dev", state, json!(["delete-start"])),
            host,
        )
        .unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(
            *sent.lock().unwrap(),
            [
                format!("GET {ZONE}"),
                format!("GET {ZONE}/rrsets"),
                format!("DELETE {ZONE}/rrsets/www.internal.acme.dev./A"),
                format!("DELETE {ZONE}"),
            ]
        );
    }

    // A create whose reply was lost saved no zone name, and the stack now names another domain:
    // the delete keeps the apex records of the domain the create sent.
    #[test]
    fn delete_after_a_lost_create_keeps_the_apex_of_the_domain_the_create_sent() {
        let state = started();
        let (host, sent) = a_zone_holding(
            &reference_of(&state),
            vec![vec![
                set("internal.acme.dev.", "NS"),
                set("internal.acme.dev.", "SOA"),
            ]],
        );
        let reply = answer(
            crate::register,
            &step_for("renamed.acme.dev", state, json!(["delete-start"])),
            host,
        )
        .unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(
            *sent.lock().unwrap(),
            [
                format!("GET {ZONE}"),
                format!("GET {ZONE}/rrsets"),
                format!("DELETE {ZONE}"),
            ]
        );
    }

    // A step sends at most 50 cloud calls, so a zone with more record sets than a step deletes
    // is emptied over several steps, then deleted.
    #[test]
    fn a_large_zone_is_emptied_over_several_steps() {
        let state = ready();
        let sets: Vec<Value> = (0..45)
            .map(|index| set(&format!("r{index}.internal.acme.dev."), "A"))
            .collect();
        let (host, sent) = a_zone_holding(&reference_of(&state), vec![sets]);
        let host = Arc::new(Mutex::new(host));
        let first = answer(crate::register, &step(state, json!(["delete-start"])), {
            let host = host.clone();
            move |request: &Value| (host.lock().unwrap())(request)
        })
        .unwrap();
        assert_eq!(first["controller"]["status"], "deleting", "{first}");
        let deletes = |sent: &[String]| {
            sent.iter()
                .filter(|line| line.starts_with("DELETE"))
                .count()
        };
        assert_eq!(deletes(&sent.lock().unwrap()), 40);

        let second = answer(
            crate::register,
            &step(first["controller"]["state"].clone(), json!([])),
            {
                let host = host.clone();
                move |request: &Value| (host.lock().unwrap())(request)
            },
        )
        .unwrap();
        assert_eq!(second["controller"]["status"], "deleted", "{second}");
        let sent = sent.lock().unwrap();
        assert_eq!(deletes(&sent), 46, "45 record sets and the zone");
        assert_eq!(sent.last().unwrap(), &format!("DELETE {ZONE}"));
    }
}
