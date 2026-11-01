//! Platform actions on an existing deployment: compute, environment variables,
//! stack inputs, health checks, machines, and machine join tokens.

use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use alien_core::{ComputePoolSelection, ComputeSettings, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types::{
    DeploymentDetailResponse, DrainMachinesMachineRequest, EnvironmentVariableConfig,
    UpdateDeploymentComputeRequest, UpdateDeploymentEnvironmentVariablesRequest,
    UpdateDeploymentInputsRequest,
};
use alien_platform_api::SdkResultExt as _;
use serde_json::{Map, Value};

use crate::error::{ErrorData, Result};
use crate::interaction::ConfirmationMode;
use crate::output::{print_json, prompt_confirm};
use crate::ui::{command, dim_label, make_table, print_table, success_line};

/// A requested change to one machine pool. Unset fields keep the pool's current value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PoolChange {
    pub pool: Option<String>,
    pub machines: Option<u32>,
    pub min: Option<u32>,
    pub max: Option<u32>,
    pub machine: Option<String>,
}

impl PoolChange {
    fn is_empty(&self) -> bool {
        self.machines.is_none()
            && self.min.is_none()
            && self.max.is_none()
            && self.machine.is_none()
    }
}

/// Compute changes for `alien deployments set-compute`.
#[derive(Debug, Clone, Default)]
pub(crate) struct ComputeChange {
    pub pool: PoolChange,
    /// `None` keeps the saved overrides; `Some` replaces them (empty clears them).
    pub quota_warning_overrides: Option<Vec<String>>,
}

fn detail_json(deployment: &DeploymentDetailResponse) -> Result<Value> {
    serde_json::to_value(deployment)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "deployment detail".to_string(),
        })
}

fn request_body<T: serde::de::DeserializeOwned>(value: Value, field: &str) -> Result<T> {
    serde_json::from_value(value)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: field.to_string(),
            message: "The request doesn't match the Platform API schema".to_string(),
        })
}

// ---------------------------------------------------------------------------
// Compute
// ---------------------------------------------------------------------------

fn pool_list(compute: &ComputeSettings) -> String {
    let mut ids: Vec<&str> = compute.pools.keys().map(String::as_str).collect();
    ids.sort_unstable();
    ids.join(", ")
}

fn select_pool(compute: &ComputeSettings, requested: Option<&str>) -> Result<String> {
    let invalid = |message: String| {
        AlienError::new(ErrorData::ValidationError {
            field: "pool".to_string(),
            message,
        })
    };
    if let Some(pool) = requested {
        if compute.pools.contains_key(pool) {
            return Ok(pool.to_string());
        }
        return Err(invalid(format!(
            "Pool '{pool}' is not in this deployment's compute. Pools: {}",
            pool_list(compute)
        )));
    }
    match compute.pools.len() {
        0 => Err(invalid(
            "This deployment has no stored compute pool choices. Choose compute pools \
             during deployment setup; rerunning `alien deploy` alone doesn't save them."
                .to_string(),
        )),
        1 => Ok(compute.pools.keys().next().expect("one pool").clone()),
        _ => Err(invalid(format!(
            "This deployment has several machine pools ({}). Pass --pool.",
            pool_list(compute)
        ))),
    }
}

