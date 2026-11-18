use crate::{
    is_prepared_cluster, ownership_policy_for_resource_type, Build, ComputeCluster, Container,
    CustomResource, Daemon, ErrorData, Key, KubernetesCluster, KubernetesClusterOwnership,
    ManagementPermissions, NetworkSettings, Platform, ResourceEntry, ResourceLifecycle,
    ResourceRef, ResourceType, Result, Sandbox, SetupManagement, Stack, StackSettings, Storage,
    Worker,
};
use alien_error::AlienError;

/// Whether a permission profile scopes sets to `id`, or a workload with a profile links it and
/// its type links sets, as `ResourceLinkPermissionsMutation` grants them.
fn is_granted_on(stack: &Stack, id: &str, resource_type: &ResourceType) -> bool {
    // A type no extension defines links nothing, as the mutation skips its links.
    let links_sets = stack
        .descriptor(resource_type)
        .is_some_and(|descriptor| !descriptor.link.permission_sets.is_empty());
    let linked = |links: &[ResourceRef]| links_sets && links.iter().any(|link| link.id == id);
    stack
        .permissions
        .profiles
        .values()
        .any(|profile| profile.0.get(id).is_some_and(|sets| !sets.is_empty()))
        || stack.resources().any(|(_, entry)| {
            let config = &entry.config;
            config
                .downcast_ref::<Worker>()
                .is_some_and(|w| linked(&w.links))
                || config
                    .downcast_ref::<Container>()
                    .is_some_and(|c| linked(&c.links))
                || config
                    .downcast_ref::<Daemon>()
                    .is_some_and(|d| d.permissions.is_some() && linked(&d.links))
                || config
                    .downcast_ref::<Build>()
                    .is_some_and(|b| linked(&b.links))
        })
}

/// Refuse resources whose setup-owned scaffolding cannot yet be created by runtime.
/// Check both package generation and deployment planning, including later additions.
pub fn validate_automatic_setup_resources(
    stack: &Stack,
    platform: Platform,
    management: SetupManagement,
) -> Result<()> {
    if management == SetupManagement::Automatic
        && matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
    {
        let unsupported = |resource_type: &str, resource_id: &str| {
            AlienError::new(ErrorData::AutomaticSetupResourceUnsupported {
                resource_type: resource_type.to_string(),
                resource_id: resource_id.to_string(),
                platform,
            })
        };
        match stack.management() {
            // Setup must be able to grant whatever a later release adds.
            ManagementPermissions::Override(_) => {
                return Err(unsupported(
                    "management permission override",
                    "permissions.management",
                ));
            }
            // The AWS management role may not change its own policies after setup, so a
            // grant scoped to one resource could never be attached by the runtime.
            ManagementPermissions::Extend(profile) if platform == Platform::Aws => {
                if let Some(scope) = profile.0.keys().find(|scope| scope.as_str() != "*") {
                    return Err(unsupported("resource-scoped management permission", scope));
                }
            }
            _ => {}
        }
        for (id, entry) in stack.resources() {
            if let Some(resource_type) = automatic_setup_unsupported_resource(entry, platform) {
                return Err(unsupported(&resource_type, id));
            }
            // On AWS the runtime creates a workload's role after setup, and nothing then grants
            // a custom resource's sets, so a grant on one would never be written.
            if platform == Platform::Aws
                && entry.config.downcast_ref::<CustomResource>().is_some()
                && is_granted_on(stack, id, &entry.config.resource_type())
            {
                return Err(unsupported("custom resource grant", id));
            }
        }
    }
    if management == SetupManagement::Automatic && platform == Platform::Azure {
        for (id, entry) in stack.resources() {
            if entry.config.downcast_ref::<Sandbox>().is_some() {
                return Err(AlienError::new(ErrorData::SandboxCapabilityUnsupported {
                    capability: format!(
                        "automatic infrastructure management for sandbox '{id}'; use manual setup to create and import its sandbox group"
                    ),
                    platform: platform.to_string(),
                }));
            }
        }
    }
    Ok(())
}

