//! A service an extension type asks for never takes the place of one of Alien's activations.

use alien_core::{
    CustomResource, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform,
    ResourceLifecycle, ServiceActivation, SetupManagement, Stack, StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;
use serde_json::json;
use std::collections::BTreeMap;

const ZONE: &str = "acme/dns-zone";

fn descriptor() -> serde_json::Value {
    json!({
        "lifecycles": ["frozen", "live"],
        "holdsData": false,
        "setup": { "cloudformation": ["aws"], "terraform": ["aws", "gcp", "azure"], "helm": ["kubernetes"], "importers": ["aws", "gcp", "azure"] },
        "controllerPlatforms": ["aws", "gcp", "azure"],
        "permissionSets": [],
        "link": { "permissionSets": [] },
        "schemas": { "config": { "type": "object" }, "outputs": { "type": "object" } }
    })
}

fn zone_stack(edit: impl FnOnce(&mut serde_json::Value)) -> Stack {
    let mut d = descriptor();
    edit(&mut d);
    let zone =
        CustomResource::from_untagged(ZONE, json!({ "id": "zone", "domain": "x.dev" })).unwrap();
    let mut stack = Stack::new("my-app".to_string())
        .add(zone, ResourceLifecycle::Frozen)
        .build();
    stack.extensions = vec![serde_json::from_value(json!({
        "name": "acme", "version": "0.1.0", "digest": "sha256:4ff9", "resourceTypes": { ZONE: d }
    }))
    .unwrap()];
    stack
}

fn config(base: Option<Platform>) -> DeploymentConfig {
    let mut config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    config.stack_settings.setup_management = SetupManagement::Manual;
    config.base_platform = base;
    config
}

async fn prepare(stack: Stack, platform: Platform, base: Option<Platform>) -> Stack {
    PreflightRunner::new()
        .apply_mutations(stack, &StackState::new(platform), &config(base))
        .await
        .unwrap()
}

fn activations(stack: &Stack) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::<String, Vec<String>>::new();
    for (id, entry) in stack.resources() {
        if let Some(a) = entry.config.downcast_ref::<ServiceActivation>() {
            out.entry(a.service_name.clone())
                .or_default()
                .push(id.clone());
        }
    }
    out
}

/// A service name whose derived id equals one of Alien's ids must not replace Alien's activation.
#[tokio::test]
async fn gcp_extension_id_does_not_replace_alien_s_activation() {
    let prepared = prepare(
        zone_stack(|d| d["services"] = json!({ "gcp": ["cloud.quotas"] })),
        Platform::Gcp,
        None,
    )
    .await;
    let a = activations(&prepared);
    assert!(
        a.contains_key("cloudquotas.googleapis.com"),
        "Alien's Cloud Quotas activation is gone: {a:?}"
    );
}

#[tokio::test]
async fn azure_extension_id_does_not_replace_alien_s_activation() {
    let mut stack = zone_stack(|d| d["services"] = json!({ "azure": ["App"] }));
    let api = alien_core::Worker::new("api".to_string())
        .code(alien_core::WorkerCode::Image {
            image: "r/a:1".to_string(),
        })
        .permissions("api".to_string())
        .build();
    stack.resources.insert(
        "api".to_string(),
        alien_core::ResourceEntry {
            config: alien_core::Resource::new(api),
            lifecycle: ResourceLifecycle::Live,
            dependencies: vec![],
            remote_access: false,
            enabled_when: None,
            display_name: None,
            description: None,
        },
    );
    stack.permissions.profiles.insert(
        "api".to_string(),
        alien_core::permissions::PermissionProfile::new(),
    );
    let prepared = prepare(stack, Platform::Azure, None).await;
    let a = activations(&prepared);
    assert!(
        a.contains_key("Microsoft.App"),
        "Alien's Microsoft.App activation is gone: {a:?}"
    );
}

/// Azure provider names an extension spells differently: the same provider in another case is
/// one activation, and two names sharing an id are refused rather than dropping every Azure
/// activation.
mod azure_spellings {
    use alien_core::{
        CustomResource, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform,
        ResourceEntry, ResourceLifecycle, ServiceActivation, SetupManagement, Stack, StackSettings,
        StackState,
    };
    use alien_preflights::runner::PreflightRunner;
    use serde_json::json;

    fn descriptor(services: serde_json::Value) -> serde_json::Value {
        json!({
            "lifecycles": ["frozen", "live"],
            "holdsData": false,
            "setup": { "cloudformation": ["aws"], "terraform": ["aws", "gcp", "azure"], "helm": ["kubernetes"], "importers": ["aws", "gcp", "azure"] },
            "controllerPlatforms": ["aws", "gcp", "azure"],
            "permissionSets": [],
            "link": { "permissionSets": [] },
            "schemas": { "config": { "type": "object" }, "outputs": { "type": "object" } },
            "services": services,
        })
    }

    fn config() -> DeploymentConfig {
        let mut config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        config.stack_settings.setup_management = SetupManagement::Manual;
        config
    }

    #[tokio::test]
    async fn two_extensions_spelling_one_azure_provider_in_two_cases_get_one_activation() {
        let mut stack = Stack::new("my-app".to_string()).build();
        let mut extensions = Vec::new();
        for (extension, spelling) in [("acme", "Microsoft.Dns"), ("globex", "microsoft.dns")] {
            let resource_type = format!("{extension}/zone");
            let resource =
                CustomResource::from_untagged(&resource_type, json!({ "id": extension })).unwrap();
            stack.resources.insert(
                extension.to_string(),
                ResourceEntry::new(resource, ResourceLifecycle::Frozen),
            );
            extensions.push(
                serde_json::from_value(json!({
                    "name": extension, "version": "0.1.0", "digest": "sha256:4ff9",
                    "resourceTypes": { resource_type: descriptor(json!({ "azure": [spelling] })) }
                }))
                .unwrap(),
            );
        }
        stack.extensions = extensions;

        let prepared = PreflightRunner::new()
            .apply_mutations(stack, &StackState::new(Platform::Azure), &config())
            .await
            .expect("the same provider in two cases is one service, not a collision");
        let dns: Vec<&String> = prepared
            .resources()
            .filter(|(_, entry)| {
                entry
                    .config
                    .downcast_ref::<ServiceActivation>()
                    .is_some_and(|a| a.service_name.eq_ignore_ascii_case("microsoft.dns"))
            })
            .map(|(id, _)| id)
            .collect();
        assert_eq!(dns, ["extension-service-microsoft-dns"]);
    }

    /// Two Azure names that share an activation id (reachable only when the name check was skipped)
    /// are refused on Azure as on GCP, not dropped with every other Azure activation.
    #[tokio::test]
    async fn two_azure_names_sharing_an_id_are_refused() {
        let resource = CustomResource::from_untagged("acme/zone", json!({ "id": "zone" })).unwrap();
        let mut stack = Stack::new("my-app".to_string())
            .add(resource, ResourceLifecycle::Frozen)
            .build();
        stack.extensions = vec![serde_json::from_value(json!({
            "name": "acme", "version": "0.1.0", "digest": "sha256:4ff9",
            "resourceTypes": { "acme/zone": descriptor(json!({ "azure": ["Microsoft.Dns", "Microsoft-Dns"] })) }
        }))
        .unwrap()];
        let error = PreflightRunner::new()
            .apply_mutations(stack, &StackState::new(Platform::Azure), &config())
            .await
            .expect_err("a shared id is refused")
            .to_string();
        assert!(error.contains("already enables 'Microsoft.Dns'"), "{error}");
    }
}
