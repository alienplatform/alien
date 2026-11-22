//! The deployment controllers publishing Azure `internet` egress on a real setup. Ignored: it
//! needs the Terraform module that `alien-terraform`'s `azure_internet_live_render` test renders,
//! applied in a real subscription. `scripts/azure-internet-live.sh` runs the whole sequence.
//!
//! This is the runtime half of setup: it imports the module's registration into a stack state the
//! way the manager does, then runs the real `StackExecutor` over the prepared stack, so the
//! sandbox controller builds the disk image, checks the network, writes the signing key, replica
//! secret and gateway config to the deployment's Key Vault, and publishes its binding. The
//! executor accepting the prepared stack (its gateway `Container` declaration included) is part
//! of what this proves.
//!
//! Env: `AZURE_TARGET_SUBSCRIPTION_ID`, `AZURE_TARGET_TENANT_ID`, `AZURE_TARGET_CLIENT_ID`
//! (a principal with the setup's management roles; its federated token file in CI, `az login` locally),
//! `AZURE_INTERNET_STACK` (the render test's `stack.json`), `AZURE_INTERNET_REGISTRATION` (the
//! `deployment_registration` input, as JSON), `AZURE_INTERNET_GATEWAY_BINDING_OUT` (the gateway Container binding), `AZURE_INTERNET_BINDING_OUT` (where the sandbox
//! binding is written).
//!
//! ```text
//! cargo test -p alien-infra --features all-platforms,test-utils --test azure_internet_live -- --ignored --nocapture
//! ```

#![cfg(feature = "azure")]

use alien_core::{
    import::ImportContext, AzureClientConfig, AzureCredentials, ClientConfig, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, InitialSetupAuthority, Platform,
    ResourceLifecycle, ResourceStatus, Stack, StackSettings, StackState,
};
use alien_infra::{ImporterRegistry, RunningResourcePolicy, StackExecutor, StackResourceStateExt};
use serde_json::Value;
use std::time::Duration;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

/// Terraform renders some payload fields as JSON text and some as objects.
fn json_field(registration: &Value, field: &str) -> Value {
    match &registration[field] {
        Value::String(text) => serde_json::from_str(text)
            .unwrap_or_else(|error| panic!("registration field {field} is not JSON: {error}")),
        other => other.clone(),
    }
}

/// Prints what is not Running, with the failure the controller recorded, so a stuck or failed
/// step explains itself in the log.
fn report(state: &StackState) {
    for (id, resource) in &state.resources {
        if resource.status != ResourceStatus::Running {
            eprintln!("[state] {id}: {:?} {:?}", resource.status, resource.error);
        }
    }
}

