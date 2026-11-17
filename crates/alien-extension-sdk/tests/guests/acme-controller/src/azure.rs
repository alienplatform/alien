use acme_types::{AzureDnsZoneFields, DnsZone, DnsZoneOutputs};
use alien_extension_sdk::controller_prelude::*;
use serde_json::json;

const API_VERSION: &str = "2018-05-01";

/// The zone on Azure. Alien has no Azure DNS client, so the controller writes the requests, and
/// Alien sends each to Azure Resource Manager as the deployment.
#[controller]
pub struct AzureDnsZoneController {
    #[serde(flatten)]
    fields: AzureDnsZoneFields,
}

/// What the deployment's resource group outputs, read as the controller's own type.
#[derive(Deserialize)]
struct ResourceGroup {
    name: String,
}

/// What Azure returns for a zone.
#[derive(Deserialize)]
struct Zone {
    id: String,
}

#[controller]
impl AzureDnsZoneController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let zone = ctx.desired_resource_config::<DnsZone>()?;
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
        let path = format!(
            "/subscriptions/{subscription_id}/resourceGroups/{}/providers/Microsoft.Network/dnsZones/{}",
            group.name, zone.domain
        );
        // A PUT of the same zone answers with the zone, so a second run of this step is safe.
        let request = CloudRequest::azure("PUT", &path)
            .query("api-version", API_VERSION)
            .json(&json!({ "location": "global" }))?;
        let created: Zone = ctx
            .cloud()
            .send(&request)?
            .ok("Creating the Azure DNS zone")?
            .json()?;
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
        // No id: the create never made a zone. The id is the zone's path in Resource Manager.
        if let Some(zone_id) = &self.fields.zone_id {
            let request = CloudRequest::azure("DELETE", zone_id).query("api-version", API_VERSION);
            let response = ctx.cloud().send(&request)?;
            // A second run of this step gets 404: the first one deleted the zone.
            if response.status != 404 {
                response.ok("Deleting the Azure DNS zone")?;
            }
            self.fields.zone_id = None;
        }
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
