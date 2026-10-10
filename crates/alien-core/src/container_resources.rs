//! Resolve deployment-time container allocations before planning or provisioning.

use crate::{
    instance_catalog::{parse_cpu, parse_memory_bytes},
    ComputeSettings, Container, ErrorData, ResourceChoiceRange, ResourceSpec, Stack,
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

fn invalid(id: &str, dimension: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ContainerResourceSelectionInvalid {
        resource_id: id.to_string(),
        dimension: dimension.to_string(),
        reason: reason.to_string(),
    })
}
