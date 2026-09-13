//! Importer for the Azure Sandbox.

use alien_core::{
    import::{data::AzureSandboxImportData, ImportContext},
    ErrorData as CoreErrorData, Platform, ResourceStatus, Result, Sandbox, StackResourceState,
};
use alien_error::AlienError;

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state_with_status;
use crate::sandbox::{AzureSandboxController, AzureSandboxState};

/// Azure Sandbox importer — for the sandbox group the setup package created.
///
/// It arrives already provisioned, so it imports at `Ready`; the controller reads and heartbeats
/// it and never creates or deletes one.
///
/// All three fields are required: the ADC data plane endpoint is per-region and its paths are
/// scoped by resource group, so a group imported without either cannot be addressed at all.
#[derive(Debug, Default)]
pub struct AzureSandboxImporter;

impl ResourceImporter for AzureSandboxImporter {
    type ImportData = AzureSandboxImportData;

    fn import(
        &self,
        data: AzureSandboxImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // The import data does not carry the image, egress or idle-pause; they come from the
        // declaration, so an imported group publishes a complete binding at import rather than
        // waiting for the first reconcile.
        let sandbox = ctx
            .resource
            .config
            .downcast_ref::<Sandbox>()
            .ok_or_else(|| {
                AlienError::new(CoreErrorData::ImportDataInvalid {
                    resource_id: ctx.resource_id.to_string(),
                    resource_type: Sandbox::RESOURCE_TYPE,
                    platform: Platform::Azure,
                })
            })?;
        let controller = AzureSandboxController {
            state: AzureSandboxState::Ready,
            sandbox_group: Some(data.sandbox_group),
            region: Some(data.region),
            resource_group: Some(data.resource_group),
            disk_image: Some(sandbox.azure_catalog_image()?.to_string()),
            egress: Some(sandbox.egress.clone()),
            idle_pause_seconds: sandbox.lifecycle.idle_pause_seconds,
            _internal_stay_count: None,
        };
        make_imported_state_with_status(controller, ctx, ResourceStatus::Running)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Resource, ResourceEntry, ResourceLifecycle, SandboxCode, SandboxEgress,
        SandboxLifecyclePolicy, StackSettings, ToolchainConfig,
    };

    fn entry(config: Resource) -> ResourceEntry {
        ResourceEntry {
            config,
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    fn import_context<'a>(
        settings: &'a StackSettings,
        resource: &'a ResourceEntry,
    ) -> ImportContext<'a> {
        ImportContext {
            resource_id: "sbx",
            platform: Platform::Azure,
            region: "westus2",
            stack_settings: settings,
            management_config: None,
            resource,
        }
    }

    fn import_data() -> AzureSandboxImportData {
        AzureSandboxImportData {
            sandbox_group: "sbg".to_string(),
            region: "westus2".to_string(),
            resource_group: "rg".to_string(),
        }
    }

    /// The image, egress and idle-pause are not in the Azure import data — they come from the
    /// declaration — so the import must capture them into state, or the binding stays incomplete
    /// until a reconcile. Pinning each keeps that capture from silently regressing.
    #[test]
    fn azure_sandbox_import_captures_the_session_fields_from_the_declaration() {
        let resource = entry(Resource::new(
            Sandbox::new("sbx".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu".to_string(),
                })
                .egress(SandboxEgress::Deny)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: Some(300),
                })
                .build(),
        ));
        let settings = StackSettings::default();
        let ctx = import_context(&settings, &resource);

        let imported = AzureSandboxImporter
            .import(import_data(), &ctx)
            .expect("the sandbox import should succeed");

        let internal = imported
            .internal_state
            .expect("imported sandbox should have controller state");
        assert_eq!(internal["sandboxGroup"], "sbg");
        assert_eq!(internal["diskImage"], "ubuntu");
        assert_eq!(internal["egress"]["mode"], "deny");
        assert_eq!(internal["idlePauseSeconds"], 300);
    }

    /// Azure creates a sandbox only from a catalog image, so a source-built sandbox is refused at
    /// import rather than reaching the caller later as an invalid binding.
    #[test]
    fn azure_sandbox_import_refuses_a_source_sandbox() {
        let resource = entry(Resource::new(
            Sandbox::new("sbx".to_string())
                .code(SandboxCode::Source {
                    src: "./sandbox".to_string(),
                    toolchain: ToolchainConfig::Docker {
                        dockerfile: None,
                        build_args: None,
                        target: None,
                    },
                })
                .egress(SandboxEgress::Deny)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
        ));
        let settings = StackSettings::default();
        let ctx = import_context(&settings, &resource);

        AzureSandboxImporter
            .import(import_data(), &ctx)
            .expect_err("a source-built sandbox has no catalog image to bind");
    }
}
