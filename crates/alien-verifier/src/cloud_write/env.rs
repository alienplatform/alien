//! The env a controller's request must carry: what the signed release, its config and the
//! install decide, plus the values the release lets the deployment provide, exactly.

use std::collections::BTreeMap;

use alien_core::{
    worker_runtime_environment_plan, Platform, RuntimeEnvironmentValue,
    ENV_ALIEN_PUBLIC_ENDPOINTS_JSON,
};
use alien_error::{AlienError, Context};
use alien_signing::{
    binding_env, binding_env_name, build_workload, check_own_binding, release::RUNTIME_SEND_OTLP,
    resolve_bindings, InstallId, LocalValues, OwnBindingFacts, Workload, WorkloadRequest,
};
use serde_json::Value;

use crate::error::{ErrorData, Result};
use crate::plan::{changed_env, Place, Reads, SECRETS_ENV};
use crate::trust::Accepted;

/// Where telemetry goes. Only the signed config's `telemetryEndpoint` may be it.
pub const OTLP_LOGS_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT";
/// Telemetry names that follow the endpoint: allowed only when the config signs one.
const TELEMETRY_FOLLOWERS: &[&str] = &["OTEL_SERVICE_NAME", "OTEL_RESOURCE_ATTRIBUTES"];
/// The runtime's pointer to its own telemetry credentials, keys in the install's `secrets`
/// vault.
const RUNTIME_SECRETS: &str = "ALIEN_RUNTIME_SECRETS";
/// The install's `secrets` vault, which the runtime reads its credentials from.
const SECRETS_VAULT: &str = "secrets";
/// The public endpoints the control plane lists for a workload. A protected workload learns its
/// public address from its own binding, which is checked against the cloud's read.
const PUBLIC_ENDPOINTS: &str = ENV_ALIEN_PUBLIC_ENDPOINTS_JSON;

fn refused(reason: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Refused { reason })
}

/// The runtime values the deployment provides for `workload` that the verifier knows itself,
/// from the runtime plan the controller follows: what platform and transport this is, which
/// binding is the worker's own, and the install's account, project or subscription. The rest
/// (region, tenant, client id, and what only the control plane holds, such as the commands
/// token) stay the control plane's to provide.
fn derived_runtime_env(place: &Place, workload: &Workload) -> BTreeMap<&'static str, String> {
    let platform = match &place.install {
        InstallId::Aws { .. } => Platform::Aws,
        InstallId::Gcp { .. } => Platform::Gcp,
        InstallId::Azure { .. } => Platform::Azure,
        InstallId::Kubernetes { .. } | InstallId::Machine { .. } => return BTreeMap::new(),
    };
    worker_runtime_environment_plan(platform)
        .into_iter()
        // On or off, telemetry can only go to the signed endpoint (see `check_env`).
        .filter(|entry| entry.name != RUNTIME_SEND_OTLP)
        .filter_map(|entry| {
            let value = match (entry.value, &place.install) {
                (RuntimeEnvironmentValue::Literal(value), _) => value.to_string(),
                (RuntimeEnvironmentValue::CurrentWorkerBindingName, _) => workload.name.clone(),
                (RuntimeEnvironmentValue::AwsAccountId, InstallId::Aws { account, .. }) => {
                    account.clone()
                }
                (RuntimeEnvironmentValue::GcpProjectId, InstallId::Gcp { project, .. }) => {
                    project.clone()
                }
                (
                    RuntimeEnvironmentValue::AzureSubscriptionId,
                    InstallId::Azure { subscription, .. },
                ) => subscription.clone(),
                _ => return None,
            };
            Some((entry.name, value))
        })
        .collect()
}

