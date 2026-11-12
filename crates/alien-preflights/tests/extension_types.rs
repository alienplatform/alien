//! Preflights for resource types an extension defines, read from the descriptors in
//! `Stack.extensions`. The stack is the docs' DNS zone from the `acme` extension, linked to a
//! worker.

use alien_core::{
    permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference},
    AwsManagementConfig, AzureManagementConfig, CustomResource, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExtensionManifest, ExternalBindings, GcpManagementConfig,
    ManagementConfig, Platform, ResourceLifecycle, ResourceRef, ResourceType, ServiceAccount,
    ServiceActivation, SetupManagement, Stack, StackSettings, StackState, Worker, WorkerCode,
};
use alien_preflights::compile_time::{
    AllowedUserResourcesCheck, FrozenResourceLifecycleCheck, LiveProvisionPermissionsCheck,
    PermissionSetsExistCheck,
};
use alien_preflights::mutations::ResourceLinkPermissionsMutation;
use alien_preflights::runner::PreflightRunner;
use alien_preflights::{ClusterCloud, CompileTimeCheck, StackMutation};
use serde_json::json;
use std::collections::BTreeMap;

const ZONE: &str = "acme/dns-zone";

/// The descriptor `alien extensions build` writes for the docs' DNS zone.
fn dns_zone_descriptor() -> serde_json::Value {
    // Creating a zone takes no resource; every other call names the zone.
    let set = |name: &str, actions: &[&str]| {
        let entries: Vec<_> = [true, false]
            .into_iter()
            .filter_map(|creates| {
                let actions: Vec<_> = actions
                    .iter()
                    .filter(|action| (**action == "route53:CreateHostedZone") == creates)
                    .collect();
                let resource = if creates { "*" } else { "arn:aws:route53:::hostedzone/*" };
                (!actions.is_empty()).then(|| json!({
                    "grant": { "actions": actions },
                    "binding": { "stack": { "resources": [resource] }, "resource": { "resources": [resource] } }
                }))
            })
            .collect();
        json!({
            "id": format!("{ZONE}/{name}"),
            "description": name,
            "platforms": { "aws": entries }
        })
    };
    json!({
        "lifecycles": ["frozen", "live"],
        "holdsData": false,
        "setup": {
            "cloudformation": ["aws"],
            "terraform": ["aws", "gcp", "azure"],
            "helm": ["kubernetes"],
            "importers": ["aws", "gcp", "azure"]
        },
        "controllerPlatforms": ["aws", "gcp", "azure"],
        "permissionSets": [
            set("provision", &["route53:CreateHostedZone", "route53:DeleteHostedZone"]),
            set("records-write", &["route53:ChangeResourceRecordSets"])
        ],
        "link": { "permissionSets": [format!("{ZONE}/records-write")] },
        "schemas": { "config": { "type": "object" }, "outputs": { "type": "object" } }
    })
}

fn manifest(name: &str, descriptor: serde_json::Value) -> ExtensionManifest {
    serde_json::from_value(json!({
        "name": name,
        "version": "0.1.0",
        "digest": "sha256:4ff931a2",
        "resourceTypes": { ZONE: descriptor }
    }))
    .unwrap()
}

fn acme() -> ExtensionManifest {
    manifest("acme", dns_zone_descriptor())
}

/// The docs' stack: a zone, and a worker that links it with no profile entry for it.
fn stack(zone_lifecycle: ResourceLifecycle, extensions: Vec<ExtensionManifest>) -> Stack {
    let zone =
        CustomResource::from_untagged(ZONE, json!({ "id": "zone", "domain": "internal.acme.dev" }))
            .unwrap();
    let mut api = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "registry.example.com/api:1".to_string(),
        })
        .permissions("api".to_string())
        .build();
    api.links
        .push(ResourceRef::new(ResourceType::from(ZONE), "zone"));
    let mut stack = Stack::new("my-app".to_string())
        .add(zone, zone_lifecycle)
        .add(api, ResourceLifecycle::Live)
        .permission("api", PermissionProfile::new())
        .build();
    stack.extensions = extensions;
    stack
}

fn config(setup_management: SetupManagement) -> DeploymentConfig {
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
    config.stack_settings.setup_management = setup_management;
    config
}

async fn errors(check: &dyn CompileTimeCheck, stack: &Stack, platform: Platform) -> Vec<String> {
    check.check(stack, platform).await.unwrap().errors
}

/// The errors on a Kubernetes cluster that runs where `cloud` says.
async fn errors_on_kubernetes(
    check: &dyn CompileTimeCheck,
    stack: &Stack,
    cloud: ClusterCloud,
) -> Vec<String> {
    check
        .check_on(stack, Platform::Kubernetes, cloud)
        .await
        .unwrap()
        .errors
}

#[tokio::test]
async fn a_custom_type_a_listed_extension_declares_is_allowed() {
    let stack = stack(ResourceLifecycle::Frozen, vec![acme()]);
    assert_eq!(
        errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await,
        Vec::<String>::new()
    );
}

/// The release that removes an extension before its resources.
#[tokio::test]
async fn a_custom_type_no_extension_declares_names_the_extension_and_its_resources() {
    let stack = stack(ResourceLifecycle::Frozen, Vec::new());
    let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("Resources 'zone' have type 'acme/dns-zone', which no extension")
            && errors[0].contains("extension 'acme'"),
        "{errors:?}"
    );
}

/// Types sit under their extension's name, so this also refuses a type two extensions declare.
#[tokio::test]
async fn two_extensions_with_one_name_are_refused() {
    let mut other_version: serde_json::Value = serde_json::to_value(acme()).unwrap();
    other_version["version"] = json!("0.2.0");
    other_version["digest"] = json!("");
    let other_type = other_version["resourceTypes"][ZONE].clone();
    other_version["resourceTypes"] = json!({ "acme/record": other_type });
    let versions = stack(
        ResourceLifecycle::Frozen,
        vec![acme(), serde_json::from_value(other_version).unwrap()],
    );
    let refused = errors(&AllowedUserResourcesCheck, &versions, Platform::Aws).await;
    assert!(
        refused.contains(&"The stack lists more than one extension named 'acme': 'acme' 0.1.0 (sha256:4ff931a2), 'acme' 0.2.0. Keep only one of them".to_string()),
        "{refused:?}"
    );

    let stack = stack(ResourceLifecycle::Frozen, vec![acme(), acme()]);
    let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("more than one extension named 'acme'"),
        "{errors:?}"
    );
}

/// A type name may nest, such as `acme/dns/zone`; the extension's name is its first segment.
#[tokio::test]
async fn a_nested_type_is_allowed_and_its_permission_sets_resolve() {
    const NESTED: &str = "acme/dns/zone";
    let mut descriptor = dns_zone_descriptor();
    for set in descriptor["permissionSets"].as_array_mut().unwrap() {
        let id = set["id"].as_str().unwrap().replace(ZONE, NESTED);
        set["id"] = json!(id);
    }
    descriptor["link"] = json!({ "permissionSets": [format!("{NESTED}/records-write")] });
    let mut stack = Stack::new("my-app".to_string())
        .add(
            CustomResource::from_untagged(NESTED, json!({ "id": "zone" })).unwrap(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.extensions = vec![serde_json::from_value(json!({
        "name": "acme",
        "version": "0.1.0",
        "digest": "sha256:4ff931a2",
        "resourceTypes": { NESTED: descriptor }
    }))
    .unwrap()];

    assert_eq!(
        errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await,
        Vec::<String>::new()
    );
    let resolved = alien_permissions::get_stack_permission_set(&stack, "acme/dns/zone/provision");
    assert_eq!(
        resolved.map(|set| set.id.as_str()),
        Some("acme/dns/zone/provision")
    );
}

#[tokio::test]
async fn a_lifecycle_the_type_does_not_declare_is_refused() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["lifecycles"] = json!(["frozen"]);
    let stack = stack(ResourceLifecycle::Live, vec![manifest("acme", descriptor)]);
    let errors = errors(&FrozenResourceLifecycleCheck, &stack, Platform::Aws).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("'zone' of type 'acme/dns-zone' has lifecycle Live, but its extension allows only [Frozen]"),
        "{errors:?}"
    );
}

