//! Resources that own links to other resources.
//!
//! A link produces an `ALIEN_<ID>_BINDING` and is the one reference kind a declined gate can
//! remove; structural edges cannot. Resolution is by concrete type, not by tag string.

use crate::resource::{Resource, ResourceRef};
use crate::resources::{Build, Container, Daemon, Worker};

/// A resource definition that owns resource links.
pub trait ResourceLinks {
    /// The links this definition owns, excluding triggers and ordering edges.
    fn links(&self) -> &[ResourceRef];

    /// Mutable access, for dropping links to resources leaving the stack.
    fn links_mut(&mut self) -> &mut Vec<ResourceRef>;
}

macro_rules! impl_resource_links {
    ($($ty:ty),+ $(,)?) => {$(
        impl ResourceLinks for $ty {
            fn links(&self) -> &[ResourceRef] {
                &self.links
            }

            fn links_mut(&mut self) -> &mut Vec<ResourceRef> {
                &mut self.links
            }
        }
    )+};
}

impl_resource_links!(Worker, Container, Daemon, Build);

/// The link-owning view of a resource, or `None` when it owns no links.
///
/// `None` is ordinary and callers walking every resource skip it; a caller that has already
/// established it holds a link owner should fail loudly instead.
pub fn resource_links(resource: &Resource) -> Option<&dyn ResourceLinks> {
    if let Some(worker) = resource.downcast_ref::<Worker>() {
        return Some(worker);
    }
    if let Some(container) = resource.downcast_ref::<Container>() {
        return Some(container);
    }
    if let Some(daemon) = resource.downcast_ref::<Daemon>() {
        return Some(daemon);
    }
    // Build is not a compute kind, but it owns author-declared links producing the same
    // bindings, so a declined gate must reach them too. Strip timing follows the target's
    // lifecycle, not Build's, so a Build linking a live-gated target takes the late strip.
    if let Some(build) = resource.downcast_ref::<Build>() {
        return Some(build);
    }
    None
}

/// Mutable counterpart of [`resource_links`].
pub fn resource_links_mut(resource: &mut Resource) -> Option<&mut dyn ResourceLinks> {
    // Probed immutably first: returning a `&mut` borrow out of a conditional keeps the
    // borrow alive across the whole function, which rejects a downcast chain.
    if resource.downcast_ref::<Worker>().is_some() {
        return resource
            .downcast_mut::<Worker>()
            .map(|worker| worker as &mut dyn ResourceLinks);
    }
    if resource.downcast_ref::<Container>().is_some() {
        return resource
            .downcast_mut::<Container>()
            .map(|container| container as &mut dyn ResourceLinks);
    }
    if resource.downcast_ref::<Daemon>().is_some() {
        return resource
            .downcast_mut::<Daemon>()
            .map(|daemon| daemon as &mut dyn ResourceLinks);
    }
    if resource.downcast_ref::<Build>().is_some() {
        return resource
            .downcast_mut::<Build>()
            .map(|build| build as &mut dyn ResourceLinks);
    }
    None
}