/// Check `env`, the environment of `workload`'s function in a controller's request, against
/// the accepted release, its config and what the verifier read from its install. `own_name` is
/// the function's name, which its own binding must describe.
pub fn check_env(
    place: &Place,
    accepted: &Accepted,
    workload: &Workload,
    reads: &Reads,
    own_name: &str,
    env: &BTreeMap<String, String>,
) -> Result<()> {
    let name = &workload.name;
    let config = accepted.config_body();
    let mut rest = env.clone();

    if rest.contains_key(PUBLIC_ENDPOINTS) {
        return Err(refused(format!(
            "env {PUBLIC_ENDPOINTS} of workload '{name}' lists public URLs the control plane chose; the workload's own binding carries the address the cloud reports"
        )));
    }

    // Telemetry goes only where the signed config says.
    let endpoint = rest.remove(OTLP_LOGS_ENDPOINT);
    match (&config.telemetry_endpoint, &endpoint) {
        (Some(signed), Some(sent)) if signed == sent => {}
        (Some(signed), Some(sent)) => {
            return Err(refused(format!(
                "env {OTLP_LOGS_ENDPOINT} of workload '{name}' is {sent}, not the signed telemetry endpoint {signed}"
            )))
        }
        (None, Some(sent)) => {
            return Err(refused(format!(
                "workload '{name}' sends telemetry to {sent}, but the signed config names no telemetry endpoint"
            )))
        }
        (_, None) => {}
    }
    for follower in TELEMETRY_FOLLOWERS.iter().chain([&RUNTIME_SECRETS]) {
        if let Some(value) = rest.remove(*follower) {
            if endpoint.is_none() {
                return Err(refused(format!(
                    "env {follower} of workload '{name}' is telemetry, and the request sends none to the signed endpoint"
                )));
            }
            if *follower == RUNTIME_SECRETS {
                check_runtime_secrets(name, &value)?;
            }
        }
    }

    // What the deployment provides and the verifier knows itself must be what it knows.
    for (key, expected) in derived_runtime_env(place, workload) {
        if let Some(sent) = rest.get(key) {
            if *sent != expected {
                return Err(refused(format!(
                    "env {key} of workload '{name}' is {sent}, not the value the verifier derives ({expected})"
                )));
            }
        }
    }

    // The secrets vault is the install's own.
    let vault_key = binding_env_name(SECRETS_VAULT);
    if let Some(value) = rest.remove(&vault_key) {
        let derived = binding_env(
            &BTreeMap::from([(SECRETS_VAULT.to_string(), "vault".to_string())]),
            &place.binding_context(),
        )
        .context(ErrorData::Refused {
            reason: "the install's secrets vault binding can't be derived".to_string(),
        })?;
        let expected = derived.values.get(&vault_key).ok_or_else(|| {
            refused(format!(
                "the install's secrets vault binding ({vault_key}) can't be derived here"
            ))
        })?;
        if !same_json(expected, &value) {
            return Err(refused(format!(
                "env {vault_key} of workload '{name}' is not the install's secrets vault"
            )));
        }
    }

    // The function's own binding describes this function and nothing else.
    let own_key = binding_env_name(name);
    let own_facts = own_binding_facts(place, reads, own_name);
    if let Some(value) = rest.remove(&own_key) {
        check_own_binding(&value, &own_facts).map_err(|reason| {
            refused(format!(
                "env {own_key} of workload '{name}' doesn't describe the function {own_name}: {reason}"
            ))
        })?;
    }

    // The secrets the function loads at start are the signed list.
    let secrets_list = rest.remove(SECRETS_ENV);

    let derived =
        binding_env(&workload.bindings, &place.binding_context()).context(ErrorData::Refused {
            reason: format!("the bindings of workload '{name}' were refused"),
        })?;
    let bindings = resolve_bindings(derived, &reads.bindings, &config.bindings).context(
        ErrorData::Refused {
            reason: format!("the bindings of workload '{name}' have no value"),
        },
    )?;
    let built = build_workload(
        &accepted.release.body,
        name,
        &config.env,
        &LocalValues {
            deployer_inputs: reads.deployer_inputs.clone(),
            bindings,
            own_binding: own_facts,
        },
        &WorkloadRequest {
            env: rest.clone(),
            ..Default::default()
        },
    )
    .context(ErrorData::Refused {
        reason: "the control plane's values don't fit the release".to_string(),
    })?;
    let differs: Vec<&String> = changed_env(&rest, &built.env)
        .into_iter()
        .filter(|key| match (rest.get(*key), built.env.get(*key)) {
            (Some(sent), Some(signed)) => !same_json(signed, sent),
            _ => true,
        })
        .collect();
    if !differs.is_empty() {
        return Err(refused(format!(
            "the env of workload '{name}' differs from the signed one for {differs:?}"
        )));
    }

    let signed_secrets = (!workload.secrets.is_empty()).then(|| {
        let keys: Vec<&str> = workload
            .secrets
            .iter()
            .map(|secret| secret.name.as_str())
            .collect();
        serde_json::json!({ "keys": keys, "hash": accepted.release.digest }).to_string()
    });
    match (&signed_secrets, &secrets_list) {
        (None, None) => Ok(()),
        (Some(signed), Some(sent)) if same_json(signed, sent) => Ok(()),
        _ => Err(refused(format!(
            "env {SECRETS_ENV} of workload '{name}' is not the signed secrets list"
        ))),
    }
}

/// Two env values that are the same text, or the same JSON object: binding values and lists are
/// JSON whose key order the controller may not keep.
fn same_json(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (
        serde_json::from_str::<Value>(a),
        serde_json::from_str::<Value>(b),
    ) {
        (Ok(a @ Value::Object(_)), Ok(b)) => a == b,
        _ => false,
    }
}

/// The runtime secrets pointer names keys in the install's `secrets` vault and nothing else.
fn check_runtime_secrets(name: &str, value: &str) -> Result<()> {
    let pointer: serde_json::Map<String, Value> = serde_json::from_str(value).map_err(|_| {
        refused(format!(
            "env {RUNTIME_SECRETS} of workload '{name}' is not a runtime secrets pointer"
        ))
    })?;
    for (key, value) in &pointer {
        let known = matches!(
            key.as_str(),
            "otlpLogsAuthHeader" | "otlpMetricsAuthHeader" | "hash"
        );
        if !known || !(value.is_string() || value.is_null()) {
            return Err(refused(format!(
                "env {RUNTIME_SECRETS} of workload '{name}' has field {key}: it may only name keys in the install's secrets vault"
            )));
        }
    }
    Ok(())
}

/// What the function's own binding is checked against: the install and what the cloud reports
/// for the function.
fn own_binding_facts(place: &Place, reads: &Reads, own_name: &str) -> OwnBindingFacts {
    OwnBindingFacts {
        context: Some(place.binding_context()),
        resource_name: own_name.to_string(),
        resource_group: match &place.install {
            InstallId::Azure { resource_group, .. } => Some(resource_group.clone()),
            _ => None,
        },
        location: reads.own.location.clone(),
        urls: reads.own.urls.clone(),
    }
}