/// Return resources whose declarations cannot be created by their runtime
/// controllers when omitted from automatic setup.
fn automatic_setup_unsupported_resource(
    entry: &ResourceEntry,
    platform: Platform,
) -> Option<String> {
    if entry.config.downcast_ref::<Key>().is_some() {
        return Some("key".to_string());
    }
    let resource_type = entry.config.resource_type();
    // These controllers refuse runtime creation: only a setup package can provision them.
    if matches!(
        resource_type.as_ref(),
        "email" | "experimental/aws-opensearch" | "kubernetes-cluster"
    ) {
        return Some(resource_type.as_ref().to_string());
    }
    // Azure CORS belongs to the shared account blob service; the container
    // controller cannot apply it independently.
    if platform == Platform::Azure
        && entry
            .config
            .downcast_ref::<Storage>()
            .is_some_and(|storage| storage.has_cors())
    {
        return Some("storage with CORS origins".to_string());
    }
    None
}

/// Controls configuration exposure, independently of installation authority.
/// Existing generator entry points retain the complete Advanced interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupConfigurationProfile {
    Standard,
    Advanced,
}

/// Where a setup package installs: the platform the deployment runs on and the cloud its
/// infrastructure lives in. They differ for managed Kubernetes (an EKS, GKE or AKS cluster runs
/// on `Kubernetes`, in the `Aws`, `Gcp` or `Azure` cloud).
///
/// Every question about what a setup package emits and registers is answered from this, so the
/// generators, the emitters and the manager's registration check cannot disagree about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupTarget {
    /// The platform the deployment runs on.
    pub deployment_platform: Platform,
    /// The cloud the setup's infrastructure lives in.
    pub cloud_platform: Platform,
}

impl SetupTarget {
    /// A target from a registration's `platform` and `basePlatform`.
    pub fn new(deployment_platform: Platform, base_platform: Option<Platform>) -> Self {
        Self {
            deployment_platform,
            cloud_platform: base_platform.unwrap_or(deployment_platform),
        }
    }

    /// A target from the cloud and whether the setup installs onto a Kubernetes cluster in it.
    pub fn from_cloud(cloud_platform: Platform, targets_kubernetes: bool) -> Self {
        Self {
            deployment_platform: if targets_kubernetes {
                Platform::Kubernetes
            } else {
                cloud_platform
            },
            cloud_platform,
        }
    }

    pub fn is_kubernetes(&self) -> bool {
        self.deployment_platform == Platform::Kubernetes
    }
}

/// Whether the resource type is one a Kubernetes target runs on the cluster itself: a
/// `ComputeCluster` is a set of logical pools the Operator validates and a `Sandbox` is a pod.
/// Setup creates no cloud infrastructure for either, and runtime provisioning verifies them
/// through the Kubernetes controllers rather than a cloud controller.
///
/// Setup's emit rule ([`emits_package_scaffolding`]) and the executor's choice of controller
/// both ask this, so they cannot disagree.
pub fn is_kubernetes_native_resource_type(resource_type: &crate::ResourceType) -> bool {
    *resource_type == Sandbox::RESOURCE_TYPE || *resource_type == ComputeCluster::RESOURCE_TYPE
}

/// Whether this resource is a cloud database (Postgres or Redis) whose private endpoint needs a
/// network of the cloud's own. On a Kubernetes target a stack keeps a network for it in every
/// cluster mode; without one an existing cluster's own network serves the workloads.
///
/// The network preflight and the setup generators both ask this, so "this stack needs a network"
/// has one answer.
pub fn is_network_database(entry: &ResourceEntry) -> bool {
    let resource_type = entry.config.resource_type();
    resource_type == crate::Postgres::RESOURCE_TYPE || resource_type == crate::Redis::RESOURCE_TYPE
}

/// Whether this resource keeps a network of the cloud's own on a Kubernetes target, in every
/// cluster mode: a cloud database, or an extension type whose descriptor requires the network.
///
/// The network preflight and the Terraform network gate both ask this, so they cannot disagree.
pub fn keeps_cloud_network(stack: &Stack, entry: &ResourceEntry) -> bool {
    is_network_database(entry)
        || stack
            .descriptor(&entry.config.resource_type())
            .is_some_and(|descriptor| descriptor.requires_network)
}