/// Applies `change` to one pool of `current` and returns the selected pool and
/// the full compute settings to send.
///
/// A compute update replaces the whole pool map, so every other pool is sent
/// back unchanged. The pool's failure-domain placement is kept: it is fixed
/// once machines exist.
pub(crate) fn apply_pool_change(
    current: Option<&ComputeSettings>,
    change: &PoolChange,
) -> Result<(String, ComputeSettings)> {
    let mut compute = current.cloned().unwrap_or_default();
    let pool_id = select_pool(&compute, change.pool.as_deref())?;
    let selection = compute.pools[&pool_id].clone();
    let (current_machine, failure_domains) = match &selection {
        ComputePoolSelection::Fixed {
            machine,
            failure_domains,
            ..
        }
        | ComputePoolSelection::Autoscale {
            machine,
            failure_domains,
            ..
        } => (machine.clone(), failure_domains.clone()),
    };
    let machine = change.machine.clone().or(current_machine);
    let invalid = |field: &str, message: String| {
        AlienError::new(ErrorData::ValidationError {
            field: field.to_string(),
            message,
        })
    };

    let next = match (change.machines, change.min, change.max, &selection) {
        (Some(_), Some(_), _, _) | (Some(_), _, Some(_), _) => {
            return Err(invalid(
                "machines",
                "Pass --machines for a fixed pool, or --min and --max for an autoscaling pool, not both."
                    .to_string(),
            ));
        }
        (Some(machines), None, None, _) => ComputePoolSelection::Fixed {
            machines,
            machine,
            failure_domains,
        },
        (None, None, None, ComputePoolSelection::Fixed { machines, .. }) => {
            ComputePoolSelection::Fixed {
                machines: *machines,
                machine,
                failure_domains,
            }
        }
        (
            None,
            min,
            max,
            ComputePoolSelection::Autoscale {
                min: cur_min,
                max: cur_max,
                ..
            },
        ) => ComputePoolSelection::Autoscale {
            min: min.unwrap_or(*cur_min),
            max: max.unwrap_or(*cur_max),
            machine,
            failure_domains,
        },
        (None, Some(min), Some(max), ComputePoolSelection::Fixed { .. }) => {
            ComputePoolSelection::Autoscale {
                min,
                max,
                machine,
                failure_domains,
            }
        }
        (None, _, _, ComputePoolSelection::Fixed { .. }) => {
            return Err(invalid(
                "min",
                format!(
                    "Pool '{pool_id}' has a fixed machine count. Pass both --min and --max to make it autoscale."
                ),
            ));
        }
    };
    if let ComputePoolSelection::Autoscale { min, max, .. } = &next {
        if min > max {
            return Err(invalid(
                "min",
                format!("--min ({min}) must not be greater than --max ({max})."),
            ));
        }
    }
    compute.pools.insert(pool_id.clone(), next);
    Ok((pool_id, compute))
}

fn saved_compute(detail: &Value) -> Result<Option<ComputeSettings>> {
    match detail.pointer("/stackSettings/compute") {
        None | Some(Value::Null) => Ok(None),
        Some(compute) => serde_json::from_value(compute.clone())
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "parse".to_string(),
                reason: "the deployment's saved compute settings".to_string(),
            })
            .map(Some),
    }
}

fn machine_count(count: u32) -> String {
    if count == 1 {
        "1 machine".to_string()
    } else {
        format!("{count} machines")
    }
}

fn describe_pool(selection: &ComputePoolSelection) -> String {
    let (size, machine) = match selection {
        ComputePoolSelection::Fixed {
            machines, machine, ..
        } => (format!("{} (fixed)", machine_count(*machines)), machine),
        ComputePoolSelection::Autoscale {
            min, max, machine, ..
        } => (format!("{min}-{max} machines (autoscale)"), machine),
    };
    match machine {
        Some(machine) => format!("{size}, {machine}"),
        None => size,
    }
}

/// Rejects quota identities the deployment's platform doesn't know, as
/// `alien deploy` does, so a typo fails here instead of in the update.
fn validate_quota_warning_overrides(detail: &Value, overrides: &[String]) -> Result<()> {
    let platform = detail
        .get("platform")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let platform = Platform::from_str(platform).map_err(|message| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message,
        })
    })?;
    alien_preflights::quotas::validate_warning_overrides(platform, overrides).context(
        ErrorData::ValidationError {
            field: "quota-warning-override".to_string(),
            message: "Invalid quota warning override".to_string(),
        },
    )
}

/// Builds the compute update body.
///
/// Only the pool map is sent. The API keeps the saved container settings when
/// `containers` is absent, so a concurrent container change isn't reverted.
/// `Some(vec![])` for the overrides clears them.
fn compute_request(
    pools: Option<&HashMap<String, ComputePoolSelection>>,
    quota_warning_overrides: Option<Vec<String>>,
) -> Result<UpdateDeploymentComputeRequest> {
    let mut body = Map::new();
    if let Some(pools) = pools {
        body.insert("compute".to_string(), serde_json::json!({ "pools": pools }));
    }
    if let Some(overrides) = quota_warning_overrides {
        body.insert(
            "quotaWarningOverrides".to_string(),
            serde_json::json!(overrides),
        );
    }
    request_body(Value::Object(body), "compute")
}

