use acme_types::{AwsDnsZoneFields, DnsZone, DnsZoneOutputs};
use alien_extension_sdk::controller_prelude::*;

#[controller]
pub struct AwsDnsZoneController {
    #[serde(flatten)]
    fields: AwsDnsZoneFields, // the same fields the importer fills
}

#[controller]
impl AwsDnsZoneController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let zone = ctx.desired_resource_config::<DnsZone>()?;
        let route53 = aws_sdk_route53::Client::new(&ctx.aws_sdk_config().await?);
        let failed = || ErrorData::CloudPlatformError {
            message: "Failed to create the Route53 hosted zone".to_string(),
            resource_id: Some(zone.id.clone()),
        };
        // Unique per deployment and zone.
        let reference = format!("{}-{}", ctx.resource_prefix, zone.id);
        let request = route53
            .create_hosted_zone()
            .name(&zone.domain)
            .caller_reference(&reference);
        let id = match request.send().await {
            Ok(created) => created.hosted_zone().map(|z| z.id().to_string()),
            // A second run gets HostedZoneAlreadyExists.
            // Find the zone by its caller reference and save its id.
            Err(e)
                if e.as_service_error()
                    .is_some_and(|e| e.is_hosted_zone_already_exists()) =>
            {
                let zones = route53
                    .list_hosted_zones_by_name()
                    .dns_name(&zone.domain)
                    .send()
                    .await;
                let zones = zones.into_alien_error().context(failed())?;
                let found = zones
                    .hosted_zones()
                    .iter()
                    .find(|z| z.caller_reference() == reference);
                found.map(|z| z.id().to_string())
            }
            Err(e) => return Err(e).into_alien_error().context(failed()),
        };
        // CloudFormation's Ref returns "Z123", so keep that form on both setup paths.
        self.fields.zone_id = id.map(|id| id.trim_start_matches("/hostedzone/").to_string());
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // Check that the zone still exists.
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        // Apply the changed config, then go back to Ready.
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        // Delete the zone's records, then the zone.
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
