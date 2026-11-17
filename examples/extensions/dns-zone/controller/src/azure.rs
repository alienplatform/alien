use acme_types::{AzureDnsZoneFields, DnsZone, DnsZoneOutputs};
use alien_extension_sdk::controller_prelude::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const API_VERSION: &str = "2018-05-01";

/// The zone on Azure. Alien has no Azure DNS client, so the controller writes the requests, and
/// Alien sends each to Azure Resource Manager as the deployment.
#[controller]
pub struct AzureDnsZoneController {
    #[serde(flatten)]
    fields: AzureDnsZoneFields,
    /// The tag value this zone's create writes. A create that finds the zone there takes it only
    /// when it carries this value, and a delete removes only a zone that does. A zone the
    /// importer read from the setup template has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    create_reference: Option<String>,
    /// The domain the zone was made for. A rename the update refused still leaves the new domain
    /// in the stack, so the delete reads the zone's own. A zone the importer read has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
}

/// The tag a zone's create writes its reference under.
const CREATE_TAG: &str = "create-reference";

/// What the deployment's resource group outputs, read as the controller's own type.
#[derive(Deserialize)]
struct ResourceGroup {
    name: String,
}

/// What Azure returns for a zone.
#[derive(Deserialize)]
struct Zone {
    id: String,
    /// Azure may send `null` for a zone with no tags.
    #[serde(default)]
    tags: Option<BTreeMap<String, String>>,
}

impl Zone {
    fn tag(&self, name: &str) -> Option<&String> {
        self.tags.as_ref()?.get(name)
    }
}

#[controller]
impl AzureDnsZoneController {
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
        // Names this resource as well as this create: two resources for one domain never share it.
        self.create_reference = Some(format!(
            "{}-{}-{millis}",
            ctx.resource_prefix,
            ctx.desired_config.id()
        ));
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
        let path = zone_path(ctx, &zone, &zone.domain)?;
        // `If-None-Match: *` creates the zone only where there is none, rather than take over
        // a zone that is already at the domain.
        let request = CloudRequest::azure("PUT", &path)
            .query("api-version", API_VERSION)
            .header("If-None-Match", "*")
            .json(&json!({ "location": "global", "tags": { CREATE_TAG: reference } }))?;
        let response = ctx.cloud().send(&request)?;
        let created: Zone = if response.status == 412 {
            // Taken: by this step's own earlier run, or by a zone someone else made.
            let existing: Zone = ctx
                .cloud()
                .send(&CloudRequest::azure("GET", &path).query("api-version", API_VERSION))?
                .ok("Reading the Azure DNS zone at the domain")?
                .json()?;
            if existing.tag(CREATE_TAG) != Some(&reference) {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "An Azure DNS zone for {} already exists and this resource didn't create it",
                        zone.domain
                    ),
                    resource_id: Some(zone.id.clone()),
                }));
            }
            existing
        } else {
            response.ok("Creating the Azure DNS zone")?.json()?
        };
        self.fields.zone_id = Some(created.id);
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
                    "An Azure DNS zone can't change its domain from {} to {}",
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
        let domain = self.domain.clone().unwrap_or_else(|| zone.domain.clone());
        let refuse = |message: String| {
            AlienError::new(ErrorData::CloudPlatformError {
                message,
                resource_id: Some(zone.id.clone()),
            })
        };
        let delete = match (&self.fields.zone_id, &self.create_reference) {
            // Read from the setup template, which makes the zone at the stack's domain.
            (Some(zone_id), None) => {
                if !zone_id.eq_ignore_ascii_case(&zone_path(ctx, &zone, &domain)?) {
                    return Err(refuse(format!(
                        "The zone to delete is {zone_id}, not the zone for {}, so it was not made for this resource",
                        domain
                    )));
                }
                Some(zone_id.clone())
            }
            // Made by this resource, or maybe by a create whose reply was lost before it saved
            // the id: deleted only while it carries the create's tag.
            (saved, Some(reference)) => {
                // A saved id is the zone's own path; without one, the zone is where the create
                // put it.
                let path = match saved {
                    Some(zone_id) => zone_id.clone(),
                    None => zone_path(ctx, &zone, &domain)?,
                };
                let response = ctx
                    .cloud()
                    .send(&CloudRequest::azure("GET", &path).query("api-version", API_VERSION))?;
                if response.status == 404 {
                    None
                } else {
                    let existing: Zone = response
                        .ok("Reading the Azure DNS zone to delete")?
                        .json()?;
                    if existing.tag(CREATE_TAG) == Some(reference) {
                        Some(existing.id)
                    } else if saved.is_some() {
                        return Err(refuse(format!(
                            "The Azure DNS zone for {} no longer carries the tag its create wrote, so it is left for you to delete",
                            domain
                        )));
                    } else {
                        None
                    }
                }
            }
            (None, None) => None,
        };
        // The id is the zone's path in Resource Manager. A zone's delete deletes its records.
        if let Some(zone_id) = delete {
            let request = CloudRequest::azure("DELETE", &zone_id).query("api-version", API_VERSION);
            let response = ctx.cloud().send(&request)?;
            // A second run of this step gets 404: the first one deleted the zone.
            if response.status != 404 {
                response.ok("Deleting the Azure DNS zone")?;
            }
        }
        self.fields.zone_id = None;
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
        let zone_id = self.fields.zone_id.clone()?;
        Some(ResourceOutputs::new(DnsZoneOutputs { zone_id }))
    }
}