/// A type with no controller for the platform can only be Frozen there.
#[tokio::test]
async fn live_needs_a_controller_for_the_platform() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["controllerPlatforms"] = json!(["aws"]);
    let stack = stack(ResourceLifecycle::Live, vec![manifest("acme", descriptor)]);

    assert!(errors(&FrozenResourceLifecycleCheck, &stack, Platform::Aws)
        .await
        .is_empty());
    let errors = errors(&FrozenResourceLifecycleCheck, &stack, Platform::Gcp).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0]
            .contains("has lifecycle Live, but its extension has no controller for platform 'gcp'"),
        "{errors:?}"
    );
}

/// Setup's template creates a Frozen-only resource, and only an importer reads it back, even
/// where a controller could create it from the setup CLI.
#[tokio::test]
async fn a_frozen_only_type_needs_an_importer_for_the_platform() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["lifecycles"] = json!(["frozen"]);
    descriptor["controllerPlatforms"] = json!(["aws", "azure"]);
    descriptor["setup"]["importers"] = json!(["aws"]);
    let stack = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor)],
    );

    assert!(errors(&FrozenResourceLifecycleCheck, &stack, Platform::Aws)
        .await
        .is_empty());
    let errors = errors(&FrozenResourceLifecycleCheck, &stack, Platform::Azure).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("'zone' of type 'acme/dns-zone' can only be Frozen on platform 'azure', but its extension has no importer for it"),
        "{errors:?}"
    );
}

/// A type that declares Live but ships no controller for the platform can only be Frozen there.
#[tokio::test]
async fn a_type_with_no_controller_on_the_platform_needs_an_importer_there() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["controllerPlatforms"] = json!(["aws"]);
    descriptor["setup"]["importers"] = json!(["aws"]);
    let stack = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor)],
    );

    assert!(errors(&FrozenResourceLifecycleCheck, &stack, Platform::Aws)
        .await
        .is_empty());
    let errors = errors(&FrozenResourceLifecycleCheck, &stack, Platform::Gcp).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("'zone' of type 'acme/dns-zone' can only be Frozen on platform 'gcp', but its extension has no importer for it"),
        "{errors:?}"
    );
}

/// `alien dev` builds no setup template, so nothing needs importing there. A cloud does.
#[tokio::test]
async fn alien_dev_needs_no_importer() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["lifecycles"] = json!(["frozen"]);
    descriptor["controllerPlatforms"] = json!(["aws", "local"]);
    descriptor["setup"]["importers"] = json!(["aws"]);
    let stack = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor)],
    );

    assert_eq!(
        errors(&FrozenResourceLifecycleCheck, &stack, Platform::Local).await,
        Vec::<String>::new()
    );
    let errors = errors(&FrozenResourceLifecycleCheck, &stack, Platform::Gcp).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains(
            "can only be Frozen on platform 'gcp', but its extension has no importer for it"
        ),
        "{errors:?}"
    );
}

/// The eks, gke and aks targets run the template preflights on Kubernetes. Their module's cloud
/// emitters create the resource, and the manager imports it with the base platform's importer,
/// so a Helm emitter does not stand in for an importer there.
#[tokio::test]
async fn a_frozen_only_type_with_only_a_helm_emitter_is_refused_on_a_cluster_in_a_cloud() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["lifecycles"] = json!(["frozen"]);
    descriptor["controllerPlatforms"] = json!([]);
    descriptor["setup"] = json!({
        "cloudformation": [],
        "terraform": ["aws"],
        "helm": ["kubernetes"],
        "importers": []
    });
    let stack = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor)],
    );

    let errors = errors(&FrozenResourceLifecycleCheck, &stack, Platform::Aws).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains(
            "can only be Frozen on platform 'aws', but its extension has no importer for it"
        ),
        "{errors:?}"
    );
    let errors = errors_on_kubernetes(
        &FrozenResourceLifecycleCheck,
        &stack,
        ClusterCloud::Cloud(Platform::Aws),
    )
    .await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains(
            "can only be Frozen on platform 'aws', the cloud its Kubernetes cluster runs in, but its extension has no importer for it"
        ),
        "{errors:?}"
    );
}

/// On a cluster in a cloud, setup creates the resource there and the cloud's importer reads it
/// back, whatever the type's Helm emitter says.
#[tokio::test]
async fn a_frozen_only_type_on_a_cluster_in_a_cloud_needs_that_cloud_s_importer() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["lifecycles"] = json!(["frozen"]);
    descriptor["controllerPlatforms"] = json!([]);
    descriptor["setup"]["importers"] = json!(["aws"]);
    descriptor["setup"]["helm"] = json!([]);
    let stack = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor)],
    );

    assert_eq!(
        errors_on_kubernetes(
            &FrozenResourceLifecycleCheck,
            &stack,
            ClusterCloud::Cloud(Platform::Aws)
        )
        .await,
        Vec::<String>::new()
    );
    for cloud in [Platform::Gcp, Platform::Azure] {
        let errors = errors_on_kubernetes(
            &FrozenResourceLifecycleCheck,
            &stack,
            ClusterCloud::Cloud(cloud),
        )
        .await;
        assert_eq!(errors.len(), 1, "{cloud:?}: {errors:?}");
        assert!(
            errors[0].contains(&format!(
                "can only be Frozen on platform '{}', the cloud its Kubernetes cluster runs in, but its extension has no importer for it",
                cloud.as_str()
            )),
            "{errors:?}"
        );
    }
}

/// On a cluster in no cloud, setup creates nothing: the chart asks the customer for an existing
/// resource through the Helm emitter, and the Operator binds it. No importer is asked for.
#[tokio::test]
async fn a_frozen_only_type_on_a_cluster_in_no_cloud_needs_a_helm_emitter() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["lifecycles"] = json!(["frozen"]);
    descriptor["controllerPlatforms"] = json!([]);
    descriptor["setup"]["importers"] = json!([]);
    let with_helm = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor.clone())],
    );
    assert_eq!(
        errors_on_kubernetes(
            &FrozenResourceLifecycleCheck,
            &with_helm,
            ClusterCloud::NoCloud
        )
        .await,
        Vec::<String>::new()
    );
    assert_eq!(
        errors(
            &FrozenResourceLifecycleCheck,
            &with_helm,
            Platform::Kubernetes
        )
        .await,
        Vec::<String>::new(),
        "a check that isn't told the cloud accepts a type that runs on one cluster"
    );

    descriptor["setup"]["helm"] = json!([]);
    descriptor["setup"]["importers"] = json!(["aws", "gcp", "azure"]);
    let without_helm = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor)],
    );
    let errors = errors_on_kubernetes(
        &FrozenResourceLifecycleCheck,
        &without_helm,
        ClusterCloud::NoCloud,
    )
    .await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains(
            "can only be Frozen on platform 'kubernetes', but its extension has no Helm emitter"
        ),
        "{errors:?}"
    );
}