#[tokio::test]
#[ignore = "runs against a real subscription with the setup module applied; see the module docs"]
async fn the_executor_publishes_the_internet_binding_on_the_applied_setup() {
    let stack: Stack =
        serde_json::from_slice(&std::fs::read(env("AZURE_INTERNET_STACK")).expect("stack file"))
            .expect("the prepared stack deserializes");
    let registration: Value = serde_json::from_slice(
        &std::fs::read(env("AZURE_INTERNET_REGISTRATION")).expect("registration file"),
    )
    .expect("registration JSON");
    let settings: StackSettings =
        serde_json::from_value(json_field(&registration, "stack_settings")).expect("settings");
    let resources = json_field(&registration, "resources");
    let region = registration["region"].as_str().expect("region").to_string();
    let prefix = registration["resource_prefix"]
        .as_str()
        .expect("resource_prefix")
        .to_string();

    let mut state = StackState::with_resource_prefix(Platform::Azure, prefix);
    let registry = ImporterRegistry::built_in();
    for imported in resources.as_array().expect("resources") {
        let id = imported["id"].as_str().expect("resource id");
        let entry = stack
            .resources
            .get(id)
            .unwrap_or_else(|| panic!("registration names {id}, which the stack does not declare"));
        let context = ImportContext {
            resource_id: id,
            platform: Platform::Azure,
            region: &region,
            stack_settings: &settings,
            management_config: None,
            resource: entry,
        };
        let resource = registry
            .run(
                &entry.config.resource_type(),
                Platform::Azure,
                imported["importData"].clone(),
                &context,
            )
            .unwrap_or_else(|error| panic!("import of {id} failed: {error}"));
        state.resources.insert(id.to_string(), resource);
    }

    let client_config = ClientConfig::Azure(Box::new({
    let mut config = alien_azure_clients::live_test::live_test_config("TARGET");
    config.region = Some(region);
    config
}));
    let config = DeploymentConfig::builder()
        .stack_settings(settings)
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    // The deployment's two phases, built as `initial_setup.rs` and `provisioning.rs` build them:
    // setup-owned (Frozen) resources continue from the import one step at a time, then the Live
    // phase steps over the same state, which is where the gateway declaration is accepted.
    let frozen = StackExecutor::builder(&stack, client_config.clone())
        .deployment_config(&config)
        .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
        .lifecycle_filter(vec![ResourceLifecycle::Frozen])
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .step_out_of_scope_resources(false)
        .build()
        .expect("the executor accepts the prepared stack");
    let settled = |state: &StackState, lifecycle: ResourceLifecycle| {
        state
            .resources
            .values()
            .filter(|resource| resource.lifecycle == Some(lifecycle))
            .all(|resource| resource.status == ResourceStatus::Running)
    };
    for step in 0..400 {
        let before = state.clone();
        let result = match frozen.continue_imported(state).await {
            Ok(result) => result,
            Err(error) => {
                report(&before);
                panic!("setup step {step} failed: {error}")
            }
        };
        state = result.next_state;
        report(&state);
        if settled(&state, ResourceLifecycle::Frozen) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(
            result.suggested_delay_ms.unwrap_or(500),
        ))
        .await;
    }
    assert!(
        settled(&state, ResourceLifecycle::Frozen),
        "the setup phase did not settle: {:#?}",
        state
            .resources
            .iter()
            .map(|(id, resource)| (id.clone(), resource.status))
            .collect::<Vec<_>>()
    );
    let live = StackExecutor::builder(&stack, client_config)
        .deployment_config(&config)
        .lifecycle_filter(vec![ResourceLifecycle::Live])
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .build()
        .expect("the executor accepts the prepared stack in the Live phase");
    for step in 0..400 {
        let result = live
            .step(state)
            .await
            .unwrap_or_else(|error| panic!("live step {step} failed: {error}"));
        state = result.next_state;
        report(&state);
        if settled(&state, ResourceLifecycle::Live) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(
            result.suggested_delay_ms.unwrap_or(500),
        ))
        .await;
    }
    assert!(
        settled(&state, ResourceLifecycle::Live),
        "the Live phase did not settle: {:#?}",
        state
            .resources
            .iter()
            .map(|(id, resource)| (id.clone(), resource.status))
            .collect::<Vec<_>>()
    );

    for (id, resource) in &state.resources {
        eprintln!("[state] {id}: {:?}", resource.status);
    }
    let sandbox = &state.resources["agents"];
    assert_eq!(sandbox.status, ResourceStatus::Running);
    let binding = sandbox
        .get_internal_controller()
        .expect("controller state")
        .expect("the sandbox has a controller")
        .get_binding_params()
        .expect("binding params")
        .expect("the sandbox publishes a binding");
    assert!(
        binding["egress"].get("kid").is_some() && binding["egress"].get("replicas").is_some(),
        "the published binding must carry the gateway handoff: {binding}"
    );
    std::fs::write(
        env("AZURE_INTERNET_BINDING_OUT"),
        serde_json::to_vec_pretty(&binding).expect("binding serializes"),
    )
    .expect("binding file");
    // The gateway Container's binding as its own controller published it: the driver loads the
    // sandbox with this exact value, so nothing here is hand-built.
    let gateway = &state.resources["sandbox-egress-gateway"];
    let gateway_binding = gateway
        .get_internal_controller()
        .expect("controller state")
        .expect("the gateway has a controller")
        .get_binding_params()
        .expect("binding params")
        .expect("the gateway Container publishes a binding");
    std::fs::write(
        env("AZURE_INTERNET_GATEWAY_BINDING_OUT"),
        serde_json::to_vec_pretty(&gateway_binding).expect("binding serializes"),
    )
    .expect("gateway binding file");
}
