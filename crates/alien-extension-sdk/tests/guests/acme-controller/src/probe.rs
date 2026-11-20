//! A controller for what the SDK's tests step and the docs' zone never uses: a request to an
//! origin the extension declares.

pub use resource::Probe;

use alien_extension_sdk::controller_prelude::*;

mod resource {
    use alien_extension_sdk::prelude::*;

    #[alien_resource(type = "acme/probe", outputs = ProbeOutputs, lifecycles = [Live], holds_data = false)]
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema, bon::Builder)]
    #[serde(rename_all = "camelCase")]
    #[builder(start_fn = new)]
    pub struct Probe {
        #[builder(start_fn)]
        pub id: String,
        /// The URL the probe posts to.
        pub url: String,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
    #[serde(rename_all = "camelCase")]
    pub struct ProbeOutputs {
        pub answer: String,
    }
}

#[controller]
pub struct ProbeController {
    answer: Option<String>,
}

#[controller]
impl ProbeController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = Failed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let probe = ctx.desired_resource_config::<Probe>()?;
        // The vendor's own key, and a body that is not text.
        let request = HttpRequest::new("POST", &probe.url)
            .header("authorization", "Bearer the-vendor-key")
            .body_bytes(&[0xff, 0x00, 0x7f]);
        let response = ctx.cloud().http(&request)?.ok("The vendor's API")?;
        self.answer = Some(format!("{} {:?}", response.status, response.bytes()?));
        Ok(HandlerAction::Continue {
            state: Done,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = Failed, status = ResourceStatus::Deleting)]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(state = Done, status = ResourceStatus::Running);
    terminal_state!(state = Failed, status = ResourceStatus::ProvisionFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let answer = self.answer.clone()?;
        Some(ResourceOutputs::new(resource::ProbeOutputs { answer }))
    }
}
