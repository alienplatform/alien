//! Importer for the AWS Sandbox.

use alien_core::{
    import::{data::AwsSandboxImportData, ImportContext},
    ErrorData as CoreErrorData, Platform, ResourceLifecycle, ResourceStatus, Result, Sandbox,
    StackResourceState,
};
use alien_error::AlienError;

use crate::core::{serialize_controller, ResourceController};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state_with_status;
use crate::sandbox::{AwsSandboxController, AwsSandboxState};

/// AWS Sandbox importer.
///
/// A template's Frozen sandbox names its built image. Legacy egress imports Ready; Internet
/// first initializes its gateway configuration. A Live sandbox, or a Frozen one a direct setup
/// registers, names the build role and bundle and starts the create flow, so the deployment loop
/// builds the image once; later bundles roll via update.
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
            egress: Some(sandbox.into()),
            egress_connector_arns: data.egress_connector_arns,
            allow_egress: data.allow_egress,
            preview_ports: data.preview_ports,
            idle_pause_seconds: sandbox.lifecycle.idle_pause_seconds,
            max_lifetime_seconds: sandbox.lifecycle.max_lifetime_seconds,
            ..Default::default()
        };

        match (
            data.image_identifier,
            data.image_arn,
            data.image_version,
            data.build_role_arn,
            data.bundle_uri,
        ) {
            // Frozen: setup built the image; Internet still needs gateway initialization.
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
                    state: if matches!(sandbox.egress, alien_core::SandboxEgress::Internet) {
                        AwsSandboxState::InitializingEgressKey
                    } else {
                        AwsSandboxState::Ready
                    },
                    image_identifier: Some(image_identifier),
                    image_arn: Some(image_arn),
                    active_version: Some(image_version),
                    region: Some(region),
                    ..base
                };
                let status = if controller.state == AwsSandboxState::Ready {
                    ResourceStatus::Running
                } else {
                    ResourceStatus::Provisioning
                };
                make_imported_state_with_status(controller, ctx, status)
            }
            // Setup registered the build inputs; the controller builds the image once
            // the deployment loop steps it, which is why this imports Provisioning at the
            // create entry state rather than Running.
            (None, None, None, Some(build_role_arn), Some(bundle_uri)) => {
                let controller = AwsSandboxController {
                    state: AwsSandboxState::CreatingImage,
                    build_role_arn: Some(build_role_arn),
                    granted_bundle_uri: Some(bundle_uri.clone()),
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

    /// A registration naming build inputs leaves the image to the controller, so a re-import must
    /// keep the state tracking it: replacing would drop the built version, unbinding a serving
    /// sandbox. A direct setup re-registers a Frozen sandbox this way on every pass.
    ///
    /// Only the setup-owned facts cross over. The bundle deliberately does not: a new release's
    /// bundle is a desired-config change and reaches the image through the update flow. A Frozen
    /// registration naming a built image replaces the state outright.
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

        let mut imported_controller: AwsSandboxController = serde_json::from_value(imported_state)
            .map_err(|error| {
                AlienError::new(CoreErrorData::GenericError {
                    message: format!(
                        "sandbox '{}' was re-imported with unreadable state: {error}",
                        ctx.resource_id
                    ),
                })
            })?;
        let existing_controller =
            AwsSandboxController::from_persisted(existing_state).map_err(|error| {
                AlienError::new(CoreErrorData::GenericError {
                    message: format!(
                        "sandbox '{}' has unreadable controller state: {error}",
                        ctx.resource_id
                    ),
                })
            })?;

        let requires_gateway = imported_controller.egress.as_ref().is_some_and(|value| {
            matches!(value.declaration(), alien_core::SandboxEgress::Internet)
        });
        // Older serving sandboxes publish no egress descriptor. Repeating setup
        // must keep that binding stable when the egress declaration is unchanged.
        if !requires_gateway
            && existing_controller.egress.is_none()
            && existing
                .config
                .downcast_ref::<Sandbox>()
                .is_some_and(|sandbox| {
                    imported_controller
                        .egress
                        .as_ref()
                        .is_some_and(|egress| egress.declaration() == &sandbox.egress)
                })
        {
            imported_controller.egress = None;
        }
        if ctx.resource.lifecycle == ResourceLifecycle::Frozen
            && imported_controller.build_role_arn.is_none()
        {
            // The registration replaces what setup owns, but not what this controller wrote at
            // runtime: the signing key's public half, the configuration it published and the
            // replica secret's readiness. Without the public half the key owner writes a new
            // signing key, which invalidates every session token and stored policy.
            if requires_gateway {
                imported_controller.egress_public_key = existing_controller.egress_public_key;
                imported_controller.egress_config_digest = existing_controller.egress_config_digest;
                imported_controller.egress_replica_secret_ready =
                    existing_controller.egress_replica_secret_ready;
            }
            return make_imported_state_with_status(imported_controller, ctx, imported.status);
        }
        let gateway = existing_controller
            .egress
            .as_ref()
            .and_then(|value| value.gateway())
            .cloned();
        let initialize_gateway = requires_gateway
            && existing_controller.egress_public_key.is_none()
            && existing_controller.get_status() == ResourceStatus::Running;
        let merged = AwsSandboxController {
            state: if initialize_gateway {
                AwsSandboxState::InitializingEgressKey
            } else {
                existing_controller.state
            },
            default_policy: ctx
                .resource
                .config
                .downcast_ref::<Sandbox>()
                .and_then(|sandbox| sandbox.default_policy.clone()),
            egress: if requires_gateway && gateway.is_some() {
                gateway.map(alien_core::bindings::SandboxEgressBinding::Gateway)
            } else {
                imported_controller.egress
            },
            egress_public_key: if requires_gateway {
                existing_controller.egress_public_key.clone()
            } else {
                None
            },
            build_role_arn: imported_controller
                .build_role_arn
                .or(existing_controller.build_role_arn.clone()),
            granted_bundle_uri: imported_controller
                .granted_bundle_uri
                .or(existing_controller.granted_bundle_uri.clone()),
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
            status: merged.get_status(),
            internal_state: Some(internal_state),
            outputs,
            remote_binding_params,
            lifecycle: Some(ctx.resource.lifecycle),
            ..existing
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        AwsManagementConfig, ManagementConfig, Resource, ResourceEntry, SandboxCode, SandboxEgress,
        SandboxLifecyclePolicy, StackSettings,
    };

    /// A setup update re-registers a template's Frozen internet sandbox with its new image. The
    /// image is setup's, but the signing key's public half, the published configuration's
    /// digest and the replica secret's readiness are what the controller wrote at runtime.
    /// Dropping the public half made the key owner write a new signing key on its next pass.
    #[test]
    fn a_frozen_internet_sandbox_reimport_keeps_its_signing_key() {
        let entry = ResourceEntry {
            config: Resource::new(
                Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: "manager.example.com/alien-artifacts-proj:base".to_string(),
                    })
                    .egress(SandboxEgress::Internet)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: Some(1800),
                        idle_pause_seconds: None,
                    })
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: vec![],
            added_by_alien: false,
            display_name: None,
            description: None,
            remote_access: true,
            enabled_when: None,
        };
        let settings = StackSettings::default();
        let management = ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "arn:aws:iam::123456789012:role/alien-manager".to_string(),
        });
        let ctx = ImportContext {
            resource_id: "agents",
            platform: Platform::Aws,
            region: "us-east-2",
            stack_settings: &settings,
            management_config: Some(&management),
            resource: &entry,
        };
        let frozen = |version: &str| AwsSandboxImportData {
            image_identifier: Some(
                "arn:aws:lambda:us-east-2:123456789012:microvm-image:stack-agents".to_string(),
            ),
            image_arn: Some(
                "arn:aws:lambda:us-east-2:123456789012:microvm-image:stack-agents".to_string(),
            ),
            image_version: Some(version.to_string()),
            build_role_arn: None,
            bundle_uri: None,
            egress_connector_arns: vec![
                "arn:aws:lambda:us-east-2:123456789012:network-connector:nc-1".to_string(),
            ],
            allow_egress: false,
            preview_ports: vec![],
        };
        let importer = AwsSandboxImporter;
        let existing = importer.import(frozen("1.0"), &ctx).expect("first import");
        let mut existing_internal = existing.internal_state.clone().unwrap();
        existing_internal["state"] = serde_json::json!("ready");
        existing_internal["egressPublicKey"] =
            serde_json::json!({"kid": "kid-in-use", "publicKey": "public-key-in-use"});
        existing_internal["egressConfigDigest"] = serde_json::json!("digest-published");
        existing_internal["egressReplicaSecretReady"] = serde_json::json!(true);
        let existing = StackResourceState {
            internal_state: Some(existing_internal),
            status: ResourceStatus::Running,
            ..existing
        };
        let imported = importer.import(frozen("2.0"), &ctx).expect("re-import");

        let merged = importer
            .merge_reimport(existing, imported, &ctx)
            .expect("merge should succeed");

        let internal = merged.internal_state.unwrap();
        assert_eq!(internal["activeVersion"], "2.0", "setup owns the image");
        assert_eq!(
            internal["egressPublicKey"],
            serde_json::json!({"kid": "kid-in-use", "publicKey": "public-key-in-use"}),
            "the key owner must keep the key every session token is signed with"
        );
        assert_eq!(internal["egressConfigDigest"], "digest-published");
        assert_eq!(internal["egressReplicaSecretReady"], true);
    }
}