/// A Live custom resource on a cluster in a cloud runs on that cloud's controller, so it needs
/// one there and none for Kubernetes.
#[tokio::test]
async fn live_on_a_cluster_in_a_cloud_needs_that_cloud_s_controller() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["controllerPlatforms"] = json!(["aws", "local"]);
    let stack = stack(ResourceLifecycle::Live, vec![manifest("acme", descriptor)]);

    assert_eq!(
        errors_on_kubernetes(
            &FrozenResourceLifecycleCheck,
            &stack,
            ClusterCloud::Cloud(Platform::Aws)
        )
        .await,
        Vec::<String>::new()
    );
    let errors = errors_on_kubernetes(
        &FrozenResourceLifecycleCheck,
        &stack,
        ClusterCloud::Cloud(Platform::Gcp),
    )
    .await;
    assert!(
        errors[0].contains(
            "has lifecycle Live, but its extension has no controller for platform 'gcp', the cloud its Kubernetes cluster runs in"
        ),
        "{errors:?}"
    );
    let errors =
        errors_on_kubernetes(&FrozenResourceLifecycleCheck, &stack, ClusterCloud::NoCloud).await;
    assert!(
        errors[0].contains(
            "has lifecycle Live, but its extension has no controller for platform 'kubernetes'"
        ),
        "{errors:?}"
    );
}

/// The template preflights for an eks module ask for the cloud's importer, and the chart's ask
/// for the Helm emitter.
#[tokio::test]
async fn the_template_preflights_read_the_cloud_under_the_cluster() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["lifecycles"] = json!(["frozen"]);
    descriptor["controllerPlatforms"] = json!([]);
    descriptor["setup"]["importers"] = json!([]);
    let stack = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor)],
    );
    let runner = PreflightRunner::new();

    let refused = runner
        .run_template_preflights_on(
            &stack,
            Platform::Kubernetes,
            ClusterCloud::Cloud(Platform::Aws),
        )
        .await
        .unwrap_err();
    assert!(
        format!("{refused:?}").contains(
            "the cloud its Kubernetes cluster runs in, but its extension has no importer"
        ),
        "{refused:?}"
    );
    runner
        .run_template_preflights(&stack, Platform::Kubernetes)
        .await
        .unwrap();
}

/// An extension's types, and so its permission sets, carry its own name.
#[tokio::test]
async fn a_type_outside_the_extensions_name_is_refused() {
    let stack = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("evil", dns_zone_descriptor())],
    );
    let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains(
            "Extension 'evil' declares resource type 'acme/dns-zone', which only an extension named 'acme' can define"
        ),
        "{errors:?}"
    );
}

#[tokio::test]
async fn a_permission_set_outside_its_type_is_refused() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["permissionSets"][0]["id"] = json!("acme/record/provision");
    let stack = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor)],
    );
    let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains(
            "Extension 'acme' defines permission set 'acme/record/provision' for resource type 'acme/dns-zone'"
        ),
        "{errors:?}"
    );
}

/// The provision, management and heartbeat sets go to the extension's identity, whose credentials
/// reach the extension's controller, so like Alien's own they never read the customer's data. A
/// link set, which goes to the customer's workloads, may.
#[tokio::test]
async fn an_identity_set_that_reads_stored_data_is_refused() {
    for actions in [json!(["s3:GetObject"]), json!(["s3:Get*"]), json!(["*"])] {
        let mut descriptor = dns_zone_descriptor();
        descriptor["permissionSets"][0]["platforms"]["aws"][0]["grant"]["actions"] =
            actions.clone();
        descriptor["permissionSets"][1]["platforms"]["aws"][0]["grant"]["actions"] =
            json!(["s3:GetObject"]);
        let stack = stack(
            ResourceLifecycle::Frozen,
            vec![manifest("acme", descriptor)],
        );
        let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
        assert_eq!(errors.len(), 1, "{actions}: {errors:?}");
        assert!(
            errors[0].contains(
                "Extension 'acme' permission set 'acme/dns-zone/provision' grants AWS action"
            ),
            "{errors:?}"
        );
    }
}

/// Only Alien makes an extension's identity, trusted by the manager; a stack can't declare one.
#[tokio::test]
async fn a_declared_service_account_with_an_extension_is_refused() {
    let mut stack = stack(ResourceLifecycle::Frozen, vec![acme()]);
    let account = ServiceAccount::new("acme-ext".to_string())
        .extension("acme".to_string())
        .build();
    let declared = Stack::new("declared".to_string())
        .add(account, ResourceLifecycle::Frozen)
        .build();
    stack.resources.extend(declared.resources);
    let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("Service account 'acme-ext' sets `extension`"),
        "{errors:?}"
    );
}

#[tokio::test]
async fn a_worker_that_links_the_zone_gets_the_link_permission_sets() {
    let stack = stack(ResourceLifecycle::Frozen, vec![acme()]);
    let prepared = ResourceLinkPermissionsMutation
        .mutate(
            stack,
            &StackState::new(Platform::Aws),
            &config(SetupManagement::Manual),
        )
        .await
        .unwrap();
    assert_eq!(
        prepared.permissions.profiles["api"].0["zone"],
        [PermissionSetReference::from_name(
            "acme/dns-zone/records-write"
        )]
    );
}

/// The docs' stack names the link set in the worker's profile, and preparation compiles it into
/// the worker's service account.
#[tokio::test]
async fn a_workload_profile_resolves_the_stack_extension_permission_sets() {
    let mut stack = stack(ResourceLifecycle::Frozen, vec![acme()]);
    stack.permissions.profiles.insert(
        "api".to_string(),
        PermissionProfile::new().resource("zone", ["acme/dns-zone/records-write"]),
    );
    assert_eq!(
        errors(&PermissionSetsExistCheck, &stack, Platform::Aws).await,
        Vec::<String>::new()
    );

    let prepared = PreflightRunner::new()
        .apply_mutations(
            stack.clone(),
            &StackState::new(Platform::Aws),
            &config(SetupManagement::Manual),
        )
        .await
        .unwrap();
    let account = prepared.resources["api-sa"]
        .config
        .downcast_ref::<ServiceAccount>()
        .unwrap();
    let granted: Vec<&str> = account.resource_permission_sets["zone"]
        .iter()
        .map(|set| set.id.as_str())
        .collect();
    assert_eq!(granted, ["acme/dns-zone/records-write"]);

    // Without the extension in the stack the name resolves to nothing.
    stack.extensions.clear();
    let errors = errors(&PermissionSetsExistCheck, &stack, Platform::Aws).await;
    assert!(
        errors[0].contains("unknown permission set 'acme/dns-zone/records-write'"),
        "{errors:?}"
    );
}

/// The management identity never holds an extension's permission sets: the extension's own
/// identity provisions its resources.
#[tokio::test]
async fn no_management_grant_holds_an_extension_set() {
    for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        for setup_management in [SetupManagement::Manual, SetupManagement::Automatic] {
            for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
                let prepared = PreflightRunner::new()
                    .apply_mutations(
                        stack(lifecycle, vec![acme()]),
                        &StackState::new(platform),
                        &config(setup_management),
                    )
                    .await
                    .unwrap_or_else(|error| {
                        panic!("{platform:?} {setup_management:?} {lifecycle:?}: {error}")
                    });
                let ManagementPermissions::Extend(profile) = prepared.management() else {
                    panic!("the management profile is derived");
                };
                let extension_grants: Vec<_> = profile
                    .0
                    .values()
                    .flatten()
                    .map(PermissionSetReference::id)
                    .filter(|id| id.starts_with("acme/"))
                    .collect();
                assert!(
                    extension_grants.is_empty(),
                    "{platform:?} {setup_management:?} {lifecycle:?}: {extension_grants:?}"
                );
            }
        }
    }
}