/// The path of `zone`'s zone for `domain` in the deployment's resource group.
fn zone_path(ctx: &ResourceControllerContext<'_>, zone: &DnsZone, domain: &str) -> Result<String> {
    let Some(CloudTarget::Azure {
        subscription_id, ..
    }) = ctx.cloud().target()
    else {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "The step is not for an Azure subscription".to_string(),
            resource_id: Some(zone.id.clone()),
        }));
    };
    // Every resource of an Azure deployment depends on its resource group.
    let group: ResourceGroup = ctx.dependency_outputs("default-resource-group")?;
    Ok(format!(
        "/subscriptions/{subscription_id}/resourceGroups/{}/providers/Microsoft.Network/dnsZones/{}",
        group.name, domain
    ))
}

#[cfg(test)]
mod tests {
    use alien_extension_sdk::controller_module::testing::answer;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    const GROUP: &str = "/subscriptions/the-subscription/resourceGroups/rg-q6aa0b26";
    const ZONE: &str = "/subscriptions/the-subscription/resourceGroups/rg-q6aa0b26/providers/Microsoft.Network/dnsZones/internal.acme.dev";

    fn step(state: Value, transitions: Value) -> Value {
        step_for("internal.acme.dev", state, transitions)
    }

    fn step_for(domain: &str, state: Value, transitions: Value) -> Value {
        json!({
            "protocol": 1, "stepId": "step-1", "resourceType": "acme/dns-zone",
            "platform": "azure", "resourcePrefix": "q6aa0b26",
            "resource": { "type": "acme/dns-zone", "id": "zone", "domain": domain },
            "previousResource": null, "state": state, "transitions": transitions,
            "dependencies": {
                "default-resource-group": {
                    "resourceType": "azure_resource_group", "status": "running",
                    "outputs": {
                        "type": "azure_resource_group", "name": "rg-q6aa0b26",
                        "resourceId": GROUP, "location": "westeurope"
                    },
                    "bindingParams": null
                }
            },
            "cloud": { "platform": "azure", "subscriptionId": "the-subscription", "tenantId": "the-tenant" },
            "deadline": "2099-01-01T00:00:00Z"
        })
    }

    fn reply(status: u16, body: Value) -> Result<Value, String> {
        Ok(json!({ "status": status, "headers": [], "body": body.to_string() }))
    }

    fn zone(tag: &str) -> Value {
        json!({ "id": ZONE, "location": "global", "tags": { "create-reference": tag } })
    }

    /// The state after the create's first step, which picks the reference.
    fn started() -> Value {
        let started = answer(crate::register, &step(Value::Null, json!([])), |request| {
            panic!("the first step sends nothing: {request}")
        })
        .unwrap();
        started["controller"]["state"].clone()
    }