pub(crate) async fn set_compute_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment: &DeploymentDetailResponse,
    change: ComputeChange,
    confirmation: ConfirmationMode,
    json: bool,
) -> Result<()> {
    let deployment_id = deployment.id.as_str();
    if change.pool.is_empty() && change.quota_warning_overrides.is_none() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "compute".to_string(),
            message: "Nothing to change. Pass --machines, --min/--max, --machine, \
                      --quota-warning-override, or --clear-quota-warning-overrides."
                .to_string(),
        }));
    }
    if change.pool.is_empty() && change.pool.pool.is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "pool".to_string(),
            message: "--pool needs --machines, --min/--max, or --machine.".to_string(),
        }));
    }

    let detail = detail_json(deployment)?;
    let mut changed_pool = None;
    let mut pools_to_send = None;
    if !change.pool.is_empty() {
        let current = saved_compute(&detail)?;
        let (pool_id, compute) = apply_pool_change(current.as_ref(), &change.pool)?;
        if matches!(confirmation, ConfirmationMode::Prompt)
            && !prompt_confirm(
                &format!("Scale pool '{pool_id}' to 0 machines? Its workloads stop running."),
                false,
            )?
        {
            println!("{}", dim_label("Nothing changed."));
            return Ok(());
        }
        changed_pool = Some((pool_id.clone(), describe_pool(&compute.pools[&pool_id])));
        pools_to_send = Some(compute.pools);
    }
    if let Some(overrides) = &change.quota_warning_overrides {
        validate_quota_warning_overrides(&detail, overrides)?;
    }
    let body = compute_request(
        pools_to_send.as_ref(),
        change.quota_warning_overrides.clone(),
    )?;

    let response = client
        .update_deployment_compute()
        .id(deployment_id)
        .workspace(workspace)
        .body(body)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("updating compute for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();
    if json {
        return print_json(&response);
    }
    let response =
        serde_json::to_value(&response)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialize".to_string(),
                reason: "the compute update response".to_string(),
            })?;
    if response.get("outcome").and_then(Value::as_str) == Some("unchanged") {
        println!("Compute is already set to these values. Nothing to update.");
        return Ok(());
    }
    println!("{}", success_line("Compute update requested."));
    if let Some((pool_id, description)) = changed_pool {
        println!("{} {pool_id}: {description}", dim_label("Pool"));
    }
    if let Some(overrides) = &change.quota_warning_overrides {
        let value = if overrides.is_empty() {
            "none".to_string()
        } else {
            overrides.join(", ")
        };
        println!("{} {value}", dim_label("Quota warning overrides"));
    }
    if let Some(operation_id) = response.pointer("/operation/id").and_then(Value::as_str) {
        println!("{} {operation_id}", dim_label("Operation"));
    }
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!(
            "alien deployments wait {deployment_id} --for terminal"
        ))
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Environment variables
// ---------------------------------------------------------------------------

