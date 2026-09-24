//! Cloud objects a direct setup creates for resources a runtime controller owns. The runtime
//! identity may use them but is never granted what creating them takes, so they are made during
//! InitialSetup, the only time Alien holds the deployer's administrator credentials.

use std::collections::BTreeMap;

use alien_core::{
    import::ImportContext, ownership_policy_for_resource_type, ClientConfig, ManagementConfig,
    Platform, ResourceEntry, SetupScaffolding, Stack, StackSettings, StackState,
};
use alien_error::{AlienError, Context};

use crate::{ErrorData, ImporterRegistry, PlatformServiceProvider, Result};

#[cfg(feature = "aws")]
mod aws_sandbox;
#[cfg(feature = "aws")]
mod aws_sandbox_egress;

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
    stack_state: &StackState,
    records: &mut BTreeMap<String, SetupScaffolding>,
) -> Result<ScaffoldingProgress> {
    let mut progress = ScaffoldingProgress::Done;
    for (resource_id, entry, scaffolded) in scaffolded(stack, stack_state.platform) {
        let step = match scaffolded {
            #[cfg(feature = "aws")]
            Scaffolded::AwsSandbox(sandbox) => {
                aws_sandbox::reconcile(ctx, stack, stack_state, sandbox, entry.lifecycle, records)
                    .await?
            }
            #[cfg(not(feature = "aws"))]
            Scaffolded::AwsSandbox(_) => return Err(aws_not_built()),
        };
        if step == ScaffoldingProgress::InProgress {
            tracing::info!(resource_id = %resource_id, "Setup scaffolding in progress");
            progress = ScaffoldingProgress::InProgress;
        }
    }
    Ok(progress)
}

/// Why an update from `installed` to `target` needs setup to run first: a scaffolded resource that
/// is new, or whose scaffolding would change. An update acts with the runtime identity, which is
/// never granted what creating or changing scaffolding takes.
pub fn changes_requiring_setup(
    client_config: &ClientConfig,
    installed: &Stack,
    target: &Stack,
    platform: Platform,
) -> Result<Vec<String>> {
    let mut changes = Vec::new();
    for (resource_id, entry, scaffolded) in scaffolded(target, platform) {
        let installed_entry = installed
            .resources
            .get(resource_id)
            .filter(|installed_entry| needs_setup_scaffolding(installed_entry));
        match scaffolded {
            #[cfg(feature = "aws")]
            Scaffolded::AwsSandbox(sandbox) => {
                let Some((installed_sandbox, installed_lifecycle)) =
                    installed_entry.and_then(|installed_entry| {
                        Some((
                            installed_entry
                                .config
                                .downcast_ref::<alien_core::Sandbox>()?,
                            installed_entry.lifecycle,
                        ))
                    })
                else {
                    changes.push(format!(
                        "sandbox '{resource_id}' is new, and setup creates its build role"
                    ));
                    continue;
                };
                let before = aws_sandbox::setup_inputs(
                    client_config,
                    installed,
                    installed_sandbox,
                    installed_lifecycle,
                )?;
                let after =
                    aws_sandbox::setup_inputs(client_config, target, sandbox, entry.lifecycle)?;
                for ((name, was), (_, is)) in before.iter().zip(&after) {
                    if was != is {
                        changes.push(format!("sandbox '{resource_id}' changes its {name}"));
                    }
                }
            }
            #[cfg(not(feature = "aws"))]
            Scaffolded::AwsSandbox(_) => return Err(aws_not_built()),
        }
    }
    Ok(changes)
}

/// The resources a direct setup scaffolds on `platform`; the template setups render the rest.
fn scaffolded(
    stack: &Stack,
    platform: Platform,
) -> impl Iterator<Item = (&String, &ResourceEntry, Scaffolded<'_>)> {
    stack.resources().filter_map(move |(resource_id, entry)| {
        if !needs_setup_scaffolding(entry) {
            return None;
        }
        let scaffolded = match platform {
            Platform::Aws => Scaffolded::AwsSandbox(entry.config.downcast_ref()?),
            _ => return None,
        };
        Some((resource_id, entry, scaffolded))
    })
}

enum Scaffolded<'a> {
    AwsSandbox(&'a alien_core::Sandbox),
}

#[cfg(not(feature = "aws"))]
fn aws_not_built() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ControllerNotAvailable {
        resource_type: alien_core::Sandbox::RESOURCE_TYPE,
        platform: Platform::Aws,
    })
}

/// State a scaffolded resource's controller starts from, as its registered importer reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScaffoldingSeed {
    pub resource_id: String,
    pub platform: Platform,
    pub region: String,
    pub import_data: serde_json::Value,
}

/// The seeds of a stack whose [`reconcile`] reported Done. Reads only the records, so repeating it
/// after a crash yields the same seeds.
pub fn seeds(
    ctx: &SetupScaffoldingContext<'_>,
    stack: &Stack,
    stack_state: &StackState,
    records: &BTreeMap<String, SetupScaffolding>,
) -> Result<Vec<ScaffoldingSeed>> {
    scaffolded(stack, stack_state.platform)
        .map(|(_, _, scaffolded)| match scaffolded {
            #[cfg(feature = "aws")]
            Scaffolded::AwsSandbox(sandbox) => aws_sandbox::seed(ctx, sandbox, records),
            #[cfg(not(feature = "aws"))]
            Scaffolded::AwsSandbox(_) => Err(aws_not_built()),
        })
        .collect()
}