/// A management profile the developer writes can't grant an extension's set either.
#[tokio::test]
async fn a_written_management_profile_cannot_grant_an_extension_set() {
    let profile = PermissionProfile::new().resource("zone", ["acme/dns-zone/provision"]);
    for management in [
        ManagementPermissions::Extend(profile.clone()),
        ManagementPermissions::Override(profile),
    ] {
        let mut stack = stack(ResourceLifecycle::Live, vec![acme()]);
        stack.permissions.management = management;
        let errors = errors(&PermissionSetsExistCheck, &stack, Platform::Aws).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains(
                "The management profile grants 'acme/dns-zone/provision' for resource 'zone', but an extension's permission sets are granted only to the extension's own identity"
            ),
            "{errors:?}"
        );
    }
}

/// An override that grants every Alien set its Live resources need passes, though it holds
/// nothing for the Live zone.
#[tokio::test]
async fn an_override_does_not_need_provision_for_a_live_custom_resource() {
    let mut stack = stack(ResourceLifecycle::Live, vec![acme()]);
    stack.permissions.management =
        ManagementPermissions::Override(PermissionProfile::new().global(["worker/provision"]));
    assert_eq!(
        errors(&LiveProvisionPermissionsCheck, &stack, Platform::Aws).await,
        Vec::<String>::new()
    );
}

/// The zone alone, with `descriptor` edited by `edit`.
fn zone_only(edit: impl FnOnce(&mut serde_json::Value)) -> Stack {
    let mut descriptor = dns_zone_descriptor();
    edit(&mut descriptor);
    let zone =
        CustomResource::from_untagged(ZONE, json!({ "id": "zone", "domain": "internal.acme.dev" }))
            .unwrap();
    let mut stack = Stack::new("my-app".to_string())
        .add(zone, ResourceLifecycle::Frozen)
        .build();
    stack.extensions = vec![manifest("acme", descriptor)];
    stack
}

async fn prepare(stack: Stack, platform: Platform) -> Stack {
    PreflightRunner::new()
        .apply_mutations(
            stack,
            &StackState::new(platform),
            &config(SetupManagement::Manual),
        )
        .await
        .unwrap()
}

/// Each service the prepared stack enables, with the ids of the activations that enable it.
fn activations(stack: &Stack) -> BTreeMap<String, Vec<String>> {
    let mut activations = BTreeMap::<String, Vec<String>>::new();
    for (id, entry) in stack.resources() {
        if let Some(activation) = entry.config.downcast_ref::<ServiceActivation>() {
            activations
                .entry(activation.service_name.clone())
                .or_default()
                .push(id.clone());
        }
    }
    activations
}

fn depends_on(stack: &Stack, resource: &str, dependency: &str) -> bool {
    stack.resources[resource]
        .dependencies
        .iter()
        .any(|reference| reference.id() == dependency)
}

/// The docs' stack with the zone asking for Cloud DNS and Cloud Run, which Alien already enables
/// for the worker, and a second extension's type also asking for Cloud DNS.
#[tokio::test]
async fn a_gcp_api_custom_types_ask_for_is_enabled_once_with_alien_s_own() {
    let mut zone = dns_zone_descriptor();
    zone["services"] = json!({ "gcp": ["dns.googleapis.com", "run.googleapis.com"] });
    let mut stack = stack(ResourceLifecycle::Frozen, vec![manifest("acme", zone)]);
    let mut record = dns_zone_descriptor();
    record["permissionSets"] = json!([]);
    record["link"] = json!({ "permissionSets": [] });
    record["services"] = json!({ "gcp": ["dns.googleapis.com"] });
    stack.extensions.push(
        serde_json::from_value(json!({
            "name": "globex",
            "version": "1.0.0",
            "digest": "sha256:9a1c",
            "resourceTypes": { "globex/record": record }
        }))
        .unwrap(),
    );
    stack.resources.insert(
        "record".to_string(),
        alien_core::ResourceEntry {
            config: alien_core::Resource::new(
                CustomResource::from_untagged("globex/record", json!({ "id": "record" })).unwrap(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
            display_name: None,
            description: None,
        },
    );

    let prepared = prepare(stack.clone(), Platform::Gcp).await;
    assert_eq!(
        activations(&prepared),
        BTreeMap::from([
            (
                "cloudquotas.googleapis.com".into(),
                vec!["enable-cloud-quotas".into()]
            ),
            (
                "dns.googleapis.com".into(),
                vec!["extension-service-dns-googleapis-com".into()]
            ),
            ("run.googleapis.com".into(), vec!["enable-cloud-run".into()]),
            (
                "secretmanager.googleapis.com".into(),
                vec!["enable-secret-manager".into()]
            ),
        ])
    );
    assert!(depends_on(
        &prepared,
        "zone",
        "extension-service-dns-googleapis-com"
    ));
    assert!(depends_on(&prepared, "zone", "enable-cloud-run"));
    assert!(depends_on(
        &prepared,
        "record",
        "extension-service-dns-googleapis-com"
    ));
    assert!(!depends_on(&prepared, "record", "enable-cloud-run"));

    // Asked for nowhere, Cloud DNS is not enabled.
    for extension in &mut stack.extensions {
        for descriptor in extension.resource_types.values_mut() {
            descriptor.services.clear();
        }
    }
    let prepared = prepare(stack, Platform::Gcp).await;
    assert!(!activations(&prepared).contains_key("dns.googleapis.com"));
}

/// The zone asks for Microsoft.App, which Alien already registers for the worker, and for
/// Microsoft.Network, which nothing of Alien's in this stack needs but Alien's network uses.
#[tokio::test]
async fn an_azure_provider_a_custom_type_asks_for_is_registered_once_with_alien_s_own() {
    let mut zone = dns_zone_descriptor();
    zone["services"] = json!({
        "azure": ["Microsoft.App", "Microsoft.Network"],
        "gcp": ["dns.googleapis.com"]
    });
    let prepared = prepare(
        stack(ResourceLifecycle::Frozen, vec![manifest("acme", zone)]),
        Platform::Azure,
    )
    .await;

    let activations = activations(&prepared);
    assert_eq!(activations["Microsoft.App"], ["enable-app"]);
    assert_eq!(activations["Microsoft.Network"], ["enable-network"]);
    assert!(!activations.contains_key("dns.googleapis.com"));
    assert!(depends_on(&prepared, "zone", "enable-app"));
    assert!(depends_on(&prepared, "zone", "enable-network"));
}

/// The zone has an Azure emitter, importer and controller, so it lives in the deployment's
/// resource group; a type with nothing for Azure doesn't add one.
#[tokio::test]
async fn an_azure_custom_type_gets_the_default_resource_group() {
    let prepared = prepare(zone_only(|_| {}), Platform::Azure).await;
    assert!(prepared.resources.contains_key("default-resource-group"));
    assert!(depends_on(&prepared, "zone", "default-resource-group"));

    for azure_support in [
        "/setup/cloudformation",
        "/setup/terraform",
        "/setup/helm",
        "/setup/importers",
        "/controllerPlatforms",
    ] {
        let only_this = zone_only(|descriptor| {
            for list in [
                "/setup/cloudformation",
                "/setup/terraform",
                "/setup/helm",
                "/setup/importers",
                "/controllerPlatforms",
            ] {
                *descriptor.pointer_mut(list).unwrap() = if list == azure_support {
                    json!(["azure"])
                } else {
                    json!(["aws"])
                };
            }
        });
        let prepared = prepare(only_this, Platform::Azure).await;
        assert!(
            prepared.resources.contains_key("default-resource-group"),
            "{azure_support}"
        );
    }

    let no_azure = zone_only(|descriptor| {
        descriptor["setup"]["terraform"] = json!(["aws", "gcp"]);
        descriptor["setup"]["importers"] = json!(["aws", "gcp"]);
        descriptor["controllerPlatforms"] = json!(["aws", "gcp"]);
    });
    let prepared = prepare(no_azure, Platform::Azure).await;
    assert!(!prepared.resources.contains_key("default-resource-group"));
}

#[tokio::test]
async fn a_custom_type_that_requires_the_network_gets_one() {
    for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        let prepared = prepare(
            zone_only(|descriptor| descriptor["requiresNetwork"] = json!(true)),
            platform,
        )
        .await;
        assert!(
            prepared.resources.contains_key("default-network"),
            "{platform:?}"
        );
        assert!(
            depends_on(&prepared, "zone", "default-network"),
            "{platform:?}"
        );

        let prepared = prepare(zone_only(|_| {}), platform).await;
        assert!(
            !prepared.resources.contains_key("default-network"),
            "{platform:?}"
        );
    }
}

/// A service Alien's own types can need gets Alien's activation id even when only an extension
/// asks for it, so the id doesn't move when an Alien type later needs the service. Azure names
/// compare without case.
#[tokio::test]
async fn a_service_alien_can_enable_keeps_alien_s_activation_id() {
    let gcp = prepare(
        zone_only(|descriptor| {
            descriptor["services"] =
                json!({ "gcp": ["compute.googleapis.com", "dns.googleapis.com"] })
        }),
        Platform::Gcp,
    )
    .await;
    let activations_on_gcp = activations(&gcp);
    assert_eq!(
        activations_on_gcp["compute.googleapis.com"],
        ["enable-compute-engine"]
    );
    assert_eq!(
        activations_on_gcp["dns.googleapis.com"],
        ["extension-service-dns-googleapis-com"]
    );
    assert!(depends_on(&gcp, "zone", "enable-compute-engine"));

    let azure = prepare(
        zone_only(|descriptor| {
            descriptor["services"] = json!({ "azure": ["microsoft.network", "Microsoft.Network"] })
        }),
        Platform::Azure,
    )
    .await;
    assert_eq!(
        activations(&azure),
        BTreeMap::from([("Microsoft.Network".into(), vec!["enable-network".into()])])
    );
    assert!(depends_on(&azure, "zone", "enable-network"));
}

/// Two names that would share an activation id are refused rather than one replacing the other.
#[tokio::test]
async fn two_services_that_would_share_an_activation_id_are_refused() {
    let error = PreflightRunner::new()
        .apply_mutations(
            zone_only(|descriptor| {
                descriptor["services"] =
                    json!({ "gcp": ["dns.x.googleapis.com", "dns-x.googleapis.com"] })
            }),
            &StackState::new(Platform::Gcp),
            &config(SetupManagement::Manual),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(
            "Resource type 'acme/dns-zone' needs the service 'dns-x.googleapis.com', whose activation id 'extension-service-dns-x-googleapis-com' already enables 'dns.x.googleapis.com'"
        ),
        "{error}"
    );
}

#[tokio::test]
async fn a_service_name_alien_cannot_enable_is_refused() {
    let stack = zone_only(|descriptor| {
        descriptor["services"] = json!({
            "gcp": ["dns.googleapis.com", "cloud.quotas"],
            "azure": ["Microsoft.Network", "App"],
            "aws": ["route53"]
        })
    });
    let mut errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Gcp).await;
    // The platforms' order follows the JSON map's.
    errors.sort();
    assert_eq!(
        errors,
        [
            "Extension 'acme' asks for the service 'App' on azure for resource type 'acme/dns-zone', which is not an Azure resource provider, 'Microsoft.<Namespace>'",
            "Extension 'acme' asks for the service 'cloud.quotas' on gcp for resource type 'acme/dns-zone', which is not a GCP API name, '<name>.googleapis.com'",
            "Extension 'acme' asks for the service 'route53' on aws for resource type 'acme/dns-zone', which is not a GCP API or an Azure resource provider: Alien enables services only on GCP and Azure",
        ]
    );
}