pub(crate) async fn set_environment_variables_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment: &DeploymentDetailResponse,
    variables: Vec<EnvironmentVariableConfig>,
    confirmation: ConfirmationMode,
    json: bool,
) -> Result<()> {
    let deployment_id = deployment.id.as_str();
    let names: Vec<String> = variables
        .iter()
        .map(|variable| variable.name.as_str().to_string())
        .collect();
    if !json {
        println!(
            "Replacing the environment variables of {} ({deployment_id}).",
            deployment.name.as_str()
        );
        println!(
            "{}",
            dim_label(
                "Variables not listed here are removed. Variables backed by stack inputs are kept; change those with `alien deployments set-inputs`."
            )
        );
    }
    if matches!(confirmation, ConfirmationMode::Prompt)
        && !prompt_confirm("Replace the environment variables?", false)?
    {
        println!("{}", dim_label("Nothing changed."));
        return Ok(());
    }

    let body: UpdateDeploymentEnvironmentVariablesRequest =
        request_body(serde_json::json!({ "variables": variables }), "env")?;
    let response = client
        .update_deployment_environment_variables()
        .id(deployment_id)
        .workspace(workspace)
        .body(body)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("updating environment variables for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();

    if json {
        return print_json(&serde_json::json!({
            "deploymentId": deployment_id,
            "variables": names,
            "message": response.message,
        }));
    }
    println!("{}", success_line(&response.message));
    if names.is_empty() {
        println!("{} none", dim_label("Variables"));
    } else {
        println!("{} {}", dim_label("Variables"), names.join(", "));
    }
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!(
            "alien deployments wait {deployment_id} --for terminal"
        ))
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Stack inputs
// ---------------------------------------------------------------------------

/// Stack input changes for `alien deployments set-inputs`.
#[derive(Debug, Clone, Default)]
pub(crate) struct InputChanges {
    /// `ID=VALUE`, read as `alien deploy --input` reads it.
    pub inputs: Vec<String>,
    /// `ID=VALUE`, sent as text, as `alien deploy --secret-input` sends it.
    pub secret_inputs: Vec<String>,
    /// Input IDs to reset.
    pub clear: Vec<String>,
}

/// Parses `changes` against the inputs the release declares and returns the
/// values to save. Rejects undeclared inputs, an input given twice, and an
/// input that is both set and cleared.
pub(crate) fn input_values_to_save(
    declared: &[&str],
    changes: &InputChanges,
) -> Result<Map<String, Value>> {
    let invalid = |field: &str, message: String| {
        AlienError::new(ErrorData::ValidationError {
            field: field.to_string(),
            message,
        })
    };
    if changes.inputs.is_empty() && changes.secret_inputs.is_empty() && changes.clear.is_empty() {
        return Err(invalid(
            "input",
            "Pass at least one --input ID=VALUE, --secret-input ID=VALUE, or --clear ID."
                .to_string(),
        ));
    }
    let check_declared = |id: &str| {
        if declared.contains(&id) {
            return Ok(());
        }
        Err(invalid(
            "input",
            if declared.is_empty() {
                format!("Input '{id}' is not declared: this deployment's release has no stack inputs you can change.")
            } else {
                format!(
                    "Input '{id}' is not declared by this deployment's release. Inputs: {}",
                    declared.join(", ")
                )
            },
        ))
    };

    let mut values = Map::new();
    let parsed = changes.inputs.iter().map(|arg| (arg, "--input")).chain(
        changes
            .secret_inputs
            .iter()
            .map(|arg| (arg, "--secret-input")),
    );
    for (arg, flag) in parsed {
        let (id, raw) = crate::commands::deploy::parse_stack_input_arg(arg, flag)?;
        check_declared(&id)?;
        let value = if flag == "--secret-input" {
            Value::String(raw)
        } else {
            crate::commands::deploy::parse_raw_stack_input_value(&raw)
        };
        if values.insert(id.clone(), value).is_some() {
            return Err(invalid("input", format!("Input '{id}' was given twice.")));
        }
    }
    for id in &changes.clear {
        check_declared(id)?;
        if values.contains_key(id) {
            return Err(invalid(
                "clear",
                format!("Input '{id}' can't be both set and cleared."),
            ));
        }
    }
    Ok(values)
}

async fn fetch_inputs(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
) -> Result<Value> {
    let response = client
        .get_deployment_inputs()
        .id(deployment_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("reading stack inputs for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();
    serde_json::to_value(&response)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "deployment inputs".to_string(),
        })
}

fn print_inputs(inputs: &Value) {
    let definitions = inputs
        .get("inputs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if definitions.is_empty() {
        println!(
            "{}",
            dim_label("This deployment's release declares no stack inputs.")
        );
        return;
    }
    let provided: Vec<&str> = inputs
        .get("providedInputIds")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut table = make_table(&["Input", "Kind", "Required", "Value"]);
    for definition in definitions {
        let id = definition.get("id").and_then(Value::as_str).unwrap_or("-");
        let kind = definition
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let required = definition
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let value = match inputs.pointer(&format!(
            "/values/{}",
            id.replace('~', "~0").replace('/', "~1")
        )) {
            Some(Value::String(value)) => value.clone(),
            Some(value) => value.to_string(),
            None if provided.contains(&id) => "(set)".to_string(),
            None => "-".to_string(),
        };
        table.add_row(vec![
            id.to_string(),
            kind.to_string(),
            if required { "yes" } else { "no" }.to_string(),
            value,
        ]);
    }
    print_table(table);
}

pub(crate) async fn inputs_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    json: bool,
) -> Result<()> {
    let inputs = fetch_inputs(client, workspace, deployment_id).await?;
    if json {
        return print_json(&inputs);
    }
    print_inputs(&inputs);
    Ok(())
}

