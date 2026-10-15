//! Resolve deployment-time container allocations before planning or provisioning.

use crate::{
    instance_catalog::{parse_cpu, parse_memory_bytes},
    ComputeSettings, Container, ErrorData, ResourceChoiceRange, ResourceSpec, Stack, StackState,
};
use alien_error::{AlienError, Result};

/// Apply and validate selections without changing the release's declared resource ranges.
/// The same resolver is used by compute planning and deployment preparation.
pub fn resolve_container_resources(
    stack: &mut Stack,
    settings: Option<&ComputeSettings>,
) -> Result<(), ErrorData> {
    if let Some(settings) = settings {
        let mut ids: Vec<_> = settings.containers.keys().collect();
        ids.sort();
        for id in ids {
            if !stack
                .resources
                .get(id)
                .is_some_and(|entry| entry.config.downcast_ref::<Container>().is_some())
            {
                return Err(invalid(
                    id,
                    "resources",
                    "no such container in this release",
                ));
            }
        }
    }

    let mut ids: Vec<_> = stack.resources.keys().cloned().collect();
    ids.sort();
    for id in ids {
        let entry = stack.resources.get_mut(&id).expect("id came from map");
        let Some(container) = entry.config.downcast_mut::<Container>() else {
            continue;
        };
        let selected = settings.and_then(|settings| settings.containers.get(&id));
        let cpu = selected
            .and_then(|selected| selected.cpu.as_ref())
            .map(ToString::to_string);
        let memory = selected.and_then(|selected| selected.memory.as_deref());
        if let Some(size) = selected.and_then(|selected| selected.volume_size_gib) {
            let storage = container.persistent_storage.as_mut().ok_or_else(|| {
                invalid(
                    &id,
                    "volumeSizeGib",
                    "the release declares no persistent volume",
                )
            })?;
            if size == 0 || size > 65536 {
                return Err(invalid(
                    &id,
                    "volumeSizeGib",
                    "capacity must be between 1 and 65536 GiB",
                ));
            }
            let selected = format!("{size}Gi");
            // Preserve equivalent release quantities, including immutable PVC templates.
            if parse_memory_bytes(&storage.size) != parse_memory_bytes(&selected) {
                storage.size = selected;
            }
        }
        let choices = container.resource_choices.as_ref();
        resolve_dimension(
            &id,
            "cpu",
            &mut container.cpu,
            choices.and_then(|choices| choices.cpu.as_ref()),
            cpu.as_deref(),
            parse_positive_cpu,
        )?;
        resolve_dimension(
            &id,
            "memory",
            &mut container.memory,
            choices.and_then(|choices| choices.memory.as_ref()),
            memory,
            parse_positive_memory,
        )?;
    }
    Ok(())
}

/// The persistent volume capacity, in GiB, that `state` records as installed for container `id`.
/// `None` when the container has no installed persistent storage or its size does not parse.
pub fn installed_volume_size_gib(state: &StackState, id: &str) -> Option<u32> {
    let storage = state
        .resources
        .get(id)?
        .config
        .downcast_ref::<Container>()?
        .persistent_storage
        .as_ref()?;
    parse_volume_size_gib(&storage.size).ok()
}

/// A setup-source volume size raised to the size the deployment already has or chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptVolumeSize {
    /// Container resource ID.
    pub container_id: String,
    /// Capacity the setup source asked for, in GiB.
    pub requested_gib: u32,
    /// Capacity kept instead, in GiB.
    pub kept_gib: u32,
}

/// Raise volume sizes from a setup source (a CloudFormation parameter, a Terraform variable or a
/// CLI config file) that are below an installed volume's capacity, or below the size the
/// deployment currently selects for it, to the larger of the two.
///
/// Volumes only grow, and a compute update can grow one without the setup source learning about
/// it, so a lower value from a setup source is stale, not a request to shrink. A container with
/// no installed volume is left alone: before anything is installed a smaller value is a real
/// correction. Returns the raised selections so callers can tell the user which values their
/// setup source should be updated to.
pub fn keep_volume_sizes_over_setup_source(
    requested: &mut ComputeSettings,
    installed: Option<&StackState>,
    current: Option<&ComputeSettings>,
) -> Vec<KeptVolumeSize> {
    let mut kept = Vec::new();
    let mut ids: Vec<_> = requested.containers.keys().cloned().collect();
    ids.sort();
    for id in ids {
        let selection = requested.containers.get_mut(&id).expect("id came from map");
        let Some(requested_gib) = selection.volume_size_gib else {
            continue;
        };
        let Some(installed_gib) = installed.and_then(|state| installed_volume_size_gib(state, &id))
        else {
            continue;
        };
        let chosen_gib = current
            .and_then(|current| current.containers.get(&id))
            .and_then(|selection| selection.volume_size_gib);
        let kept_gib = chosen_gib.map_or(installed_gib, |chosen| chosen.max(installed_gib));
        if requested_gib < kept_gib {
            selection.volume_size_gib = Some(kept_gib);
            kept.push(KeptVolumeSize {
                container_id: id,
                requested_gib,
                kept_gib,
            });
        }
    }
    kept
}