/// On Kubernetes, the cloud the cluster runs in still creates the network, as for Postgres.
#[tokio::test]
async fn a_custom_type_that_requires_the_network_gets_one_on_kubernetes() {
    for base in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        let mut config = config(SetupManagement::Manual);
        config.base_platform = Some(base);
        for (requires_network, expected) in [(true, true), (false, false)] {
            let prepared = PreflightRunner::new()
                .apply_mutations(
                    zone_only(|descriptor| descriptor["requiresNetwork"] = json!(requires_network)),
                    &StackState::new(Platform::Kubernetes),
                    &config,
                )
                .await
                .unwrap();
            assert_eq!(
                prepared.resources.contains_key("default-network"),
                expected,
                "{base:?}"
            );
        }
    }
}

/// A link grants the workload each set on the linked resource, so a built-in set or another
/// type's would be granted on a resource that never declared it.
#[tokio::test]
async fn a_link_to_a_set_the_type_does_not_define_is_refused() {
    let stack = zone_only(|descriptor| {
        descriptor["link"] = json!({
            "permissionSets": [format!("{ZONE}/records-write"), "storage/data-read", "acme/record/write"]
        })
    });
    let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
    assert_eq!(
        errors,
        [
            "Extension 'acme' links resource type 'acme/dns-zone' with permission set 'storage/data-read', which is not one of the type's own permission sets",
            "Extension 'acme' links resource type 'acme/dns-zone' with permission set 'acme/record/write', which is not one of the type's own permission sets",
        ]
    );
}

/// `alien release --prebuilt` skips the compile-time checks its source stack already passed,
/// and still checks the extensions a built stack lists.
#[tokio::test]
async fn a_prebuilt_release_checks_the_types_its_extensions_declare() {
    let runner = PreflightRunner::new();
    runner
        .run_prebuilt_release_checks(&zone_only(|_| {}), Platform::Gcp)
        .await
        .expect("the docs' zone passes");

    let stack =
        zone_only(|descriptor| descriptor["services"] = json!({ "gcp": ["Not A Service"] }));
    let error = runner
        .run_prebuilt_release_checks(&stack, Platform::Gcp)
        .await
        .expect_err("an invalid service name is refused");
    let Some(alien_preflights::error::ErrorData::ValidationFailed { results, .. }) = &error.error
    else {
        panic!("{error}");
    };
    let failed: Vec<&String> = results.iter().flat_map(|result| &result.errors).collect();
    assert!(
        failed
            .iter()
            .any(|message| message.contains("asks for the service 'Not A Service' on gcp")),
        "{failed:?}"
    );
}

/// The extensions check runs on a prebuilt stack only when the stack has extensions or custom
/// resources: a stack with neither gets the checks it always had.
#[tokio::test]
async fn a_prebuilt_stack_without_extensions_keeps_its_checks() {
    let description = AllowedUserResourcesCheck.description();
    let ran = |summary: &alien_preflights::PreflightSummary| {
        summary
            .results
            .iter()
            .any(|result| result.check_description.as_deref() == Some(description))
    };
    let runner = PreflightRunner::new();
    let plain = Stack::new("my-app".to_string()).build();
    let summary = runner
        .run_prebuilt_release_checks(&plain, Platform::Aws)
        .await
        .unwrap();
    assert!(!ran(&summary), "{:?}", summary.results);
    let summary = runner
        .run_prebuilt_release_checks(&zone_only(|_| {}), Platform::Aws)
        .await
        .unwrap();
    assert!(ran(&summary), "{:?}", summary.results);
}

