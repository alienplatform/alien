//! Importers for the GCP Agent Platform sandbox and its reasoning engine.

use alien_core::{
    import::{
        data::{GcpAgentPlatformEngineImportData, GcpSandboxImportData},
        ImportContext,
    },
    ResourceStatus, Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::{make_imported_state, make_imported_state_with_status};
use crate::sandbox::{
    GcpAgentPlatformEngineController, GcpAgentPlatformEngineState,
    GcpAgentPlatformTemplateController, GcpAgentPlatformTemplateState,
};

/// Last path segment of a resource name — the bare id the client interpolates back into its paths.
///
/// Terraform reports the engine as `projects/…/reasoningEngines/{id}`, and the client builds that
/// path itself, so a whole name stored here would be interpolated into a doubled path.
fn last_segment(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// Reasoning-engine importer.
///
/// Registered only for a Frozen engine, which the setup stack created and owns. It imports Ready:
/// there is no runtime-owned step left, and the template controller reads the id from here.
#[derive(Debug, Default)]
pub struct GcpAgentPlatformEngineImporter;

impl ResourceImporter for GcpAgentPlatformEngineImporter {
    type ImportData = GcpAgentPlatformEngineImportData;

    fn import(
        &self,
        data: GcpAgentPlatformEngineImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = GcpAgentPlatformEngineController {
            state: GcpAgentPlatformEngineState::Ready,
            engine_id: Some(last_segment(&data.engine_id).to_string()),
            pending_operation: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}

/// Sandbox importer.
///
/// The sandbox is the environment template, which a release owns under either lifecycle: setup
/// never creates one, so this imports at the create entry state and the deployment loop builds the
/// first template from the engine it depends on.
#[derive(Debug, Default)]
pub struct GcpSandboxImporter;

impl ResourceImporter for GcpSandboxImporter {
    type ImportData = GcpSandboxImportData;

    fn import(
        &self,
        data: GcpSandboxImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = GcpAgentPlatformTemplateController {
            state: GcpAgentPlatformTemplateState::CreateStart,
            region: Some(data.region),
            ..Default::default()
        };
        make_imported_state_with_status(controller, ctx, ResourceStatus::Provisioning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImporterRegistry;
    use alien_core::{
        GcpAgentPlatformEngine, Platform, Resource, ResourceEntry, ResourceLifecycle, Sandbox,
        SandboxCode, SandboxEgress, SandboxSessionPolicy, StackSettings,
    };

    fn import_context<'a>(
        settings: &'a StackSettings,
        entry: &'a ResourceEntry,
        resource_id: &'a str,
    ) -> ImportContext<'a> {
        ImportContext {
            resource_id,
            platform: Platform::Gcp,
            region: "us-central1",
            stack_settings: settings,
            management_config: None,
            resource: entry,
        }
    }

    fn entry(config: Resource, lifecycle: ResourceLifecycle) -> ResourceEntry {
        ResourceEntry {
            config,
            lifecycle,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    fn sandbox_config() -> Resource {
        Resource::new(
            Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "python:3.12".to_string(),
                })
                .egress(SandboxEgress::Deny)
                .session(SandboxSessionPolicy {
                    max_lifetime_seconds: None,
                    idle_suspend_seconds: None,
                })
                .build(),
        )
    }

    /// The imported id is the one the setup stack registered, reduced to the bare segment the
    /// client interpolates. A fabricated id, or a whole resource name left unreduced, builds a
    /// path that no engine answers — and the first session, not the install, is where that shows.
    #[test]
    fn the_engine_importer_seeds_ready_with_the_id_from_the_ref() {
        let settings = StackSettings::default();
        let engine = entry(
            Resource::new(GcpAgentPlatformEngine::new("agents-engine".to_string()).build()),
            ResourceLifecycle::Frozen,
        );
        let ctx = import_context(&settings, &engine, "agents-engine");

        let state = GcpAgentPlatformEngineImporter
            .import(
                GcpAgentPlatformEngineImportData {
                    engine_id: "projects/p/locations/us-central1/reasoningEngines/7788990011"
                        .to_string(),
                },
                &ctx,
            )
            .expect("a frozen engine imports");

        assert_eq!(state.status, ResourceStatus::Running);
        let controller: GcpAgentPlatformEngineController = serde_json::from_value(
            state
                .internal_state
                .expect("the importer records controller state"),
        )
        .expect("the engine controller state parses");
        assert_eq!(controller.engine_id.as_deref(), Some("7788990011"));
        assert!(matches!(
            controller.state,
            GcpAgentPlatformEngineState::Ready
        ));
    }

    /// The environment template is release-owned whoever owns the engine, so the sandbox imports at
    /// the create entry rather than Ready: importing Ready would publish a binding to a template
    /// nothing has built.
    #[test]
    fn the_sandbox_importer_seeds_the_template_at_its_create_entry() {
        let settings = StackSettings::default();
        let sandbox = entry(sandbox_config(), ResourceLifecycle::Frozen);
        let ctx = import_context(&settings, &sandbox, "agents");

        let state = GcpSandboxImporter
            .import(
                GcpSandboxImportData {
                    region: "us-central1".to_string(),
                },
                &ctx,
            )
            .expect("a sandbox imports");

        assert_eq!(state.status, ResourceStatus::Provisioning);
        let controller: GcpAgentPlatformTemplateController = serde_json::from_value(
            state
                .internal_state
                .expect("the importer records controller state"),
        )
        .expect("the template controller state parses");
        assert!(matches!(
            controller.state,
            GcpAgentPlatformTemplateState::CreateStart
        ));
        assert_eq!(controller.template_id, None);
    }

    /// Both halves of a GCP sandbox must be registered, or registration of the generated package
    /// fails with `ImportRegistrationMissing` for whichever one is absent.
    #[test]
    fn both_gcp_sandbox_resources_have_a_registered_importer() {
        let registry = ImporterRegistry::built_in();
        for resource_type in [
            Sandbox::RESOURCE_TYPE,
            GcpAgentPlatformEngine::RESOURCE_TYPE,
        ] {
            assert!(
                registry.importer(&resource_type, Platform::Gcp).is_some(),
                "no importer registered for ({resource_type}, gcp)"
            );
        }
    }
}