/// Whether setup can put this resource's private endpoint on the network of an existing
/// Kubernetes cluster, so that the cluster's workloads reach it.
///
/// Only a GCP Postgres can: the module looks the cluster's own network up. A Memorystore
/// instance cannot move to another network after creation, and on AWS and Azure the module keeps
/// a network of its own that the cluster is not attached to. Any other resource is never
/// refused, so the answer for it is `true`. That includes an extension type that requires the
/// network: it uses whichever network the module keeps, whether or not the cluster reaches it.
///
/// The network preflight (at build time) and the generated modules (at plan time, where the
/// cluster mode is chosen) both ask this, so they refuse the same cases.
pub fn existing_cluster_network_hosts(entry: &ResourceEntry, cloud_platform: Platform) -> bool {
    if !is_network_database(entry) {
        return true;
    }
    entry.config.resource_type() == crate::Postgres::RESOURCE_TYPE
        && cloud_platform == Platform::Gcp
}

/// The message for a database or cache that setup cannot place on an existing cluster's network.
/// `network_hint`, when the deployer can bring the cluster's own network, names how in the
/// caller's terms.
pub fn existing_cluster_network_refusal(
    resource_type: &str,
    resource_id: &str,
    network_hint: Option<&str>,
) -> String {
    let hint = network_hint
        .map(|hint| format!(" {hint},"))
        .unwrap_or_default();
    format!(
        "Setup cannot yet place the {resource_type} '{resource_id}' on an existing cluster's network, so the cluster could not reach it. Use a cluster that setup creates,{hint} or remove the {resource_type}."
    )
}

/// Whether the setup is for an existing Kubernetes cluster: the cluster the source stack declares
/// decides; with none declared, the stack settings do. A setup that serves both modes (the
/// deployer picks at apply) is not "existing" here.
pub fn setup_targets_existing_cluster(stack: &Stack, settings: &StackSettings) -> bool {
    let ownership = stack
        .resources()
        .filter(|(id, entry)| !is_prepared_cluster(id, entry))
        .find_map(|(_, entry)| entry.config.downcast_ref::<KubernetesCluster>())
        .map(|cluster| cluster.ownership)
        .or_else(|| {
            settings
                .kubernetes
                .as_ref()
                .and_then(|kubernetes| kubernetes.cluster.as_ref())
                .map(|cluster| cluster.ownership)
        });
    ownership.is_some_and(|ownership| ownership != KubernetesClusterOwnership::Managed)
}

/// The first resource setup would create that an existing cluster could not reach, as the error
/// a generator returns when it knows the cluster is existing. `creates` says whether setup
/// creates the resource: one the installer binds to infrastructure it already has is the
/// deployer's, so it is not refused. A network the deployer names in the settings is theirs to
/// choose, so nothing is refused for it.
pub fn refuse_unreachable_databases(
    stack: &Stack,
    settings: &StackSettings,
    cloud_platform: Platform,
    network_hint: Option<&str>,
    creates: impl Fn(&ResourceEntry) -> bool,
) -> Result<()> {
    if !setup_targets_existing_cluster(stack, settings)
        || matches!(
            settings.network,
            Some(
                NetworkSettings::ByoVpcAws { .. }
                    | NetworkSettings::ByoVpcGcp { .. }
                    | NetworkSettings::ByoVnetAzure { .. }
            )
        )
    {
        return Ok(());
    }
    for (id, entry) in stack.resources() {
        // A resource the deployer can decline is checked by the module's plan instead, once its
        // input is known.
        if entry.enabled_when.is_none()
            && !existing_cluster_network_hosts(entry, cloud_platform)
            && creates(entry)
        {
            return Err(AlienError::new(ErrorData::SetupPlacementUnsupported {
                message: existing_cluster_network_refusal(
                    entry.config.resource_type().as_ref(),
                    id,
                    network_hint,
                ),
                resource_type: entry.config.resource_type().to_string(),
                resource_id: id.to_string(),
            }));
        }
    }
    Ok(())
}