/// The zone with `provision`, `management` and `heartbeat` sets besides its link's
/// `records-write`.
fn zone_with_identity_sets() -> Stack {
    zone_only(|descriptor| {
        let set = |name: &str, action: &str| {
            json!({
                "id": format!("{ZONE}/{name}"),
                "description": name,
                "platforms": { "aws": [{
                    "grant": { "actions": [action] },
                    "binding": {
                        "stack": { "resources": ["arn:aws:route53:::hostedzone/*"] },
                        "resource": { "resources": ["arn:aws:route53:::hostedzone/*"] }
                    }
                }] }
            })
        };
        descriptor["permissionSets"] = json!([
            set("provision", "route53:CreateHostedZone"),
            set("management", "route53:ChangeTagsForResource"),
            set("heartbeat", "route53:GetHostedZone"),
            set("records-write", "route53:ChangeResourceRecordSets"),
        ]);
    })
}

/// `config` with the manager's settings for `platform`, which give the stack its management
/// identity.
fn managed(mut config: DeploymentConfig, platform: Platform) -> DeploymentConfig {
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

async fn prepare_managed(stack: Stack, platform: Platform) -> Stack {
    PreflightRunner::new()
        .apply_mutations(
            stack,
            &StackState::new(platform),
            &managed(config(SetupManagement::Manual), platform),
        )
        .await
        .unwrap()
}

fn identity(stack: &Stack) -> Option<&ServiceAccount> {
    stack
        .resources
        .get("acme-ext")
        .and_then(|entry| entry.config.downcast_ref::<ServiceAccount>())
}

fn identity_set_ids(stack: &Stack) -> Vec<&str> {
    identity(stack)
        .expect("the extension's identity")
        .stack_permission_sets
        .iter()
        .map(|set| set.id.as_str())
        .collect()
}

/// Each cloud gets one Frozen identity for the extension, holding its type's `provision`,
/// `management` and `heartbeat` sets and nothing a link grants.
#[tokio::test]
async fn setup_adds_the_extensions_identity_with_its_sets() {
    for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        let prepared = prepare_managed(zone_with_identity_sets(), platform).await;
        let entry = &prepared.resources["acme-ext"];
        assert_eq!(entry.lifecycle, ResourceLifecycle::Frozen, "{platform:?}");
        assert_eq!(
            identity(&prepared).unwrap().extension.as_deref(),
            Some("acme"),
            "{platform:?}"
        );
        assert_eq!(
            identity_set_ids(&prepared),
            [
                "acme/dns-zone/provision",
                "acme/dns-zone/management",
                "acme/dns-zone/heartbeat"
            ],
            "{platform:?}"
        );
    }
}

/// Heartbeat sets follow the stack's heartbeat setting, as Alien's own do.
#[tokio::test]
async fn heartbeats_off_leave_the_heartbeat_set_out() {
    let mut config = managed(config(SetupManagement::Manual), Platform::Aws);
    config.stack_settings.heartbeats = alien_core::HeartbeatsMode::Off;
    let prepared = PreflightRunner::new()
        .apply_mutations(
            zone_with_identity_sets(),
            &StackState::new(Platform::Aws),
            &config,
        )
        .await
        .unwrap();
    assert_eq!(
        identity_set_ids(&prepared),
        ["acme/dns-zone/provision", "acme/dns-zone/management"]
    );
}

/// An extension the stack lists but uses no type of gets no identity, and neither does a stack
/// with no management identity, whose trust the identity's follows, or a deployment off the
/// clouds.
#[tokio::test]
async fn no_identity_without_a_resource_of_the_extensions_types_or_off_the_clouds() {
    let mut unused = zone_with_identity_sets();
    unused.resources.shift_remove("zone");
    assert!(identity(&prepare_managed(unused, Platform::Aws).await).is_none());
    assert!(identity(&prepare(zone_with_identity_sets(), Platform::Aws).await).is_none());
    let prepared = PreflightRunner::new()
        .apply_mutations(
            zone_with_identity_sets(),
            &StackState::new(Platform::Local),
            &managed(config(SetupManagement::Manual), Platform::Aws),
        )
        .await
        .unwrap();
    assert!(identity(&prepared).is_none());
}

/// A pull deployment on EKS, GKE or AKS gets the identity too, beside the management identity
/// the Operator runs as: the Operator passes the extension's credentials to its controller.
#[tokio::test]
async fn kubernetes_on_a_cloud_gets_the_identity() {
    for base in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        let mut config = managed(config(SetupManagement::Manual), base);
        config.base_platform = Some(base);
        config.stack_settings.deployment_model = alien_core::DeploymentModel::Pull;
        let prepared = PreflightRunner::new()
            .apply_mutations(
                zone_with_identity_sets(),
                &StackState::new(Platform::Kubernetes),
                &config,
            )
            .await
            .unwrap_or_else(|error| panic!("{base:?}: {error}"));
        assert_eq!(
            identity(&prepared).and_then(|account| account.extension.as_deref()),
            Some("acme"),
            "{base:?}"
        );
    }
}

/// A resource the developer named like the identity is never replaced by it.
#[tokio::test]
async fn the_identitys_id_is_reserved() {
    let mut stack = zone_with_identity_sets();
    stack.resources.insert(
        "acme-ext".to_string(),
        alien_core::ResourceEntry::new(
            alien_core::Storage::new("acme-ext".to_string()).build(),
            ResourceLifecycle::Frozen,
        ),
    );
    let error = PreflightRunner::new()
        .apply_mutations(
            stack,
            &StackState::new(Platform::Aws),
            &managed(config(SetupManagement::Manual), Platform::Aws),
        )
        .await
        .unwrap_err();
    assert!(
        format!("{error:?}").contains(
            "resource ID 'acme-ext' is reserved for the identity of the extension 'acme', but is already used by resource type 'storage'"
        ),
        "{error:?}"
    );
}

/// Preparing a prepared stack again keeps one identity, unchanged.
#[tokio::test]
async fn preparing_twice_keeps_one_identity() {
    let once = prepare_managed(zone_with_identity_sets(), Platform::Aws).await;
    let twice = prepare_managed(once.clone(), Platform::Aws).await;
    assert_eq!(identity(&once), identity(&twice));
}

/// The setup a customer approved names the extension's grants, so changing one asks for a setup
/// update.
#[tokio::test]
async fn the_setup_digest_follows_the_extensions_permission_sets() {
    let before = prepare_managed(zone_with_identity_sets(), Platform::Aws).await;
    let mut changed = zone_with_identity_sets();
    changed.extensions[0]
        .resource_types
        .get_mut(&ResourceType::from(ZONE))
        .unwrap()
        .permission_sets[0]
        .platforms
        .aws
        .as_mut()
        .unwrap()[0]
        .grant
        .actions
        .as_mut()
        .unwrap()
        .push("route53:DeleteHostedZone".to_string());
    let after = prepare_managed(changed, Platform::Aws).await;
    assert_ne!(before.setup_owned_digest(), after.setup_owned_digest());
    assert_ne!(before.setup_update_digest(), after.setup_update_digest());
}

/// On GCP the identity trusts the manager through the management identity's federation, so
/// direct setup creates it after the management identity.
#[tokio::test]
async fn on_gcp_the_identity_waits_for_the_management_identity() {
    let prepared = prepare_managed(zone_with_identity_sets(), Platform::Gcp).await;
    let management = prepared
        .resources
        .iter()
        .find(|(_, entry)| {
            entry.config.resource_type() == alien_core::RemoteStackManagement::RESOURCE_TYPE
        })
        .map(|(id, _)| id.clone())
        .expect("a management identity");
    assert!(depends_on(&prepared, "acme-ext", &management));
}

fn inline_set(id: &str) -> PermissionSetReference {
    PermissionSetReference::Inline(
        serde_json::from_value(json!({
            "id": id,
            "description": "inline",
            "platforms": { "aws": [{
                "grant": { "actions": ["route53:GetHostedZone"] },
                "binding": { "stack": { "resources": ["*"] } }
            }] }
        }))
        .unwrap(),
    )
}

