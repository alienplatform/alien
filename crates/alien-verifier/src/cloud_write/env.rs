//! The env a controller's request must carry: what the signed release, its config and the
//! install decide, plus the values the release lets the deployment provide, exactly.

use std::collections::BTreeMap;

use alien_core::{
    worker_runtime_environment_plan, Platform, RuntimeEnvironmentValue, DEPLOYER_SECRETS_VAULT_ID,
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
    let deployer_values = reads
        .deployer_inputs
        .iter()
        .map(|(id, value)| (id.clone(), serde_json::Value::String(value.clone())))
        .collect();
    let deployer_secrets = alien_core::release_statement::signed_deployer_secrets(
        &accepted.release.body,
        name,
        &config.inputs,
        &deployer_values,
    )
    .context(ErrorData::Refused {
        reason: format!("the deployer secrets of workload '{name}' can't be resolved"),
    })?;

    // The deployer secrets vault is the install's own, and only a worker the release gives a
    // deployer secret reads it.
    let deployer_vault = binding_env_name(DEPLOYER_SECRETS_VAULT_ID);
    match (rest.remove(&deployer_vault), deployer_secrets.is_empty()) {
        (None, true) => {}
        (None, false) => {
            return Err(refused(format!(
                "workload '{name}' loads deployer secrets, and the request carries no {deployer_vault}"
            )))
        }
        (Some(_), true) => {
            return Err(refused(format!(
                "env {deployer_vault} of workload '{name}' is the deployer secrets vault, and the signed release gives the workload no deployer secret"
            )))
        }
        (Some(sent), false) => {
            let derived = binding_env(
                &BTreeMap::from([(DEPLOYER_SECRETS_VAULT_ID.to_string(), "vault".to_string())]),
                &place.binding_context(),
            )
            .context(ErrorData::Refused {
                reason: "the install's deployer secrets vault binding can't be derived".to_string(),
            })?;
            let expected = resolve_bindings(derived, &reads.bindings, &config.bindings)
                .context(ErrorData::Refused {
                    reason: "the install's deployer secrets vault has no value".to_string(),
                })?;
            if !expected
                .get(&deployer_vault)
                .is_some_and(|expected| same_json(expected, &sent))
            {
                return Err(refused(format!(
                    "env {deployer_vault} of workload '{name}' is not the install's deployer secrets vault"
                )));
            }
        }
    }

    let configured = alien_core::release_statement::signed_config_env(
        &accepted.release.body,
        name,
        &config,
        &deployer_values,
    )
    .context(ErrorData::Refused {
        reason: format!("the signed config can't be applied to workload '{name}'"),
    })?;
    let deployer_env = alien_core::release_statement::signed_deployer_env(
        &accepted.release.body,
        name,
        &config.inputs,
        &deployer_values,
    )
    .context(ErrorData::Refused {
        reason: format!("the deployer inputs of workload '{name}' can't be resolved"),
    })?;
    let built = build_workload(
        &accepted.release.body,
        name,
        &configured,
        &LocalValues {
            deployer_env,
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

    let signed_secrets = alien_core::release_statement::signed_workload_secrets(
        &accepted.release.body,
        name,
        &accepted.release.digest,
        &config.inputs,
        &deployer_values,
    )
    .context(ErrorData::Refused {
        reason: format!("the secrets list of workload '{name}' can't be built"),
    })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::trust::tests::{found, MemoryStore};
    use crate::trust::{accept, Accepted};
    use alien_core::Platform;
    use serde_json::json;

    const AZURE_VAULT: &str = "acmeprod-deployer-s-a1b2c3";

    fn place(platform: Platform) -> Place {
        let install = match platform {
            Platform::Aws => InstallId::Aws {
                account: ACCOUNT.to_string(),
                region: REGION.to_string(),
                prefix: PREFIX.to_string(),
            },
            Platform::Gcp => InstallId::Gcp {
                project: "acme-project".to_string(),
                prefix: PREFIX.to_string(),
            },
            _ => InstallId::Azure {
                subscription: "0000".to_string(),
                resource_group: "acme-rg".to_string(),
            },
        };
        Place {
            install,
            prefix: PREFIX.to_string(),
            vpc: None,
            machine_network: None,
        }
    }

    /// Release 1 of the workloads `api` and `jobs` on `platform`; only `api` gets the
    /// deployer's database password.
    async fn accepted(platform: Platform) -> Accepted {
        let worker = |name: &str| {
            alien_core::Worker::new(name.to_string())
                .code(alien_core::WorkerCode::Image {
                    image: format!("ghcr.io/shop/{name}@{DIGEST}"),
                })
                .permissions("execution".to_string())
                .build()
        };
        let mut stack = alien_core::Stack::new("shop".to_string())
            .add(worker("api"), alien_core::ResourceLifecycle::Live)
            .add(worker("jobs"), alien_core::ResourceLifecycle::Live)
            .build();
        stack.inputs = serde_json::from_value(json!([
            {"id": "databasePassword", "kind": "secret", "providedBy": ["deployer"],
             "required": true, "label": "Database password", "description": "d",
             "env": [{"name": "DATABASE_PASSWORD", "targetResources": ["api"]}]},
        ]))
        .unwrap();
        let body = alien_core::release_statement::release_body(
            platform,
            &stack,
            &alien_core::release_statement::ReleaseOptions {
                channel: CHANNEL.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        let install = found(&MemoryStore::protected(&settings())).await.unwrap();
        accept(
            &install,
            &[signer_list()],
            &signed_release(1, &body),
            None,
            &platform.to_string(),
            now(),
        )
        .unwrap()
    }

    fn vault_binding(platform: Platform) -> String {
        match platform {
            Platform::Aws => {
                json!({"service": "parameter-store", "vaultPrefix": "acme-prod-deployer-secrets"})
            }
            Platform::Gcp => {
                json!({"service": "secret-manager", "vaultPrefix": "acme-prod-deployer-secrets"})
            }
            _ => json!({"service": "key-vault", "vaultName": AZURE_VAULT}),
        }
        .to_string()
    }

    /// What the verifier reads on its own: on Azure, the vault whose name ends in a random
    /// suffix.
    fn reads(platform: Platform) -> Reads {
        let mut reads = Reads::default();
        if platform == Platform::Azure {
            reads.bindings.insert(
                "deployer-secrets".to_string(),
                vault_binding(Platform::Azure),
            );
        }
        reads
    }

    fn signed_list(accepted: &Accepted, vault_key: &str) -> String {
        json!({
            "keys": [],
            "hash": accepted.release.digest,
            "deployerSecrets": [{
                "name": "DATABASE_PASSWORD",
                "vaultKey": vault_key,
                "secretName": vault_key,
                "label": "Database password",
                "required": true,
            }],
        })
        .to_string()
    }

    fn env(entries: &[(&str, String)]) -> BTreeMap<String, String> {
        std::iter::once(("ALIEN_DEPLOYMENT_ID".to_string(), "dep_1".to_string()))
            .chain(entries.iter().map(|(k, v)| (k.to_string(), v.clone())))
            .collect()
    }

    fn check(
        platform: Platform,
        accepted: &Accepted,
        reads: &Reads,
        workload: &str,
        env: &BTreeMap<String, String>,
    ) -> std::result::Result<(), String> {
        let workload = accepted.release.body.workload(workload).unwrap();
        check_env(
            &place(platform),
            accepted,
            workload,
            reads,
            &format!("{PREFIX}-{}", workload.name),
            env,
        )
        .map_err(|error| error.to_string())
    }

    /// A deployer secret reaches the worker the release names it for, on every cloud, through
    /// the signed list and the install's own `deployer-secrets` vault, and nothing else does.
    #[tokio::test]
    async fn a_worker_loads_the_deployer_secrets_the_release_gives_it() {
        let binding_name = "ALIEN_DEPLOYER_SECRETS_BINDING";
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let accepted = accepted(platform).await;
            let reads = reads(platform);
            let list = signed_list(&accepted, "database-password");
            let sent = env(&[
                ("WORKLOAD_SECRETS", list.clone()),
                (binding_name, vault_binding(platform)),
            ]);
            check(platform, &accepted, &reads, "api", &sent)
                .unwrap_or_else(|error| panic!("{platform}: {error}"));

            let refuse =
                |what: &str, workload: &str, sent: BTreeMap<String, String>, reads: &Reads| {
                    let error = check(platform, &accepted, reads, workload, &sent)
                        .expect_err(&format!("{platform}: {what}"));
                    error
                };
            let error = refuse(
                "management points the worker at another vault key",
                "api",
                env(&[
                    ("WORKLOAD_SECRETS", signed_list(&accepted, "other-secret")),
                    (binding_name, vault_binding(platform)),
                ]),
                &reads,
            );
            assert!(
                error.contains("is not the signed secrets list"),
                "{platform}: {error}"
            );
            let error = refuse(
                "the list is left out",
                "api",
                env(&[(binding_name, vault_binding(platform))]),
                &reads,
            );
            assert!(
                error.contains("is not the signed secrets list"),
                "{platform}: {error}"
            );
            let error = refuse(
                "the binding names another vault",
                "api",
                env(&[
                    ("WORKLOAD_SECRETS", list.clone()),
                    (
                        binding_name,
                        json!({"service": "parameter-store", "vaultPrefix": "attacker-vault", "vaultName": "attacker"}).to_string(),
                    ),
                ]),
                &reads,
            );
            assert!(
                error.contains("is not the install's deployer secrets vault"),
                "{platform}: {error}"
            );
            let error = refuse(
                "the workload loads the secret and has no vault",
                "api",
                env(&[("WORKLOAD_SECRETS", list.clone())]),
                &reads,
            );
            assert!(error.contains(binding_name), "{platform}: {error}");
            let error = refuse(
                "a worker the release gives no deployer secret",
                "jobs",
                env(&[
                    ("WORKLOAD_SECRETS", list.clone()),
                    (binding_name, vault_binding(platform)),
                ]),
                &reads,
            );
            assert!(
                error.contains("gives the workload no deployer secret"),
                "{platform}: {error}"
            );
            check(platform, &accepted, &reads, "jobs", &env(&[])).unwrap();
        }
    }

    /// The deployer secrets vault on Azure is found in the install's group; a verifier that
    /// found none refuses the worker rather than trust the vault name it was sent.
    #[tokio::test]
    async fn an_azure_worker_needs_the_vault_the_verifier_read() {
        let accepted = accepted(Platform::Azure).await;
        let sent = env(&[
            (
                "WORKLOAD_SECRETS",
                signed_list(&accepted, "database-password"),
            ),
            (
                "ALIEN_DEPLOYER_SECRETS_BINDING",
                vault_binding(Platform::Azure),
            ),
        ]);
        let error = check(Platform::Azure, &accepted, &Reads::default(), "api", &sent)
            .expect_err("no vault was read");
        assert!(
            error.contains("deployer secrets vault has no value"),
            "{error}"
        );
    }
}