/// Refuses an extension type that requires the network on an existing cluster when its input
/// also gates a frozen resource, for a setup that fixes the network when it is rendered
/// (CloudFormation). Declining that input drops the resource before the manager prepares the
/// stack, so the manager could expect no network while the setup still registers one.
///
/// Nothing is refused when the deployer brings the network in the settings, or when another
/// resource keeps the network without such an input, so it stays whatever the deployer answers.
/// A database is left to [`refuse_unreachable_databases`].
pub fn refuse_declinable_network_extensions(stack: &Stack, settings: &StackSettings) -> Result<()> {
    if !setup_targets_existing_cluster(stack, settings)
        || matches!(
            settings.network,
            Some(
                NetworkSettings::ByoVpcAws { .. }
                    | NetworkSettings::ByoVpcGcp { .. }
                    | NetworkSettings::ByoVnetAzure { .. }
            )
        )
    {
        return Ok(());
    }
    let frozen_gating: std::collections::HashSet<&str> = crate::frozen_gated(stack)
        .map(|(_, input_id)| input_id)
        .collect();
    let declinable = |entry: &ResourceEntry| {
        entry
            .enabled_when
            .as_deref()
            .is_some_and(|input_id| frozen_gating.contains(input_id))
    };
    if stack
        .resources()
        .any(|(_, entry)| keeps_cloud_network(stack, entry) && !declinable(entry))
    {
        return Ok(());
    }
    for (id, entry) in stack.resources() {
        if is_network_database(entry) || !keeps_cloud_network(stack, entry) || !declinable(entry) {
            continue;
        }
        let resource_type = entry.config.resource_type();
        let input_id = entry.enabled_when.as_deref().unwrap_or_default();
        return Err(AlienError::new(ErrorData::SetupPlacementUnsupported {
            message: format!(
                "Setup cannot yet place the {resource_type} '{id}' on an existing cluster while its input '{input_id}' also turns off a resource setup creates, because the network it requires is fixed when the template is rendered. Use a cluster that setup creates, bring the cluster's own network in the stack's network settings, give the {resource_type} an input that turns off only resources the runtime creates, or remove its input."
            ),
            resource_type: resource_type.to_string(),
            resource_id: id.to_string(),
        }));
    }
    Ok(())
}

/// Whether setup emits this resource and registers it with the manager.
///
/// This is the one rule. Both setup generators render exactly the resources it accepts, and the
/// manager expects a registration to hold exactly those, so a change here moves all three
/// together.
///
/// - On Kubernetes the cluster's own nodes and pods stand in for the cloud machines: a
///   `ComputeCluster` is a set of logical pools the Operator validates, and a `Sandbox` is a pod.
///   Setup creates no cloud infrastructure for either, whatever the cloud or whether the cluster
///   is new or existing.
/// - Automatic packages install access and secret storage, then let the runtime create
///   application infrastructure. Cloud API activation and Azure's containing resource group must
///   exist before their management identity can be installed. Azure also installs application
///   identities so management can name their principals in the role-assignment conditions.
/// - Manual packages emit what the resource's ownership policy says.
///
/// Whether a Kubernetes cluster is new or existing decides which resources the stack declares
/// (the cluster and network mutations add or omit them), not which of them setup emits.
pub fn emits_package_scaffolding(
    entry: &ResourceEntry,
    management: SetupManagement,
    target: SetupTarget,
) -> bool {
    let resource_type = entry.config.resource_type();
    if target.is_kubernetes() && is_kubernetes_native_resource_type(&resource_type) {
        return false;
    }
    if management == SetupManagement::Automatic {
        is_automatic_setup_package_resource_type(resource_type.as_ref(), target.cloud_platform)
    } else {
        ownership_policy_for_resource_type(resource_type.as_ref())
            .emits_setup_scaffolding(entry.lifecycle)
    }
}

/// Resources whose state is replaced by an automatic setup package import.
/// Application resources are managed through the installed identity and must
/// retain their controller state when that package is reimported.
pub fn is_automatic_setup_package_resource_type(resource_type: &str, platform: Platform) -> bool {
    matches!(
        resource_type,
        "remote-stack-management" | "vault" | "azure_resource_group" | "service_activation"
    ) || (platform == Platform::Azure
        && matches!(resource_type, "service-account" | "resource-access"))
}

/// Whether deleting an installation through runtime cleanup removes this resource before the
/// setup package's own teardown runs.
///
/// An automatic package creates only its scaffolding types (see
/// [`is_automatic_setup_package_resource_type`]). The runtime created everything else, Frozen
/// resources included, and the package's teardown cannot see those, so runtime cleanup removes
/// them. It leaves the package's resources, Live vaults included, to that teardown. Any other
/// package owns its Frozen resources, apart from types that need runtime cleanup first.
pub fn removed_by_runtime_cleanup(
    resource_type: &str,
    lifecycle: ResourceLifecycle,
    management: SetupManagement,
    platform: Platform,
) -> bool {
    if management == SetupManagement::Automatic {
        return !is_automatic_setup_package_resource_type(resource_type, platform);
    }
    lifecycle == ResourceLifecycle::Live
        || ownership_policy_for_resource_type(resource_type).has_runtime_cleanup_before_teardown()
}