/// An inline management set can't take an id under an extension's name: the setup template
/// would show an extension's id on the management identity.
#[tokio::test]
async fn an_inline_management_set_under_an_extensions_name_is_refused() {
    for management in [
        ManagementPermissions::Extend(PermissionProfile(
            [("*".to_string(), vec![inline_set("acme/zone-reader")])].into(),
        )),
        ManagementPermissions::Override(PermissionProfile(
            [("zone".to_string(), vec![inline_set("acme/zone-reader")])].into(),
        )),
    ] {
        let mut stack = stack(ResourceLifecycle::Frozen, vec![acme()]);
        stack.permissions.management = management;
        let errors = errors(&PermissionSetsExistCheck, &stack, Platform::Aws).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("grants the inline permission set 'acme/zone-reader'")
                && errors[0].contains("whose id is under the extension 'acme'"),
            "{errors:?}"
        );
    }
}

/// The refusal reads only the extensions the stack lists, and only the management profile.
#[tokio::test]
async fn inline_sets_outside_a_listed_extensions_name_pass() {
    let cases = [
        (vec![acme()], "zone-reader/read", true),
        (Vec::new(), "acme/zone-reader", true),
        (vec![acme()], "acme/zone-reader", false),
    ];
    for (extensions, id, management) in cases {
        let mut stack = stack(ResourceLifecycle::Frozen, Vec::new());
        stack.resources.shift_remove("zone");
        stack.extensions = extensions;
        let profile = PermissionProfile([("*".to_string(), vec![inline_set(id)])].into());
        if management {
            stack.permissions.management = ManagementPermissions::Extend(profile);
        } else {
            stack
                .permissions
                .profiles
                .insert("api".to_string(), profile);
        }
        assert_eq!(
            errors(&PermissionSetsExistCheck, &stack, Platform::Aws).await,
            Vec::<String>::new(),
            "{id} management={management}"
        );
    }
}

/// The zone alone on a Kubernetes stack, with `descriptor`.
fn zone_only_on_kubernetes(lifecycle: ResourceLifecycle, descriptor: serde_json::Value) -> Stack {
    let zone =
        CustomResource::from_untagged(ZONE, json!({ "id": "zone", "domain": "internal.acme.dev" }))
            .unwrap();
    let mut stack = Stack::new("my-app".to_string())
        .add(zone, lifecycle)
        .build();
    stack.extensions = vec![manifest("acme", descriptor)];
    stack
}

/// The deployment-time preflights of a Kubernetes deployment whose config names `base_platform`.
#[cfg(feature = "runtime-checks")]
async fn deploy_on_kubernetes(
    stack: Stack,
    base_platform: Option<Platform>,
) -> alien_preflights::error::Result<()> {
    let mut config = config(SetupManagement::Manual);
    config.base_platform = base_platform;
    PreflightRunner::new()
        .run_deployment_time_preflights(
            stack,
            &StackState::new(Platform::Kubernetes),
            &config,
            &alien_core::ClientConfig::Test,
            None,
            None,
            None,
        )
        .await
        .map(|_| ())
}

/// A deployment's preflights know its cluster: an eks deployment needs only the AWS side, and
/// one in no cloud needs a Helm emitter or a Kubernetes controller.
#[cfg(feature = "runtime-checks")]
#[tokio::test]
async fn a_deployment_is_checked_for_its_own_cluster() {
    let mut aws_only = dns_zone_descriptor();
    aws_only["controllerPlatforms"] = json!(["aws"]);
    aws_only["setup"]["helm"] = json!([]);
    aws_only["setup"]["importers"] = json!(["aws"]);
    for lifecycle in [ResourceLifecycle::Live, ResourceLifecycle::Frozen] {
        deploy_on_kubernetes(
            zone_only_on_kubernetes(lifecycle, aws_only.clone()),
            Some(Platform::Aws),
        )
        .await
        .unwrap_or_else(|e| panic!("eks, {lifecycle:?}: {e:?}"));
    }

    let refused = deploy_on_kubernetes(
        zone_only_on_kubernetes(ResourceLifecycle::Frozen, aws_only.clone()),
        None,
    )
    .await
    .unwrap_err();
    assert!(
        format!("{refused:?}").contains(
            "can only be Frozen on platform 'kubernetes', but its extension has no Helm emitter"
        ),
        "{refused:?}"
    );
    let refused = deploy_on_kubernetes(
        zone_only_on_kubernetes(ResourceLifecycle::Live, aws_only),
        None,
    )
    .await
    .unwrap_err();
    assert!(
        format!("{refused:?}").contains(
            "has lifecycle Live, but its extension has no controller for platform 'kubernetes'"
        ),
        "{refused:?}"
    );

    let mut gcp_only = dns_zone_descriptor();
    gcp_only["lifecycles"] = json!(["frozen"]);
    gcp_only["controllerPlatforms"] = json!([]);
    gcp_only["setup"]["importers"] = json!(["gcp"]);
    let refused = deploy_on_kubernetes(
        zone_only_on_kubernetes(ResourceLifecycle::Frozen, gcp_only),
        Some(Platform::Aws),
    )
    .await
    .unwrap_err();
    assert!(
        format!("{refused:?}").contains(
            "can only be Frozen on platform 'aws', the cloud its Kubernetes cluster runs in, but its extension has no importer for it"
        ),
        "{refused:?}"
    );
}

/// The docs' zone is Live with controllers for the clouds and Local only. Its chart, which every
/// Kubernetes target installs, releases, and so does a build for eks.
#[tokio::test]
async fn the_docs_live_zone_releases_a_chart() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["controllerPlatforms"] = json!(["aws", "gcp", "azure", "local"]);
    let stack = zone_only_on_kubernetes(ResourceLifecycle::Live, descriptor);
    let runner = PreflightRunner::new();

    runner
        .run_template_preflights(&stack, Platform::Kubernetes)
        .await
        .unwrap();
    runner
        .run_build_time_preflights(&stack, Platform::Kubernetes)
        .await
        .unwrap();
    runner
        .run_build_time_preflights_on(
            &stack,
            Platform::Kubernetes,
            ClusterCloud::Cloud(Platform::Aws),
        )
        .await
        .unwrap();
}

/// A zone made for eks alone, with the AWS importer and no Helm emitter, releases a chart too;
/// a deployment in no cloud is refused at its own preflights.
#[tokio::test]
async fn a_zone_for_eks_alone_releases_a_chart() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["lifecycles"] = json!(["frozen"]);
    descriptor["controllerPlatforms"] = json!([]);
    descriptor["setup"]["helm"] = json!([]);
    descriptor["setup"]["importers"] = json!(["aws"]);
    let stack = zone_only_on_kubernetes(ResourceLifecycle::Frozen, descriptor);
    let runner = PreflightRunner::new();

    runner
        .run_template_preflights(&stack, Platform::Kubernetes)
        .await
        .unwrap();
    runner
        .run_template_preflights_on(
            &stack,
            Platform::Kubernetes,
            ClusterCloud::Cloud(Platform::Aws),
        )
        .await
        .unwrap();
    assert!(runner
        .run_template_preflights_on(&stack, Platform::Kubernetes, ClusterCloud::NoCloud)
        .await
        .is_err());
}

