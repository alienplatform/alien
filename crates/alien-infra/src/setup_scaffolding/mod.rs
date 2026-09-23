//! Cloud objects a direct setup creates for resources a runtime controller owns. The runtime
//! identity may use them but is never granted what creating them takes, so they are made during
//! InitialSetup, the only time Alien holds the deployer's administrator credentials.

use std::collections::BTreeMap;

use alien_core::{
    ownership_policy_for_resource_type, ClientConfig, Platform, ResourceEntry, SetupScaffolding,
    Stack,
};

use crate::{PlatformServiceProvider, Result};

#[cfg(feature = "aws")]
mod aws_sandbox;

/// The credentials a scaffolding step acts with: setup's, never the runtime identity's.
pub struct SetupScaffoldingContext<'a> {
    pub client_config: &'a ClientConfig,
    pub service_provider: &'a dyn PlatformServiceProvider,
    pub resource_prefix: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaffoldingProgress {
    InProgress,
    Done,
}

/// Whether setup must create something for this resource without owning the resource itself.
pub fn needs_setup_scaffolding(entry: &ResourceEntry) -> bool {
    let policy = ownership_policy_for_resource_type(entry.config.resource_type().as_ref());
    policy.emits_setup_scaffolding(entry.lifecycle) && !policy.should_emit_in_setup(entry.lifecycle)
}

/// At most one mutating call per resource, so a retry after any failure repeats nothing.
pub async fn reconcile(
    ctx: &SetupScaffoldingContext<'_>,
    stack: &Stack,
    platform: Platform,
    records: &mut BTreeMap<String, SetupScaffolding>,
) -> Result<ScaffoldingProgress> {
    let mut progress = ScaffoldingProgress::Done;
    for (resource_id, entry) in stack.resources() {
        if !needs_setup_scaffolding(entry) {
            continue;
        }
        let step = match platform {
            #[cfg(feature = "aws")]
            Platform::Aws => match entry.config.downcast_ref::<alien_core::Sandbox>() {
                Some(sandbox) => {
                    aws_sandbox::reconcile(ctx, sandbox, entry.lifecycle, records).await?
                }
                None => continue,
            },
            // The template setups render the rest; the direct path has no step for them.
            _ => continue,
        };
        if step == ScaffoldingProgress::InProgress {
            tracing::info!(resource_id = %resource_id, "Setup scaffolding in progress");
            progress = ScaffoldingProgress::InProgress;
        }
    }
    Ok(progress)
}

/// A record is dropped only once its objects are gone, so a failed teardown keeps the remainder.
pub async fn teardown(
    ctx: &SetupScaffoldingContext<'_>,
    records: &mut BTreeMap<String, SetupScaffolding>,
) -> Result<()> {
    let resource_ids: Vec<String> = records.keys().cloned().collect();
    for resource_id in resource_ids {
        match &records[&resource_id] {
            #[cfg(feature = "aws")]
            SetupScaffolding::AwsSandbox { build_role_name } => {
                aws_sandbox::teardown(ctx, &resource_id, build_role_name).await?
            }
            #[cfg(not(feature = "aws"))]
            SetupScaffolding::AwsSandbox { .. } => {
                return Err(alien_error::AlienError::new(
                    crate::ErrorData::ControllerNotAvailable {
                        resource_type: alien_core::Sandbox::RESOURCE_TYPE,
                        platform: Platform::Aws,
                    },
                ))
            }
        }
        records.remove(&resource_id);
    }
    Ok(())
}
