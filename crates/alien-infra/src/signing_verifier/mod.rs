//! A protected install's verifier, `<prefix>-verifier`.
//!
//! Setup creates the verifier and only setup changes it: the management identity may not, and
//! its controller writes nothing. It is imported Ready at registration, and its outputs name it.

use std::time::Duration;

use alien_core::{
    import::{data::SigningVerifierImportData, ImportContext},
    ResourceOutputs, ResourceStatus, SigningVerifier, SigningVerifierOutputs, StackResourceState,
};
use alien_error::AlienError;
use alien_macros::controller;

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
    import::ResourceImporter,
    import_helpers::make_imported_state,
};

/// The verifier's controller, the same on every cloud: setup owns the resource.
#[controller]
pub struct SigningVerifierController {
    /// `<prefix>-verifier`.
    pub(crate) name: Option<String>,
}

#[controller]
impl SigningVerifierController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<SigningVerifier>()?;
        Err(AlienError::new(ErrorData::SetupRerunRequired {
            resource_id: config.id.clone(),
            message: format!(
                "the install's verifier {}-verifier is created by setup; run setup with this release",
                ctx.resource_prefix
            ),
        }))
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(300)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        // The verifier has no setting of its own; setup changes its image and grants.
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
        // Setup's teardown removes it with the rest of what setup created.
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
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        Some(ResourceOutputs::new(SigningVerifierOutputs {
            name: self.name.clone()?,
        }))
    }
}

/// Imports the verifier setup created, on any cloud.
#[derive(Debug, Default)]
pub struct SigningVerifierImporter;

impl ResourceImporter for SigningVerifierImporter {
    type ImportData = SigningVerifierImportData;

    fn import(
        &self,
        data: SigningVerifierImportData,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        make_imported_state(
            SigningVerifierController {
                state: SigningVerifierState::Ready,
                name: Some(data.name),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use alien_core::Platform;
    use std::sync::Arc;

    /// Management never creates the verifier: a stack that gains one before setup made it says
    /// setup must run, and calls no cloud (the provider mock has no expectation).
    #[tokio::test]
    async fn only_setup_creates_the_verifier() {
        let mut executor = SingleControllerExecutor::builder()
            .resource(SigningVerifier::new(SigningVerifier::ID.to_string()).build())
            .controller(SigningVerifierController::default())
            .platform(Platform::Aws)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .await
            .unwrap();
        let error = executor
            .run_until_terminal()
            .await
            .expect_err("setup creates the verifier");
        assert_eq!(error.code, "SETUP_RERUN_REQUIRED", "{error:?}");
        assert!(
            error
                .to_string()
                .contains("verifier test-verifier is created by setup"),
            "{error}"
        );
    }
}