/// A type that runs on no cluster the chart serves is refused at release, each cluster's reason
/// named.
#[tokio::test]
async fn a_type_that_runs_on_no_cluster_the_chart_serves_is_refused() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["lifecycles"] = json!(["frozen"]);
    descriptor["controllerPlatforms"] = json!([]);
    descriptor["setup"]["helm"] = json!([]);
    descriptor["setup"]["importers"] = json!([]);
    let stack = zone_only_on_kubernetes(ResourceLifecycle::Frozen, descriptor);

    let errors = errors(&FrozenResourceLifecycleCheck, &stack, Platform::Kubernetes).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    for part in [
        "Resource 'zone' of type 'acme/dns-zone' can run on none of the Kubernetes clusters its chart serves",
        "on a cluster in no cloud it can only be Frozen on platform 'kubernetes', but its extension has no Helm emitter",
        "on eks it can only be Frozen on platform 'aws', the cloud its Kubernetes cluster runs in, but its extension has no importer",
        "on gke it",
        "on aks it",
    ] {
        assert!(errors[0].contains(part), "{part}: {errors:?}");
    }
}

/// The eks package and an eks build need the AWS side: a type that has only the GCP side is
/// refused there, Live or Frozen.
#[tokio::test]
async fn the_eks_package_and_build_need_the_aws_side() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["controllerPlatforms"] = json!(["gcp"]);
    descriptor["setup"]["importers"] = json!(["gcp"]);
    let runner = PreflightRunner::new();
    for (lifecycle, refusal) in [
        (
            ResourceLifecycle::Live,
            "has lifecycle Live, but its extension has no controller for platform 'aws', the cloud its Kubernetes cluster runs in",
        ),
        (
            ResourceLifecycle::Frozen,
            "can only be Frozen on platform 'aws', the cloud its Kubernetes cluster runs in, but its extension has no importer for it",
        ),
    ] {
        let stack = zone_only_on_kubernetes(lifecycle, descriptor.clone());
        let eks = ClusterCloud::Cloud(Platform::Aws);
        let package = runner
            .run_template_preflights_on(&stack, Platform::Kubernetes, eks)
            .await
            .unwrap_err();
        assert!(format!("{package:?}").contains(refusal), "{package:?}");
        let build = runner
            .run_build_time_preflights_on(&stack, Platform::Kubernetes, eks)
            .await
            .unwrap_err();
        assert!(format!("{build:?}").contains(refusal), "{build:?}");
        runner
            .run_build_time_preflights(&stack, Platform::Kubernetes)
            .await
            .unwrap_or_else(|e| panic!("gke serves it: {e:?}"));
    }
}

/// An external binding carries `type` and `service` beside the outputs, so a type can't name an
/// output either way.
#[tokio::test]
async fn an_output_named_like_a_binding_field_is_refused() {
    for name in ["type", "service"] {
        let mut descriptor = dns_zone_descriptor();
        descriptor["schemas"]["outputs"] =
            json!({ "type": "object", "properties": { name: { "type": "string" } } });
        let stack = stack(
            ResourceLifecycle::Frozen,
            vec![manifest("acme", descriptor)],
        );
        let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains(&format!(
                "Extension 'acme' gives resource type 'acme/dns-zone' an output named '{name}', which an external binding uses for itself"
            )),
            "{errors:?}"
        );
    }

    {
        // `#[serde(flatten)]` puts a flattened struct's fields in an `allOf` branch.
        let mut descriptor = dns_zone_descriptor();
        descriptor["schemas"]["outputs"] = json!({
            "allOf": [
                { "$ref": "#/$defs/Common" },
                { "type": "object", "properties": { "zoneId": { "type": "string" } } }
            ],
            "$defs": { "Common": { "type": "object", "properties": { "service": { "type": "string" } } } }
        });
        let stack = stack(
            ResourceLifecycle::Frozen,
            vec![manifest("acme", descriptor)],
        );
        let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("an output named 'service'"),
            "{errors:?}"
        );
    }
    // `#[serde(flatten)] Option<S>` puts a `oneOf` of null and `S` in an `allOf` branch, here
    // with `S` itself flattened into the outputs through a second `allOf`.
    for outputs in [
        json!({
            "allOf": [
                { "oneOf": [{ "type": "null" }, { "$ref": "#/$defs/Svc" }] },
                { "type": "object", "properties": { "zoneId": { "type": "string" } } }
            ],
            "$defs": { "Svc": { "type": "object", "properties": { "service": { "type": "string" } } } }
        }),
        json!({
            "allOf": [
                { "$ref": "#/$defs/Common" },
                { "type": "object", "properties": { "zoneId": { "type": "string" } } }
            ],
            "$defs": {
                "Common": { "allOf": [
                    { "oneOf": [{ "type": "null" }, { "$ref": "#/$defs/Svc" }] },
                    { "type": "object", "properties": { "region": { "type": "string" } } }
                ] },
                "Svc": { "type": "object", "properties": { "service": { "type": "string" } } }
            }
        }),
        // A plain field whose type flattens an Option, as the SDK writes it.
        json!({
            "type": "object",
            "required": ["zoneId", "mid"],
            "properties": { "mid": { "$ref": "#/$defs/MidOpt" }, "zoneId": { "type": "string" } },
            "$defs": {
                "MidOpt": { "allOf": [
                    { "oneOf": [{ "type": "null" }, { "$ref": "#/$defs/Loc" }] },
                    { "type": "object", "required": ["region"], "properties": { "region": { "type": "string" } } }
                ] },
                "Loc": { "type": "object", "properties": { "accountId": { "type": "string" } } }
            }
        }),
    ] {
        let mut descriptor = dns_zone_descriptor();
        descriptor["schemas"]["outputs"] = outputs;
        let stack = stack(
            ResourceLifecycle::Frozen,
            vec![manifest("acme", descriptor)],
        );
        let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains(
                "Extension 'acme' declares resource type 'acme/dns-zone' whose outputs flatten an Option or an enum, whose fields depend on the value, which Alien can't check a binding against"
            ),
            "{errors:?}"
        );
    }
}

/// Outputs that flatten a struct and a map side by side are refused once, with a message that
/// says to name the map.
#[tokio::test]
async fn outputs_that_flatten_a_map_beside_a_struct_are_refused() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["schemas"]["outputs"] = json!({
        "allOf": [
            { "$ref": "#/$defs/Loc" },
            {
                "type": "object",
                "required": ["zoneId"],
                "properties": { "zoneId": { "type": "string" } },
                "additionalProperties": { "type": "string" }
            }
        ],
        "$defs": { "Loc": { "type": "object", "properties": { "region": { "type": "string" } } } }
    });
    let stack = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor)],
    );
    let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
    assert_eq!(
        errors,
        vec![format!(
            "Extension 'acme' declares resource type 'acme/dns-zone' whose outputs {}",
            alien_core::binding_schema::FLATTENED_MAP
        )]
    );
}

#[tokio::test]
async fn outputs_that_flatten_a_map_beside_a_nested_struct_are_refused() {
    let mut descriptor = dns_zone_descriptor();
    descriptor["schemas"]["outputs"] = json!({
        "allOf": [
            { "$ref": "#/$defs/Mid" },
            {
                "type": "object",
                "required": ["zoneId"],
                "properties": { "zoneId": { "type": "string" } },
                "additionalProperties": { "type": "string" }
            }
        ],
        "$defs": {
            "Mid": {
                "allOf": [
                    { "$ref": "#/$defs/Loc" },
                    { "type": "object", "properties": { "name": { "type": "string" } } }
                ]
            },
            "Loc": { "type": "object", "properties": { "region": { "type": "string" } } }
        }
    });
    let stack = stack(
        ResourceLifecycle::Frozen,
        vec![manifest("acme", descriptor)],
    );
    let errors = errors(&AllowedUserResourcesCheck, &stack, Platform::Aws).await;
    assert_eq!(
        errors,
        vec![format!(
            "Extension 'acme' declares resource type 'acme/dns-zone' whose outputs {}",
            alien_core::binding_schema::FLATTENED_MAP
        )]
    );
}
