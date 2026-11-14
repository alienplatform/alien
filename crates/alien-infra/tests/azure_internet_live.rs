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
//! Env: `AZURE_TARGET_SUBSCRIPTION_ID`, `AZURE_TARGET_TENANT_ID`, `AZURE_TARGET_CLIENT_ID`,
//! `AZURE_TARGET_CLIENT_SECRET` (a principal with the setup's management roles),
//! `AZURE_INTERNET_STACK` (the render test's `stack.json`), `AZURE_INTERNET_REGISTRATION` (the
//! `deployment_registration` input, as JSON), `AZURE_INTERNET_BINDING_OUT` (where the sandbox
//! binding is written).
//!
//! ```text
//! cargo test -p alien-infra --features all-platforms,test-utils --test azure_internet_live -- --ignored --nocapture
//! ```

#![cfg(feature = "azure")]

use alien_core::{
    import::ImportContext, AzureClientConfig, AzureCredentials, ClientConfig, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, Platform, ResourceStatus, Stack,
    StackSettings, StackState,
};
use alien_infra::{ImporterRegistry, StackExecutor, StackResourceStateExt};
use serde_json::Value;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

/// Terraform renders some payload fields as JSON text and some as objects.
fn json_field(registration: &Value, field: &str) -> Value {
    match &registration[field] {
        Value::String(text) => serde_json::from_str(text).unwrap_or_else(|error| {
            panic!("registration field {field} is not JSON: {error}")
        }),
        other => other.clone(),
    }
}

#[tokio::test]
#[ignore = "runs against a real subscription with the setup module applied; see the module docs"]
async fn the_executor_publishes_the_internet_binding_on_the_applied_setup() {
    let stack: Stack = serde_json::from_slice(
        &std::fs::read(env("AZURE_INTERNET_STACK")).expect("stack file"),
    )
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

    let client_config = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
        tenant_id: env("AZURE_TARGET_TENANT_ID"),
        region: Some(region),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: env("AZURE_TARGET_CLIENT_ID"),
            client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
        },
        service_overrides: None,
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
    let executor = StackExecutor::builder(&stack, client_config)
        .deployment_config(&config)
        .build()
        .expect("the executor accepts the prepared stack");

    let mut state = executor
        .continue_imported(state)
        .await
        .expect("imported setup state continues")
        .next_state;
    let finished = executor.run_until_synced(state.clone()).await;
    state = finished.final_state;
    assert!(
        finished.success,
        "the executor did not sync: {:?}",
        finished.error
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
}