#[cfg(test)]
mod cors_tests {
    use super::*;

    #[test]
    fn automatic_cors_requires_an_implementing_controller() {
        let stack = Stack::new("browser-storage".into())
            .add(
                Storage::new("assets".into())
                    .cors_allowed_origins(vec!["https://app.example.com".into()])
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        for platform in [Platform::Aws, Platform::Gcp] {
            validate_automatic_setup_resources(&stack, platform, SetupManagement::Automatic)
                .unwrap();
        }
        let error =
            validate_automatic_setup_resources(&stack, Platform::Azure, SetupManagement::Automatic)
                .unwrap_err();
        assert!(matches!(
            error.error,
            Some(ErrorData::AutomaticSetupResourceUnsupported { .. })
        ));
    }
}

#[cfg(test)]
mod custom_resource_grant_tests {
    use super::*;
    use crate::{ResourceType, WorkerCode};

    fn linked_zone() -> Stack {
        let zone =
            CustomResource::from_untagged("acme/dns-zone", serde_json::json!({ "id": "zone" }))
                .unwrap();
        let mut api = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "registry.example.com/api:1".to_string(),
            })
            .permissions("api".to_string())
            .build();
        api.links.push(ResourceRef::new(
            ResourceType::from("acme/dns-zone"),
            "zone",
        ));
        let mut stack = Stack::new("app".to_string())
            .add(zone, ResourceLifecycle::Live)
            .add(api, ResourceLifecycle::Live)
            .build();
        stack
            .extensions
            .push(acme(&["acme/dns-zone/records-write"]));
        stack
    }

    /// The extension that defines `acme/dns-zone`, whose type links `link_sets`.
    fn acme(link_sets: &[&str]) -> crate::ExtensionManifest {
        serde_json::from_value(serde_json::json!({
            "name": "acme", "version": "1.0.0", "digest": "sha256:0",
            "resourceTypes": { "acme/dns-zone": {
                "lifecycles": ["live"], "holdsData": false,
                "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
                "controllerPlatforms": ["aws"], "permissionSets": [],
                "link": { "permissionSets": link_sets },
                "schemas": { "config": {}, "outputs": {} }
            }}
        }))
        .unwrap()
    }

    /// A workload that links a custom resource needs a grant setup writes, which automatic setup
    /// on AWS can't, at package generation and when a later release adds the link alike.
    #[test]
    fn automatic_setup_on_aws_refuses_a_grant_on_a_custom_resource() {
        let error = validate_automatic_setup_resources(
            &linked_zone(),
            Platform::Aws,
            SetupManagement::Automatic,
        )
        .expect_err("the link is refused");
        assert_eq!(
            error.code, "AUTOMATIC_SETUP_RESOURCE_UNSUPPORTED",
            "{error}"
        );
        assert!(error.to_string().contains("'zone'"), "{error}");
        assert!(!error.retryable);

        // A permission profile that scopes sets to the resource grants it too.
        let mut scoped = linked_zone();
        let api = scoped.resources.get_mut("api").unwrap();
        api.config.downcast_mut::<Worker>().unwrap().links.clear();
        scoped.permissions.profiles.insert(
            "api".to_string(),
            serde_json::from_value(serde_json::json!({ "zone": ["acme/dns-zone/records-write"] }))
                .unwrap(),
        );
        validate_automatic_setup_resources(&scoped, Platform::Aws, SetupManagement::Automatic)
            .expect_err("the profile's grant is refused");

        let mut unlinked = linked_zone();
        unlinked.resources.shift_remove("api");
        // A link to a type that links no sets grants nothing.
        let mut no_link_sets = linked_zone();
        no_link_sets.extensions = vec![acme(&[])];
        for (stack, platform, management) in [
            (linked_zone(), Platform::Aws, SetupManagement::Manual),
            (linked_zone(), Platform::Gcp, SetupManagement::Automatic),
            (unlinked, Platform::Aws, SetupManagement::Automatic),
            (no_link_sets, Platform::Aws, SetupManagement::Automatic),
        ] {
            validate_automatic_setup_resources(&stack, platform, management)
                .unwrap_or_else(|error| panic!("{platform:?} {management:?}: {error}"));
        }
    }
}
