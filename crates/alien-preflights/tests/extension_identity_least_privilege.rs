//! Which identity holds an extension's permission sets once setup has run its mutations, on AWS,
//! GCP and Azure, for a Frozen and a Live zone, with manual and automatic setup. Only the
//! extension's identity holds them, and only its type's `provision`, `management` and
//! `heartbeat` sets; a workload that links the zone gets the link's set; the management identity
//! and every other identity get none.

use alien_core::{
    permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference},
    AwsManagementConfig, AzureManagementConfig, CustomResource, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExtensionManifest, ExternalBindings, GcpManagementConfig,
    ManagementConfig, Platform, ResourceLifecycle, ResourceRef, ResourceType, ServiceAccount,
    SetupManagement, Stack, StackSettings, StackState, Worker, WorkerCode,
};
use alien_preflights::runner::PreflightRunner;
use serde_json::json;
use std::collections::BTreeSet;

const ZONE: &str = "acme/dns-zone";

fn set(name: &str) -> serde_json::Value {
    json!({
        "id": format!("{ZONE}/{name}"),
        "description": name,
        "platforms": {
            "aws": [{ "grant": { "actions": [format!("route53:{name}")] },
                "binding": { "stack": { "resources": ["*"] }, "resource": { "resources": ["*"] } } }],
            "gcp": [{ "grant": { "permissions": [format!("dns.{name}.get")] },
                "binding": { "stack": { "scope": "projects/${projectName}" }, "resource": { "scope": "projects/${projectName}" } } }],
            "azure": [{ "grant": { "actions": [format!("Microsoft.Network/{name}/read")] },
                "binding": {
                    "stack": { "scope": "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}" },
                    "resource": { "scope": "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}" } } }]
        }
    })
}

fn acme() -> ExtensionManifest {
    serde_json::from_value(json!({
        "name": "acme", "version": "0.1.0", "digest": "sha256:4ff931a2",
        "resourceTypes": { ZONE: {
            "lifecycles": ["frozen", "live"], "holdsData": false,
            "setup": { "cloudformation": ["aws"], "terraform": ["aws", "gcp", "azure"], "helm": [], "importers": ["aws", "gcp", "azure"] },
            "controllerPlatforms": ["aws", "gcp", "azure"],
            "permissionSets": [set("provision"), set("management"), set("heartbeat"), set("records-write"), set("data-read")],
            "link": { "permissionSets": [format!("{ZONE}/records-write")] },
            "schemas": { "config": { "type": "object" }, "outputs": { "type": "object" } }
        } }
    }))
    .unwrap()
}

fn stack(with_extension: bool, zone_lifecycle: ResourceLifecycle) -> Stack {
    let mut api = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "registry.example.com/api:1".to_string(),
        })
        .permissions("api".to_string())
        .build();
    let mut builder = Stack::new("my-app".to_string());
    if with_extension {
        api.links
            .push(ResourceRef::new(ResourceType::from(ZONE), "zone"));
        let zone = CustomResource::from_untagged(ZONE, json!({ "id": "zone" })).unwrap();
        builder = builder.add(zone, zone_lifecycle);
    }
    let mut stack = builder
        .add(api, ResourceLifecycle::Live)
        .permission("api", PermissionProfile::new())
        .build();
    if with_extension {
        stack.extensions = vec![acme()];
    }
    stack
}

fn config(platform: Platform, setup: SetupManagement) -> DeploymentConfig {
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
    config.stack_settings.setup_management = setup;
    config.management_config = Some(match platform {
        Platform::Gcp => ManagementConfig::Gcp(GcpManagementConfig {
            service_account_email: "manager@vendor.iam.gserviceaccount.com".to_string(),
            federation_trust: None,
        }),
        Platform::Azure => ManagementConfig::Azure(AzureManagementConfig {
            managing_tenant_id: "vendor-tenant".to_string(),
            oidc_issuer: "https://manager.example.com".to_string(),
            oidc_subject: "manager".to_string(),
        }),
        _ => ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "arn:aws:iam::111122223333:role/manager".to_string(),
        }),
    });
    config
}

fn ids(references: &[PermissionSetReference]) -> Vec<String> {
    references.iter().map(|r| r.id().to_string()).collect()
}

/// Every (holder, scope, set id) in the prepared stack: profiles, the management profile, and
/// every service account's sets.
fn holders(stack: &Stack) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (name, profile) in &stack.permissions.profiles {
        for (scope, refs) in &profile.0 {
            for id in ids(refs) {
                out.insert(format!("profile:{name} {scope} {id}"));
            }
        }
    }
    let management = match &stack.permissions.management {
        ManagementPermissions::Auto => None,
        ManagementPermissions::Extend(p) | ManagementPermissions::Override(p) => Some(p),
    };
    if let Some(profile) = management {
        for (scope, refs) in &profile.0 {
            for id in ids(refs) {
                out.insert(format!("management {scope} {id}"));
            }
        }
    }
    for (resource_id, entry) in &stack.resources {
        if let Some(account) = entry.config.downcast_ref::<ServiceAccount>() {
            for set in &account.stack_permission_sets {
                out.insert(format!("sa:{resource_id} stack {}", set.id));
            }
            for (scope, sets) in &account.resource_permission_sets {
                for set in sets {
                    out.insert(format!("sa:{resource_id} {scope} {}", set.id));
                }
            }
        }
    }
    out
}

#[tokio::test]
async fn only_the_extensions_identity_holds_its_sets() {
    for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        for setup in [SetupManagement::Manual, SetupManagement::Automatic] {
            for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
                let case = format!("{platform:?} {setup:?} zone={lifecycle:?}");
                let run = |with| async move {
                    PreflightRunner::new()
                        .apply_mutations(
                            stack(with, lifecycle),
                            &StackState::new(platform),
                            &config(platform, setup),
                        )
                        .await
                        .map(|stack| holders(&stack))
                };
                let base = run(false)
                    .await
                    .unwrap_or_else(|error| panic!("{case}: {error}"));
                let with = run(true)
                    .await
                    .unwrap_or_else(|error| panic!("{case}: {error}"));
                assert!(
                    base.difference(&with).next().is_none(),
                    "{case}: the extension takes grants away"
                );
                let identity: BTreeSet<_> = ["provision", "management", "heartbeat"]
                    .iter()
                    .map(|operation| format!("sa:acme-ext stack {ZONE}/{operation}"))
                    .collect();
                let added: BTreeSet<_> = with
                    .difference(&base)
                    .filter(|entry| entry.contains("acme/"))
                    .cloned()
                    .collect();
                let (link, rest): (BTreeSet<_>, BTreeSet<_>) =
                    added.difference(&identity).cloned().partition(|entry| {
                        entry.ends_with(&format!("{ZONE}/records-write"))
                            && (entry.starts_with("profile:api ")
                                || entry.starts_with("sa:api-sa "))
                    });
                assert!(identity.is_subset(&added), "{case}: {added:#?}");
                assert!(
                    !link.is_empty(),
                    "{case}: the linking workload gets the link's set"
                );
                assert!(
                    rest.is_empty(),
                    "{case}: other holders of extension sets: {rest:#?}"
                );
                assert!(
                    with.iter()
                        .filter(|entry| entry.starts_with("management "))
                        .eq(base.iter().filter(|entry| entry.starts_with("management "))),
                    "{case}: the management profile changes"
                );
            }
        }
    }
}
