//! Resources that own links to other resources.
//!
//! A link is an author-declared reference that produces an `ALIEN_<ID>_BINDING` for the
//! linked resource. It is the one reference kind a declined gate can remove: dropping it
//! leaves the holder running with that binding absent. Every other edge a resource carries
//! is structural — a trigger's wiring lives on the source resource, an ordering edge is not
//! a binding — so those keep refusing a gated target.
//!
//! Resolution is by concrete type rather than by resource-type string, so a misspelled tag
//! cannot silently classify a resource as link-free.

use crate::resource::{Resource, ResourceRef};
use crate::resources::{Container, Daemon, Worker};

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

impl_resource_links!(Worker, Container, Daemon);

/// The link-owning view of a resource, or `None` when it owns no links.
///
/// `None` is the ordinary answer for most resource types, and callers that walk every
/// resource should skip them. A caller that has already established it holds a link owner
/// should fail loudly on `None` rather than treating it as empty.
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

    /// Every link owner must resolve, expose its links, and drop exactly the named one.
    /// Parametrised because a type that silently stops participating would otherwise only
    /// surface as a dangling reference in a customer's account.
    #[test]
    fn every_link_owner_resolves_and_scrubs() {
        for (name, mut resource) in [
            ("worker", worker()),
            ("container", container()),
            ("daemon", daemon()),
        ] {
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
        for (name, mut resource) in [
            ("worker", worker()),
            ("container", container()),
            ("daemon", daemon()),
        ] {
            let before: Vec<ResourceRef> = links_of(&resource).to_vec();
            if let Some(owner) = resource_links_mut(&mut resource) {
                let declined: Vec<String> = Vec::new();
                owner.links_mut().retain(|l| !declined.contains(&l.id));
            }
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
}