/// The links a resource owns, empty when it owns none.
pub fn links_of(resource: &Resource) -> &[ResourceRef] {
    resource_links(resource).map_or(&[], |owner| owner.links())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{ContainerCode, DaemonCode, Kv, ResourceSpec, WorkerCode};
    use serde::Deserialize;

    fn kv(id: &str) -> Kv {
        Kv::new(id.to_string()).build()
    }

    fn worker() -> Resource {
        Resource::new(
            Worker::new("api".to_string())
                .permissions("execution".to_string())
                .code(WorkerCode::Image {
                    image: "example.com/api:latest".to_string(),
                })
                .link(&kv("cache"))
                .link(&kv("store"))
                .build(),
        )
    }

    fn container() -> Resource {
        Resource::new(
            Container::new("api".to_string())
                .code(ContainerCode::Image {
                    image: "example.com/api:latest".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "0.25".to_string(),
                    desired: "0.5".to_string(),
                })
                .memory(ResourceSpec {
                    min: "256Mi".to_string(),
                    desired: "512Mi".to_string(),
                })
                .replicas(1)
                .permissions("execution".to_string())
                .port(8080)
                .link(&kv("cache"))
                .link(&kv("store"))
                .build(),
        )
    }

    fn daemon() -> Resource {
        Resource::new(
            Daemon::new("agent".to_string())
                .code(DaemonCode::Image {
                    image: "example.com/agent:latest".to_string(),
                })
                .cluster("runtime".to_string())
                .permissions("execution".to_string())
                .link(&kv("cache"))
                .link(&kv("store"))
                .build(),
        )
    }

    fn build() -> Resource {
        Resource::new(
            Build::new("builder".to_string())
                .permissions("build".to_string())
                .link(&kv("cache"))
                .link(&kv("store"))
                .build(),
        )
    }

    /// One entry per wired link owner. The resolve, scrub and drift tests all read this, so
    /// adding a type is a single edit rather than several independently-trusted ones.
    const FIXTURES: &[(&str, fn() -> Resource)] = &[
        ("worker", worker),
        ("container", container),
        ("daemon", daemon),
        ("build", build),
    ];

    /// Every link owner must resolve, expose its links, and drop exactly the named one.
    /// Parametrised because a type that silently stops participating would otherwise only
    /// surface as a dangling reference in a deployed account.
    #[test]
    fn every_link_owner_resolves_and_scrubs() {
        for (name, make) in FIXTURES {
            let mut resource = make();
            assert_eq!(
                links_of(&resource).len(),
                2,
                "{name} should start with both links"
            );

            let owner = resource_links_mut(&mut resource)
                .unwrap_or_else(|| panic!("{name} must resolve as a link owner"));
            owner.links_mut().retain(|l| l.id != "cache");

            let remaining: Vec<&str> = links_of(&resource).iter().map(|l| l.id.as_str()).collect();
            assert_eq!(remaining, vec!["store"], "{name} kept the wrong link");
        }
    }

    /// Accepting must leave every link in place, or the scrub would be removing links it
    /// was never asked to remove.
    #[test]
    fn no_declines_leaves_every_link_owner_untouched() {
        for (name, make) in FIXTURES {
            let mut resource = make();
            let before: Vec<ResourceRef> = links_of(&resource).to_vec();
            assert_eq!(before.len(), 2, "{name} should start with both links");
            // Resolved strictly: behind an `if let` a broken resolver would skip the
            // mutation entirely and the equality below would still hold.
            let owner = resource_links_mut(&mut resource)
                .unwrap_or_else(|| panic!("{name} must resolve as a link owner"));
            let declined: Vec<String> = Vec::new();
            owner.links_mut().retain(|l| !declined.contains(&l.id));
            assert_eq!(links_of(&resource), before.as_slice(), "{name} changed");
        }
    }

    /// A resource that owns no links resolves to `None` rather than an empty owner, so a
    /// caller that requires one can fail loudly instead of silently seeing zero links.
    #[test]
    fn a_non_link_owner_does_not_resolve() {
        let mut store = Resource::new(Kv::new("store".to_string()).build());

        assert!(resource_links(&store).is_none());
        assert!(resource_links_mut(&mut store).is_none());
        assert!(links_of(&store).is_empty());
    }

    /// Every resource type the deserializer accepts, and whether it owns links.
    ///
    /// The `Deserialize for Resource` match is the registry; this only records classification.
    const LINK_OWNERSHIP: &[(&str, bool)] = &[
        ("worker", true),
        ("container", true),
        ("daemon", true),
        ("build", true),
        ("vault", false),
        ("compute-cluster", false),
        ("kubernetes-cluster", false),
        ("storage", false),
        ("queue", false),
        ("email", false),
        ("kv", false),
        ("postgres", false),
        ("ai", false),
        ("network", false),
        ("service-account", false),
        ("artifact-registry", false),
        ("service_activation", false),
        ("remote-stack-management", false),
        ("resource-access", false),
        ("azure_resource_group", false),
        ("azure_storage_account", false),
        ("azure_container_apps_environment", false),
        ("azure_service_bus_namespace", false),
        ("experimental/aws-opensearch", false),
        // Never customer-declared (the manager injects it, see
        // alien-manager's stack_augmentation), so it has no author-declared
        // `links` field to resolve or scrub.
        ("operations_worker", false),
    ];

    /// Drift guard. The registered types are read out of the deserializer's own
    /// `unknown_variant` refusal rather than restated, so a new type that never declares
    /// whether it owns links fails here instead of silently opting out of scrubbing.
    #[test]
    fn every_registered_resource_type_declares_link_ownership() {
        let refusal = Resource::deserialize(serde_json::json!({ "type": "not-a-resource" }))
            .expect_err("an unregistered type must be refused");
        let message = refusal.to_string();

        let registered: Vec<&str> = message
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|tag| *tag != "not-a-resource")
            .collect();

        assert!(
            registered.len() > 10,
            "could not read the registered types out of: {message}"
        );

        for tag in &registered {
            assert!(
                LINK_OWNERSHIP.iter().any(|(known, _)| known == tag),
                "resource type '{tag}' is registered but does not declare whether it owns \
                 links. Add it to LINK_OWNERSHIP in resource_links.rs"
            );
        }

        for (tag, _) in LINK_OWNERSHIP {
            assert!(
                registered.contains(tag),
                "'{tag}' is classified but no longer registered; drop it from LINK_OWNERSHIP"
            );
        }

        // Presence alone would pass a type declared `true` that nobody wired into
        // `impl_resource_links!`, which then never resolves and is never scrubbed.
        let declared: Vec<&str> = LINK_OWNERSHIP
            .iter()
            .filter(|(_, owns)| *owns)
            .map(|(tag, _)| *tag)
            .collect();
        for (tag, make) in FIXTURES {
            assert!(
                declared.contains(tag),
                "fixture '{tag}' is not declared a link owner"
            );
            assert!(
                resource_links(&make()).is_some(),
                "'{tag}' is declared a link owner but does not resolve as one"
            );
        }
        assert_eq!(
            declared.len(),
            FIXTURES.len(),
            "declared link owners {declared:?} have no fixture proving they resolve"
        );
    }
}
