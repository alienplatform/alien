use acme_types::{DnsZone, DnsZoneOutputs, GcpDnsZoneFields};
use alien_extension_sdk::controller_prelude::*;
use serde_json::json;

/// The zone on Google Cloud. Alien has no Cloud DNS client, so the controller writes the
/// requests, and Alien sends each to `dns.googleapis.com` as the deployment.
#[controller]
pub struct GcpDnsZoneController {
    #[serde(flatten)]
    fields: GcpDnsZoneFields,
}

#[controller]
impl GcpDnsZoneController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let zone = ctx.desired_resource_config::<DnsZone>()?;
        // The name the Terraform module gives the zone, so both setup paths save the same one.
        let name = format!("{}-{}", ctx.resource_prefix, zone.id);
        let request = CloudRequest::gcp("dns", "POST", &zones_path(ctx)?).json(&json!({
            "name": name,
            "dnsName": format!("{}.", zone.domain),
            "description": format!("The {} zone", zone.id),
        }))?;
        let response = ctx.cloud().send(&request)?;
        // A second run of this step gets 409: the first one made the zone.
        if response.status != 409 {
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
        // No name: the create never made a zone.
        if let Some(name) = &self.fields.zone_name {
            let path = format!("{}/{name}", zones_path(ctx)?);
            let response = ctx
                .cloud()
                .send(&CloudRequest::gcp("dns", "DELETE", &path))?;
            // A second run of this step gets 404: the first one deleted the zone.
            if response.status != 404 {
                response.ok("Deleting the Cloud DNS zone")?;
            }
            self.fields.zone_name = None;
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