pub(crate) async fn set_inputs_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    changes: InputChanges,
    json: bool,
) -> Result<()> {
    let current = fetch_inputs(client, workspace, deployment_id).await?;
    let declared: Vec<&str> = current
        .get("inputs")
        .and_then(Value::as_array)
        .map(|inputs| {
            inputs
                .iter()
                .filter_map(|input| input.get("id")?.as_str())
                .collect()
        })
        .unwrap_or_default();
    let values = input_values_to_save(&declared, &changes)?;
    let clear = changes.clear;

    let body: UpdateDeploymentInputsRequest = request_body(
        serde_json::json!({ "inputValues": values, "clearInputIds": clear }),
        "input",
    )?;
    let response = client
        .update_deployment_inputs()
        .id(deployment_id)
        .workspace(workspace)
        .body(body)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("updating stack inputs for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();
    let response =
        serde_json::to_value(&response)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialize".to_string(),
                reason: "updated deployment inputs".to_string(),
            })?;

    if json {
        return print_json(&response);
    }
    if response.get("outcome").and_then(Value::as_str) == Some("unchanged") {
        println!("Inputs already have these values. Nothing to update.");
        return Ok(());
    }
    println!("{}", success_line("Stack inputs saved."));
    print_inputs(&response);
    if response
        .get("runtimeUpdateRequested")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        println!(
            "{} {}",
            dim_label("Next"),
            command(&format!(
                "alien deployments wait {deployment_id} --for terminal"
            ))
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Health check
// ---------------------------------------------------------------------------

pub(crate) async fn check_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    json: bool,
) -> Result<()> {
    let response = client
        .request_deployment_check()
        .id(deployment_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("requesting a health check for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();
    if json {
        return print_json(&response);
    }
    println!(
        "{}",
        success_line(&format!(
            "Health check requested at {}.",
            response.requested_at.to_rfc3339()
        ))
    );
    println!(
        "{}",
        dim_label("The manager checks the deployment's resources on its next pass.")
    );
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments resources {deployment_id}"))
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Machines
// ---------------------------------------------------------------------------

pub(crate) async fn drain_machine_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    machine_id: &str,
    deadline: Option<Duration>,
    force: bool,
    confirmation: ConfirmationMode,
    json: bool,
) -> Result<()> {
    if matches!(confirmation, ConfirmationMode::Prompt) {
        println!(
            "{}",
            dim_label("A forced drain stops the machine's replicas without waiting for healthy replacements.")
        );
        if !prompt_confirm(&format!("Force-drain machine {machine_id}?"), false)? {
            println!("{}", dim_label("Nothing changed."));
            return Ok(());
        }
    }
    let body: DrainMachinesMachineRequest = request_body(
        serde_json::json!({
            "deadlineSeconds": deadline.map(|deadline| deadline.as_secs()),
            "force": force,
        }),
        "deadline",
    )?;
    let response = client
        .drain_machines_machine()
        .id(deployment_id)
        .machine_id(machine_id)
        .workspace(workspace)
        .body(body)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("draining machine '{machine_id}' of deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();
    if json {
        return print_json(&response);
    }
    if response.requested {
        println!(
            "{}",
            success_line(&format!("Drain requested for {machine_id}."))
        );
        println!(
            "{}",
            dim_label("Workloads move off the machine. Undo with `alien deployments cancel-machine-drain`.")
        );
    } else {
        println!("Machine {machine_id} was not drained.");
    }
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments machines {deployment_id}"))
    );
    Ok(())
}

pub(crate) async fn cancel_machine_drain_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    machine_id: &str,
    json: bool,
) -> Result<()> {
    let response = client
        .cancel_machines_machine_drain()
        .id(deployment_id)
        .machine_id(machine_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!(
                "cancelling the drain of machine '{machine_id}' of deployment '{deployment_id}'"
            ),
            url: None,
        })?
        .into_inner();
    if json {
        return print_json(&response);
    }
    if response.cancelled {
        println!(
            "{}",
            success_line(&format!("Drain cancelled for {machine_id}."))
        );
    } else {
        println!("Machine {machine_id} had no drain to cancel.");
    }
    Ok(())
}