/// Where the seeded resources' importers get their stack-wide facts.
pub struct SeedContext<'a> {
    pub registry: &'a ImporterRegistry,
    pub stack_settings: &'a StackSettings,
    pub management_config: Option<&'a ManagementConfig>,
}

/// Registers each seed as a setup import would. A resource that already has state is merged, not
/// replaced, so a setup that runs again leaves a runtime-owned resource's progress in place.
pub fn apply_seeds(
    ctx: &SeedContext<'_>,
    stack: &Stack,
    stack_state: &mut StackState,
    seeds: Vec<ScaffoldingSeed>,
) -> Result<()> {
    for seed in seeds {
        let resource_id = seed.resource_id.as_str();
        let entry = stack.resources.get(resource_id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "a setup scaffolding seed names a resource the stack does not declare"
                    .to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })?;
        let resource_type = entry.config.resource_type();
        let import_ctx = ImportContext {
            resource_id,
            platform: seed.platform,
            region: &seed.region,
            stack_settings: ctx.stack_settings,
            management_config: ctx.management_config,
            resource: entry,
        };
        // An importer's answer is fixed by its inputs, so none of these clears on a retry.
        let failed = |message: &str| ErrorData::ImportedSetupStateInvalid {
            message: message.to_string(),
            resource_id: Some(resource_id.to_string()),
        };
        let mut imported = ctx
            .registry
            .run(&resource_type, seed.platform, seed.import_data, &import_ctx)
            .context(failed("the registered importer refused the seed"))?;
        let seeded = match stack_state.resources.get(resource_id).cloned() {
            None => {
                imported.controller_platform = Some(seed.platform);
                imported
            }
            Some(existing) => {
                if existing.resource_type != imported.resource_type
                    || existing
                        .controller_platform
                        .is_some_and(|platform| platform != seed.platform)
                {
                    return Err(AlienError::new(failed(
                        "the existing state belongs to another resource type or platform",
                    )));
                }
                let mut merged = ctx
                    .registry
                    .merge_reimport(
                        &resource_type,
                        seed.platform,
                        existing,
                        imported,
                        &import_ctx,
                    )
                    .context(failed("the existing state cannot take the seed"))?;
                merged.dependencies = entry.combined_dependencies();
                merged
            }
        };
        stack_state
            .resources
            .insert(resource_id.to_string(), seeded);
    }
    Ok(())
}

pub fn scaffolds_any(stack: &Stack, platform: Platform) -> bool {
    scaffolded(stack, platform).next().is_some()
}

/// Adds to the record whatever setup created that it does not hold, found by name and the tags
/// setup creates each object with. A step whose checkpoint never landed, from a crash or a later
/// error in the same step, usually leaves an earlier record behind rather than none, so each object
/// is looked up on its own; what the record already holds is kept.
pub async fn recover_unrecorded(
    ctx: &SetupScaffoldingContext<'_>,
    stack: &Stack,
    platform: Platform,
    records: &mut BTreeMap<String, SetupScaffolding>,
) -> Result<()> {
    for (resource_id, _, scaffolded) in scaffolded(stack, platform) {
        let recovered = match scaffolded {
            #[cfg(feature = "aws")]
            Scaffolded::AwsSandbox(sandbox) => aws_sandbox::recover(ctx, sandbox).await?,
            #[cfg(not(feature = "aws"))]
            Scaffolded::AwsSandbox(_) => return Err(aws_not_built()),
        };
        let Some(recovered) = recovered else {
            continue;
        };
        match records.get_mut(resource_id) {
            Some(record) => record.fill_missing(recovered),
            None => {
                records.insert(resource_id.clone(), recovered);
            }
        }
    }
    Ok(())
}

/// A record is dropped only once its objects are gone, so a failed or unfinished teardown keeps
/// the remainder for the next call.
pub async fn teardown(
    ctx: &SetupScaffoldingContext<'_>,
    records: &mut BTreeMap<String, SetupScaffolding>,
) -> Result<ScaffoldingProgress> {
    let mut progress = ScaffoldingProgress::Done;
    let resource_ids: Vec<String> = records.keys().cloned().collect();
    for resource_id in resource_ids {
        let step = match records.get_mut(&resource_id) {
            #[cfg(feature = "aws")]
            Some(SetupScaffolding::AwsSandbox {
                build_role_name,
                egress,
            }) => aws_sandbox::teardown(ctx, &resource_id, build_role_name, egress).await?,
            #[cfg(not(feature = "aws"))]
            Some(SetupScaffolding::AwsSandbox { .. }) => return Err(aws_not_built()),
            None => continue,
        };
        match step {
            ScaffoldingProgress::Done => {
                records.remove(&resource_id);
            }
            ScaffoldingProgress::InProgress => progress = ScaffoldingProgress::InProgress,
        }
    }
    Ok(progress)
}
