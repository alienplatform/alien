//! A controller written against `controller_prelude`, the way the docs' controller crate is:
//! `#[controller]` expands to `crate::core`, `crate::error` and `crate::Result`, which the
//! prelude's glob import provides at the crate root.

use alien_extension_sdk::controller_prelude::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneFields {
    pub zone_id: Option<String>,
}

#[controller]
pub struct ZoneController {
    #[serde(flatten)]
    fields: ZoneFields,
}

#[controller]
impl ZoneController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.fields.zone_id = Some("Z123".to_string());
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

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        None
    }
}

#[test]
fn a_controller_compiles_against_the_prelude_and_saves_its_fields() {
    let controller = ZoneController {
        fields: ZoneFields {
            zone_id: Some("Z123".to_string()),
        },
        ..Default::default()
    };
    let saved = serde_json::to_value(&controller).unwrap();
    assert_eq!(saved["zoneId"], "Z123");
    assert_eq!(saved["state"], "createStart");
}