    fn reference_of(state: &Value) -> String {
        state["createReference"]
            .as_str()
            .expect("the state holds the reference")
            .to_string()
    }

    /// A host that records each call and answers the zone's GET with `tag`.
    fn azure_holding(
        tag: &'static str,
    ) -> (
        impl FnMut(&Value) -> Result<Value, String>,
        Arc<Mutex<Vec<String>>>,
    ) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let seen = sent.clone();
        let host = move |request: &Value| {
            let method = request["method"].as_str().unwrap().to_string();
            let path = request["path"].as_str().unwrap().to_string();
            seen.lock().unwrap().push(format!("{method} {path}"));
            match method.as_str() {
                "GET" => reply(200, zone(tag)),
                "DELETE" => reply(200, json!({})),
                other => panic!("{other}"),
            }
        };
        (host, sent)
    }

    // The create asks Azure to make the zone only where there is none, tagged with its
    // reference, and saves the zone's id.
    #[test]
    fn create_makes_a_tagged_zone_only_where_there_is_none() {
        let state = started();
        let reference = reference_of(&state);
        let sent_reference = reference.clone();
        let request_seen = Arc::new(Mutex::new(Value::Null));
        let seen = request_seen.clone();
        let created = answer(crate::register, &step(state, json!([])), move |request| {
            *seen.lock().unwrap() = request.clone();
            reply(201, zone(&reference))
        })
        .unwrap();

        assert_eq!(created["controller"]["status"], "running", "{created}");
        let request = request_seen.lock().unwrap().clone();
        assert_eq!(request["method"], "PUT");
        assert!(
            request["headers"]
                .as_array()
                .unwrap()
                .contains(&json!(["If-None-Match", "*"])),
            "{request}"
        );
        let body: Value = serde_json::from_str(request["body"].as_str().unwrap()).unwrap();
        assert_eq!(
            body["tags"]["create-reference"],
            json!(sent_reference),
            "{body}"
        );
        assert_eq!(created["controller"]["state"]["zoneId"], ZONE, "{created}");
    }

    // The create's reply was lost and it runs again: the zone at the domain carries its tag,
    // so it takes that zone.
    #[test]
    fn a_create_that_runs_again_takes_the_zone_it_made() {
        let state = started();
        let reference = reference_of(&state);
        let created = answer(
            crate::register,
            &step(state, json!([])),
            move |request| match request["method"].as_str().unwrap() {
                "PUT" => reply(412, json!({ "error": { "code": "PreconditionFailed" } })),
                "GET" => reply(200, zone(&reference)),
                other => panic!("{other}"),
            },
        )
        .unwrap();

        assert_eq!(created["controller"]["status"], "running", "{created}");
        assert_eq!(created["controller"]["state"]["zoneId"], ZONE, "{created}");
    }

    // A zone is already at the domain and this resource didn't make it: the create fails
    // rather than take it.
    #[test]
    fn a_zone_at_the_domain_it_did_not_make_is_not_taken() {
        let created = answer(
            crate::register,
            &step(started(), json!([])),
            |request| match request["method"].as_str().unwrap() {
                "PUT" => reply(412, json!({ "error": { "code": "PreconditionFailed" } })),
                "GET" => reply(200, zone("another")),
                other => panic!("{other}"),
            },
        )
        .unwrap();

        assert_eq!(created["result"], "failed", "{created}");
        assert!(
            created.to_string().contains("didn't create it"),
            "{created}"
        );
        assert_eq!(
            created["controller"]["state"]["zoneId"],
            Value::Null,
            "{created}"
        );
    }

    /// The state of a zone the controller created and keeps.
    fn ready() -> Value {
        let state = started();
        let reference = reference_of(&state);
        let created = answer(crate::register, &step(state, json!([])), move |_| {
            reply(201, zone(&reference))
        })
        .unwrap();
        created["controller"]["state"].clone()
    }

    #[test]
    fn delete_removes_the_zone_it_made() {
        let state = ready();
        let tag: &'static str = Box::leak(reference_of(&state).into_boxed_str());
        let (host, sent) = azure_holding(tag);
        let reply = answer(crate::register, &step(state, json!(["delete-start"])), host).unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(
            *sent.lock().unwrap(),
            [format!("GET {ZONE}"), format!("DELETE {ZONE}")]
        );
    }

    // The zone this resource made no longer carries its tag: the delete leaves it and fails,
    // naming it, rather than report a zone it did not remove as deleted.
    #[test]
    fn delete_of_a_zone_that_lost_its_tag_fails() {
        let (host, sent) = azure_holding("another");
        let reply = answer(
            crate::register,
            &step(ready(), json!(["delete-start"])),
            host,
        )
        .unwrap();

        assert_eq!(reply["result"], "failed", "{reply}");
        assert_eq!(*sent.lock().unwrap(), [format!("GET {ZONE}")]);
    }

    // The delete after a create refused for another's zone reads it, finds another's tag, and
    // leaves it.
    #[test]
    fn delete_after_a_refused_create_leaves_the_other_zone() {
        let (host, sent) = azure_holding("another");
        let reply = answer(
            crate::register,
            &step(started(), json!(["delete-start"])),
            host,
        )
        .unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(*sent.lock().unwrap(), [format!("GET {ZONE}")]);
    }

    /// The state the importer saves for a zone the setup template made: an id, no reference.
    fn imported(zone_id: &str) -> Value {
        let mut state = ready();
        state.as_object_mut().unwrap().remove("createReference");
        state["zoneId"] = json!(zone_id);
        state
    }

    #[test]
    fn delete_of_an_imported_zone_removes_it() {
        let (host, sent) = azure_holding("");
        let reply = answer(
            crate::register,
            &step(imported(ZONE), json!(["delete-start"])),
            host,
        )
        .unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(*sent.lock().unwrap(), [format!("DELETE {ZONE}")]);
    }

    #[test]
    fn delete_of_an_imported_zone_for_another_domain_fails() {
        let other = format!("{GROUP}/providers/Microsoft.Network/dnsZones/customer.example");
        let reply = answer(
            crate::register,
            &step(imported(&other), json!(["delete-start"])),
            |request| panic!("the delete sends nothing: {request}"),
        )
        .unwrap();

        assert_eq!(reply["result"], "failed", "{reply}");
    }

    // A delete that runs again after the zone went: the read answers 404, and it is done.
    #[test]
    fn delete_of_a_zone_that_is_gone_succeeds() {
        let reply = answer(
            crate::register,
            &step(ready(), json!(["delete-start"])),
            |request| match request["method"].as_str().unwrap() {
                "GET" => reply(404, json!({ "error": { "code": "ResourceNotFound" } })),
                other => panic!("the delete sends no {other}"),
            },
        )
        .unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
    }

    // The update refused a rename, and the stack still names the new domain: the delete reads
    // and removes the zone at the domain it was made for.
    #[test]
    fn delete_after_a_refused_rename_removes_the_zone_it_made() {
        let state = ready();
        let tag: &'static str = Box::leak(reference_of(&state).into_boxed_str());
        let (host, sent) = azure_holding(tag);
        let reply = answer(
            crate::register,
            &step_for("renamed.acme.dev", state, json!(["delete-start"])),
            host,
        )
        .unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(
            *sent.lock().unwrap(),
            [format!("GET {ZONE}"), format!("DELETE {ZONE}")]
        );
    }

    // A create whose reply was lost saved no id, and the stack now names another domain: the
    // delete still finds the zone at the domain its create sent, by its tag.
    #[test]
    fn delete_after_a_lost_create_uses_the_domain_the_create_sent() {
        let state = started();
        let tag: &'static str = Box::leak(reference_of(&state).into_boxed_str());
        let (host, sent) = azure_holding(tag);
        let reply = answer(
            crate::register,
            &step_for("renamed.acme.dev", state, json!(["delete-start"])),
            host,
        )
        .unwrap();

        assert_eq!(reply["controller"]["status"], "deleted", "{reply}");
        assert_eq!(
            *sent.lock().unwrap(),
            [format!("GET {ZONE}"), format!("DELETE {ZONE}")]
        );
    }
}