pub(crate) async fn remove_machine_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    machine_id: &str,
    confirmation: ConfirmationMode,
    json: bool,
) -> Result<()> {
    if !json {
        println!("Removing machine {machine_id} from deployment {deployment_id}.");
        println!(
            "{}",
            dim_label("The machine is forgotten and can't rejoin with its old identity. Drain it first so its workloads move.")
        );
    }
    if matches!(confirmation, ConfirmationMode::Prompt)
        && !prompt_confirm("Remove this machine?", false)?
    {
        println!("{}", dim_label("Nothing changed."));
        return Ok(());
    }
    let response = client
        .remove_machines_machine()
        .id(deployment_id)
        .machine_id(machine_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("removing machine '{machine_id}' from deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();
    if json {
        return print_json(&response);
    }
    if response.removed {
        println!(
            "{}",
            success_line(&format!("Machine {machine_id} removed."))
        );
    } else {
        println!("Machine {machine_id} was not found on this deployment.");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Machine join tokens
// ---------------------------------------------------------------------------

/// Whether a join token still lets a host join. A token stops working once it
/// is revoked, past its expiry, or out of joins.
fn join_token_status(
    revoked: bool,
    expires_at: Option<&str>,
    join_count: i64,
    max_joins: Option<i64>,
    now: chrono::DateTime<chrono::Utc>,
) -> &'static str {
    let expired = expires_at
        .and_then(|expires_at| chrono::DateTime::parse_from_rfc3339(expires_at).ok())
        .is_some_and(|expires_at| expires_at <= now);
    if revoked {
        "revoked"
    } else if expired {
        "expired"
    } else if max_joins.is_some_and(|max| join_count >= max) {
        "exhausted"
    } else {
        "active"
    }
}

pub(crate) async fn list_join_tokens_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    json: bool,
) -> Result<()> {
    let response = client
        .list_machines_join_tokens()
        .id(deployment_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("listing join tokens for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();
    if json {
        return print_json(&response);
    }
    if response.tokens.is_empty() {
        println!("{}", dim_label("No join tokens."));
        println!(
            "{} {}",
            dim_label("Next"),
            command(&format!(
                "alien deployments join-tokens create {deployment_id}"
            ))
        );
        return Ok(());
    }
    let now = chrono::Utc::now();
    let mut table = make_table(&[
        "Token",
        "Status",
        "Joins",
        "Expires",
        "Created",
        "Last used",
    ]);
    for token in response.tokens {
        let status = join_token_status(
            token.revoked_at.is_some(),
            token.expires_at.as_deref(),
            token.join_count,
            token.max_joins,
            now,
        );
        let joins = match token.max_joins {
            Some(max) => format!("{}/{max}", token.join_count),
            None => token.join_count.to_string(),
        };
        table.add_row(vec![
            token.id,
            status.to_string(),
            joins,
            token.expires_at.unwrap_or_else(|| "never".to_string()),
            token.created_at,
            token.last_used_at.unwrap_or_else(|| "-".to_string()),
        ]);
    }
    print_table(table);
    Ok(())
}

fn print_new_join_token(
    deployment_id: &str,
    token_id: &str,
    join_token: &str,
    cli_command_name: Option<&str>,
    cli_install_script_url: Option<&str>,
) {
    println!("{} {token_id}", dim_label("Token"));
    println!();
    println!(
        "{}",
        dim_label("Run this on each Linux host to join it. The token is shown only once.")
    );
    println!(
        "{}",
        crate::commands::deploy::machines_join_command(
            cli_command_name.unwrap_or("alien-deploy"),
            cli_install_script_url,
            join_token,
        )
    );
    println!();
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments machines {deployment_id}"))
    );
}

pub(crate) async fn create_join_token_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    json: bool,
) -> Result<()> {
    let response = client
        .create_machines_join_token()
        .id(deployment_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("creating a join token for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();
    if json {
        return print_json(&response);
    }
    println!(
        "{}",
        success_line("Join token created. Existing tokens keep working.")
    );
    let join_token = crate::commands::deploy::host_join_token(
        &response.join_token,
        &response.control_plane_url,
        &response.cluster_id,
    )?;
    print_new_join_token(
        deployment_id,
        &response.token.id,
        &join_token,
        response.cli_command_name.as_deref(),
        response.cli_install_script_url.as_deref(),
    );
    Ok(())
}

pub(crate) async fn rotate_join_token_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    confirmation: ConfirmationMode,
    json: bool,
) -> Result<()> {
    if !json {
        println!("Rotating the join tokens of deployment {deployment_id}.");
        println!(
            "{}",
            dim_label(
                "Every existing join token is revoked. Hosts that already joined keep running."
            )
        );
    }
    if matches!(confirmation, ConfirmationMode::Prompt)
        && !prompt_confirm("Revoke every join token and create a new one?", false)?
    {
        println!("{}", dim_label("Nothing changed."));
        return Ok(());
    }
    let response = client
        .rotate_machines_join_token()
        .id(deployment_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("rotating join tokens for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();
    if json {
        return print_json(&response);
    }
    println!(
        "{}",
        success_line("Join tokens rotated. Earlier tokens no longer work.")
    );
    let join_token = crate::commands::deploy::host_join_token(
        &response.join_token,
        &response.control_plane_url,
        &response.cluster_id,
    )?;
    print_new_join_token(
        deployment_id,
        &response.token.id,
        &join_token,
        response.cli_command_name.as_deref(),
        response.cli_install_script_url.as_deref(),
    );
    Ok(())
}

pub(crate) async fn revoke_join_token_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    token_id: &str,
    confirmation: ConfirmationMode,
    json: bool,
) -> Result<()> {
    if matches!(confirmation, ConfirmationMode::Prompt)
        && !prompt_confirm(&format!("Revoke join token {token_id}?"), false)?
    {
        println!("{}", dim_label("Nothing changed."));
        return Ok(());
    }
    let response = client
        .revoke_machines_join_token()
        .id(deployment_id)
        .token_id(token_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("revoking join token '{token_id}' of deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();
    if json {
        return print_json(&response);
    }
    if response.revoked {
        println!(
            "{}",
            success_line(&format!(
                "Join token {token_id} revoked. Hosts that already joined keep running."
            ))
        );
    } else {
        println!("Join token {token_id} was already revoked or doesn't exist.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::FailureDomainSelection;
    use std::collections::HashMap;

    fn compute(pools: Vec<(&str, ComputePoolSelection)>) -> ComputeSettings {
        ComputeSettings {
            pools: pools
                .into_iter()
                .map(|(id, selection)| (id.to_string(), selection))
                .collect::<HashMap<_, _>>(),
            containers: HashMap::new(),
        }
    }

    fn fixed(machines: u32, machine: Option<&str>) -> ComputePoolSelection {
        ComputePoolSelection::Fixed {
            machines,
            machine: machine.map(str::to_string),
            failure_domains: Some(FailureDomainSelection {
                spread: 2,
                selected_failure_domains: vec!["us-east-2a".into(), "us-east-2b".into()],
            }),
        }
    }

    #[test]
    fn changing_one_pool_sends_every_other_pool_unchanged() {
        let current = compute(vec![
            ("general", fixed(2, Some("t3.medium"))),
            (
                "gpu",
                ComputePoolSelection::Autoscale {
                    min: 0,
                    max: 4,
                    machine: Some("g5.xlarge".into()),
                    failure_domains: None,
                },
            ),
        ]);
        let (pool, next) = apply_pool_change(
            Some(&current),
            &PoolChange {
                pool: Some("general".into()),
                machines: Some(5),
                ..PoolChange::default()
            },
        )
        .expect("count change applies");

        assert_eq!(pool, "general");
        assert_eq!(next.pools["general"], fixed(5, Some("t3.medium")));
        assert_eq!(next.pools["gpu"], current.pools["gpu"]);
        assert_eq!(next.pools.len(), 2);
    }

    #[test]
    fn switching_modes_keeps_machine_type_and_placement() {
        let current = compute(vec![("general", fixed(2, Some("t3.medium")))]);
        let (_, autoscale) = apply_pool_change(
            Some(&current),
            &PoolChange {
                min: Some(1),
                max: Some(6),
                ..PoolChange::default()
            },
        )
        .expect("fixed pool becomes autoscaling");
        let ComputePoolSelection::Fixed {
            failure_domains, ..
        } = current.pools["general"].clone()
        else {
            unreachable!()
        };
        assert_eq!(
            autoscale.pools["general"],
            ComputePoolSelection::Autoscale {
                min: 1,
                max: 6,
                machine: Some("t3.medium".into()),
                failure_domains: failure_domains.clone(),
            }
        );

        let (_, resized) = apply_pool_change(
            Some(&autoscale),
            &PoolChange {
                max: Some(10),
                machine: Some("m6i.large".into()),
                ..PoolChange::default()
            },
        )
        .expect("one bound changes");
        assert_eq!(
            resized.pools["general"],
            ComputePoolSelection::Autoscale {
                min: 1,
                max: 10,
                machine: Some("m6i.large".into()),
                failure_domains,
            }
        );
    }

    #[test]
    fn refuses_ambiguous_or_incomplete_pool_changes() {
        let two = compute(vec![("a", fixed(1, None)), ("b", fixed(1, None))]);
        let error = apply_pool_change(
            Some(&two),
            &PoolChange {
                machines: Some(3),
                ..PoolChange::default()
            },
        )
        .expect_err("two pools need --pool");
        assert!(error.to_string().contains("a, b"), "{error}");

        let error = apply_pool_change(
            Some(&two),
            &PoolChange {
                pool: Some("c".into()),
                machines: Some(3),
                ..PoolChange::default()
            },
        )
        .expect_err("unknown pool");
        assert!(error.to_string().contains("'c'"), "{error}");

        let one = compute(vec![("a", fixed(1, None))]);
        apply_pool_change(
            Some(&one),
            &PoolChange {
                max: Some(3),
                ..PoolChange::default()
            },
        )
        .expect_err("fixed to autoscale needs both bounds");
        apply_pool_change(
            Some(&one),
            &PoolChange {
                min: Some(4),
                max: Some(3),
                ..PoolChange::default()
            },
        )
        .expect_err("min above max");
        apply_pool_change(
            None,
            &PoolChange {
                machines: Some(1),
                ..PoolChange::default()
            },
        )
        .expect_err("no pools");
    }

    #[test]
    fn compute_body_sends_pools_without_containers() {
        let mut current = compute(vec![("general", fixed(2, Some("t3.medium")))]);
        current.containers.insert(
            "api".to_string(),
            serde_json::from_value(serde_json::json!({"cpu": 1.0, "memory": "1Gi"}))
                .expect("container settings"),
        );
        let (_, next) = apply_pool_change(
            Some(&current),
            &PoolChange {
                machines: Some(3),
                ..PoolChange::default()
            },
        )
        .expect("count change applies");
        let body = compute_request(Some(&next.pools), None).expect("request type accepts pools");
        let sent = serde_json::to_value(&body).expect("serialize request");
        // The saved container settings are not sent back: the API keeps them.
        assert_eq!(
            sent,
            serde_json::json!({"compute": {"pools": serde_json::to_value(&next.pools).unwrap()}})
        );
        assert_eq!(sent["compute"]["pools"]["general"]["machines"], 3);
        assert_eq!(sent["compute"]["pools"]["general"]["machine"], "t3.medium");
    }

    #[tokio::test]
    async fn clearing_quota_warning_overrides_sends_an_empty_list() {
        let server = httpmock::MockServer::start_async().await;
        let clear = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::PATCH)
                    .path("/v1/deployments/dep_0c29fq4a2yjb7kx3smwdgxlcxd7f/compute")
                    .query_param("workspace", "my-workspace")
                    .json_body(serde_json::json!({"quotaWarningOverrides": []}));
                then.status(200)
                    .json_body(serde_json::json!({"outcome": "accepted", "operation": null}));
            })
            .await;
        let client = alien_platform_api::Client::new(&server.base_url());
        client
            .update_deployment_compute()
            .id("dep_0c29fq4a2yjb7kx3smwdgxlcxd7f")
            .workspace("my-workspace")
            .body(compute_request(None, Some(Vec::new())).expect("clear body"))
            .send()
            .await
            .expect("clear request matches");
        clear.assert_async().await;

        let unchanged = serde_json::to_value(compute_request(None, None).expect("empty body"))
            .expect("serialize request");
        assert_eq!(unchanged, serde_json::json!({}));
    }

    #[test]
    fn join_token_status_shows_tokens_that_no_longer_work() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let past = Some("2026-06-01T11:59:59Z");
        let future = Some("2026-06-02T00:00:00Z");
        assert_eq!(join_token_status(false, None, 3, None, now), "active");
        assert_eq!(join_token_status(false, future, 1, Some(2), now), "active");
        assert_eq!(join_token_status(false, past, 0, None, now), "expired");
        assert_eq!(
            join_token_status(false, future, 2, Some(2), now),
            "exhausted"
        );
        assert_eq!(join_token_status(true, past, 2, Some(2), now), "revoked");
    }

    #[test]
    fn input_changes_parse_like_deploy_and_reject_conflicts() {
        let declared = ["region", "domains", "token"];
        let changes = |inputs: &[&str], secret_inputs: &[&str], clear: &[&str]| InputChanges {
            inputs: inputs.iter().map(|value| value.to_string()).collect(),
            secret_inputs: secret_inputs
                .iter()
                .map(|value| value.to_string())
                .collect(),
            clear: clear.iter().map(|value| value.to_string()).collect(),
        };

        let values = input_values_to_save(
            &declared,
            &changes(
                &["region=us-east-2", r#"domains=["a.example","b.example"]"#],
                &[r#"token=["not","a","list"]"#],
                &[],
            ),
        )
        .expect("declared inputs parse");
        assert_eq!(values["region"], serde_json::json!("us-east-2"));
        assert_eq!(
            values["domains"],
            serde_json::json!(["a.example", "b.example"])
        );
        // A secret is sent as typed, even when it looks like JSON.
        assert_eq!(values["token"], serde_json::json!(r#"["not","a","list"]"#));

        let error = |changes: InputChanges| {
            input_values_to_save(&declared, &changes)
                .expect_err("rejected")
                .to_string()
        };
        assert!(error(changes(&["zone=a"], &[], &[])).contains("'zone'"));
        assert!(error(changes(&[], &[], &["zone"])).contains("'zone'"));
        assert!(error(changes(&["region=a"], &["region=b"], &[])).contains("twice"));
        assert!(error(changes(&["region=a"], &[], &["region"])).contains("set and cleared"));
        assert!(error(changes(&["region"], &[], &[])).contains("id=value"));
        assert!(error(changes(&[], &[], &[])).contains("at least one"));
        assert!(
            input_values_to_save(&declared, &changes(&[], &[], &["region"]))
                .expect("clear only")
                .is_empty()
        );
    }
}