/// Keep installed volumes at their installed capacity when the deployment selects no size and
/// the release default is smaller. Without a selection nobody asked for a size, so a release
/// that lowers its default must not turn into a shrink of every installed deployment. Explicit
/// selections are left to the planner's shrink check.
pub fn retain_installed_volume_sizes(
    stack: &mut Stack,
    settings: Option<&ComputeSettings>,
    installed: &StackState,
) {
    for (id, entry) in stack.resources.iter_mut() {
        if settings
            .and_then(|settings| settings.containers.get(id))
            .is_some_and(|selection| selection.volume_size_gib.is_some())
        {
            continue;
        }
        let Some(storage) = entry
            .config
            .downcast_mut::<Container>()
            .and_then(|container| container.persistent_storage.as_mut())
        else {
            continue;
        };
        let Some(installed_storage) = installed
            .resources
            .get(id)
            .and_then(|resource| resource.config.downcast_ref::<Container>())
            .and_then(|container| container.persistent_storage.as_ref())
        else {
            continue;
        };
        if let (Ok(release_gib), Ok(installed_gib)) = (
            parse_volume_size_gib(&storage.size),
            parse_volume_size_gib(&installed_storage.size),
        ) {
            if release_gib < installed_gib {
                storage.size = installed_storage.size.clone();
            }
        }
    }
}

fn resolve_dimension<T: PartialOrd>(
    id: &str,
    dimension: &str,
    spec: &mut ResourceSpec,
    choices: Option<&ResourceChoiceRange>,
    selected: Option<&str>,
    parse: fn(&str) -> std::result::Result<T, String>,
) -> Result<(), ErrorData> {
    let parse_value = |value: &str| parse(value).map_err(|reason| invalid(id, dimension, &reason));
    let Some(choices) = choices else {
        if let Some(selected) = selected {
            if parse_value(selected)? != parse_value(&spec.desired)? {
                return Err(invalid(
                    id,
                    dimension,
                    "the release declares a fixed allocation",
                ));
            }
        }
        return Ok(());
    };
    let min = parse_value(&choices.min)?;
    let max = parse_value(&choices.max)?;
    let default = parse_value(&choices.default)?;
    if min > max || default < min || default > max {
        return Err(invalid(
            id,
            dimension,
            "range must satisfy min <= default <= max",
        ));
    }
    let value = selected.unwrap_or(&choices.default);
    let allocation = parse_value(value)?;
    if allocation < min || allocation > max {
        return Err(invalid(
            id,
            dimension,
            &format!(
                "'{value}' is outside the allowed range {}..{}",
                choices.min, choices.max,
            ),
        ));
    }
    // Both scheduling reservation and runtime limit reflect the deployer's allocation.
    spec.min = value.to_string();
    spec.desired = value.to_string();
    Ok(())
}

fn parse_positive_cpu(value: &str) -> std::result::Result<f64, String> {
    let cpu = parse_cpu(value)?;
    if !cpu.is_finite() || cpu <= 0.0 {
        return Err(format!("CPU must be finite and positive, got '{value}'"));
    }
    Ok(cpu)
}

fn parse_positive_memory(value: &str) -> std::result::Result<u64, String> {
    // Keep the public contract in binary units and reject negative/non-finite values
    // before the catalog's float-to-integer conversion can saturate them.
    let numeric = ["Ki", "Mi", "Gi", "Ti"]
        .iter()
        .find_map(|suffix| value.strip_suffix(suffix))
        .ok_or_else(|| format!("Memory must use Ki, Mi, Gi, or Ti, got '{value}'"))?;
    let amount: f64 = numeric
        .parse()
        .map_err(|_| format!("Invalid memory '{value}'"))?;
    if !amount.is_finite() || amount <= 0.0 {
        return Err(format!("Memory must be finite and positive, got '{value}'"));
    }
    let bytes = parse_memory_bytes(value)?;
    if bytes == 0 || bytes == u64::MAX {
        return Err(format!("Memory is outside the supported range: '{value}'"));
    }
    Ok(bytes)
}

pub(crate) fn invalid(id: &str, dimension: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ContainerResourceSelectionInvalid {
        resource_id: id.to_string(),
        dimension: dimension.to_string(),
        reason: reason.to_string(),
    })
}

/// Parse whole-volume capacities used by cloud block disks. Legacy GB/TB
/// spellings retain their existing GiB allocation semantics.
pub fn parse_volume_size_gib(value: &str) -> std::result::Result<u32, String> {
    let value = value.trim();
    // Keep historical cloud GB/TB semantics while accepting existing Kubernetes
    // quantities. Cloud disks allocate whole GiB, so round release quantities up;
    // deployment selections themselves are unsigned integer GiB.
    let bytes = if let Some(number) = value.strip_suffix("TB") {
        parse_memory_bytes(&format!("{number}Gi"))?
            .checked_mul(1000)
            .ok_or_else(|| format!("Volume capacity '{value}' is too large"))?
    } else if let Some(number) = value
        .strip_suffix("GB")
        .or_else(|| value.strip_suffix("GiB"))
    {
        parse_memory_bytes(&format!("{number}Gi"))?
    } else if let Some(number) = value.strip_suffix("TiB") {
        parse_memory_bytes(&format!("{number}Ti"))?
    } else {
        parse_memory_bytes(value)?
    };
    let gib = bytes.div_ceil(1024 * 1024 * 1024);
    u32::try_from(gib)
        .ok()
        .filter(|n| (1..=65536).contains(n))
        .ok_or_else(|| format!("Volume capacity '{value}' must be between 1 and 65536 GiB"))
}
