//! Importer for the AWS Sandbox.

use alien_core::{
    import::{data::AwsSandboxImportData, ImportContext},
    ErrorData as CoreErrorData, Platform, ResourceStatus, Result, Sandbox, StackResourceState,
};
use alien_error::AlienError;

use crate::core::{serialize_controller, ResourceController};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state_with_status;
use crate::sandbox::{AwsSandboxController, AwsSandboxState};

/// AWS Sandbox importer.
///
/// Two registration shapes arrive here, and which fields are present says which. A Frozen
/// sandbox names the image stack creation built, and imports Ready. A Live one names the
/// build role and bundle instead — the two values the runtime controller cannot derive — and
/// imports at the start of the create flow, so the deployment loop builds the image once. A
/// later release's changed bundle is rolled by the update flow, not by re-importing.
#[derive(Debug, Default)]
pub struct AwsSandboxImporter;

impl ResourceImporter for AwsSandboxImporter {
    type ImportData = AwsSandboxImportData;

    fn import(
        &self,
        data: AwsSandboxImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let sandbox = ctx
            .resource
            .config
            .downcast_ref::<Sandbox>()
            .ok_or_else(|| {
                AlienError::new(CoreErrorData::ImportDataInvalid {
                    resource_id: ctx.resource_id.to_string(),
                    resource_type: Sandbox::RESOURCE_TYPE,
                    platform: Platform::Aws,
                })
            })?;

        let base = AwsSandboxController {
            egress_connector_arns: data.egress_connector_arns,
            allow_egress: data.allow_egress,
            preview_ports: data.preview_ports,
            idle_suspend_seconds: sandbox.session.idle_suspend_seconds,
            max_lifetime_seconds: sandbox.session.max_lifetime_seconds,
            ..Default::default()
        };

        match (
            data.image_identifier,
            data.image_arn,
            data.image_version,
            data.build_role_arn,
            data.bundle_uri,
        ) {
            // Frozen: stack creation built the image; nothing is left to provision.
            (Some(image_identifier), Some(image_arn), Some(image_version), _, _) => {
                // The region the ARN names — needed before a binding can be published, and
                // waiting for a health tick to supply it leaves the application with no
                // sandbox to address in the meantime.
                let region = image_arn
                    .split(':')
                    .nth(3)
                    .filter(|region| !region.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        AlienError::new(CoreErrorData::ImportDataInvalid {
                            resource_id: ctx.resource_id.to_string(),
                            resource_type: Sandbox::RESOURCE_TYPE,
                            platform: Platform::Aws,
                        })
                    })?;
                let controller = AwsSandboxController {
                    state: AwsSandboxState::Ready,
                    image_identifier: Some(image_identifier),
                    image_arn: Some(image_arn),
                    active_version: Some(image_version),
                    region: Some(region),
                    ..base
                };
                make_imported_state_with_status(controller, ctx, ResourceStatus::Running)
            }
            // Live: setup registered the build inputs; the controller builds the image once
            // the deployment loop steps it, which is why this imports Provisioning at the
            // create entry state rather than Running.
            (None, None, None, Some(build_role_arn), Some(bundle_uri)) => {
                let controller = AwsSandboxController {
                    state: AwsSandboxState::CreatingImage,
                    build_role_arn: Some(build_role_arn),
                    bundle_uri: Some(bundle_uri),
                    ..base
                };
                make_imported_state_with_status(controller, ctx, ResourceStatus::Provisioning)
            }
            // A partial payload is a contract violation, not something to guess through: a
            // half-imported sandbox either enumerates the wrong sessions or builds nothing.
            _ => Err(AlienError::new(CoreErrorData::ImportDataInvalid {
                resource_id: ctx.resource_id.to_string(),
                resource_type: Sandbox::RESOURCE_TYPE,
                platform: Platform::Aws,
            })),
        }
    }

    /// A Live sandbox's image is runtime-owned, so a re-import must not replace the state that
    /// tracks it: the default would drop the built version — withdrawing the binding of a
    /// sandbox that is serving — and re-run the create flow against an image that exists.
    ///
    /// Only the setup-owned facts cross over. The bundle deliberately does not: a new release's
    /// bundle is a desired-config change and reaches the image through the update flow.
    fn merge_reimport(
        &self,
        existing: StackResourceState,
        imported: StackResourceState,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let (Some(existing_state), Some(imported_state)) = (
            existing.internal_state.clone(),
            imported.internal_state.clone(),
        ) else {
            return Ok(imported);
        };

        let existing_controller =
            AwsSandboxController::from_persisted(existing_state).map_err(|error| {
                AlienError::new(CoreErrorData::GenericError {
                    message: format!(
                        "sandbox '{}' has unreadable controller state: {error}",
                        ctx.resource_id
                    ),
                })
            })?;
        let imported_controller: AwsSandboxController = serde_json::from_value(imported_state)
            .map_err(|error| {
                AlienError::new(CoreErrorData::GenericError {
                    message: format!(
                        "sandbox '{}' was re-imported with unreadable state: {error}",
                        ctx.resource_id
                    ),
                })
            })?;

        // A Frozen registration names no build role. Its image is setup-owned and stack creation
        // is authoritative about it, so replacement is right there.
        if imported_controller.build_role_arn.is_none() {
            return Ok(imported);
        }

        let merged = AwsSandboxController {
            build_role_arn: imported_controller.build_role_arn,
            egress_connector_arns: imported_controller.egress_connector_arns,
            allow_egress: imported_controller.allow_egress,
            preview_ports: imported_controller.preview_ports,
            region: imported_controller
                .region
                .or(existing_controller.region.clone()),
            ..existing_controller
        };

        // The connectors decide what a session can reach, so the binding is recomputed rather
        // than carried: a changed connector must reach the application.
        let remote_binding_params = if ctx.resource.publishes_binding_params() {
            merged.get_binding_params().map_err(|error| {
                AlienError::new(CoreErrorData::GenericError {
                    message: format!(
                        "binding params extraction failed for resource '{}': {error}",
                        ctx.resource_id
                    ),
                })
            })?
        } else {
            None
        };
        let outputs = merged.get_outputs();
        let internal_state = serialize_controller(&merged).map_err(|error| {
            AlienError::new(CoreErrorData::JsonSerializationFailed {
                reason: format!(
                    "controller serialization failed for resource '{}': {error}",
                    ctx.resource_id
                ),
            })
        })?;

        Ok(StackResourceState {
            internal_state: Some(internal_state),
            outputs,
            remote_binding_params,
            ..existing
        })
    }
}
