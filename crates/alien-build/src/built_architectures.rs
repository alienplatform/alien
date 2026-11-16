//! The build's record of the CPU architectures of each workload's image.
//!
//! Kubernetes places a Pod on nodes of the image's architecture only when the build
//! recorded it, so a cluster with arm64 and amd64 nodes cannot start an image on the
//! wrong one. Only Kubernetes builds record it; other releases are unchanged.

use alien_core::{instance_catalog::Architecture, BinaryTarget, Container, Daemon, Stack, Worker};

/// The architecture of a Linux build target, if it is one.
fn architecture_of(target: &BinaryTarget) -> Option<Architecture> {
    match target {
        BinaryTarget::LinuxX64 => Some(Architecture::X86_64),
        BinaryTarget::LinuxArm64 => Some(Architecture::Arm64),
        BinaryTarget::WindowsX64 | BinaryTarget::DarwinArm64 => None,
    }
}

/// Architectures of the Linux targets, without duplicates, in a fixed order.
pub(crate) fn architectures_of_targets(targets: &[BinaryTarget]) -> Vec<Architecture> {
    let mut result: Vec<Architecture> = Vec::new();
    for architecture in targets.iter().filter_map(architecture_of) {
        if !result.contains(&architecture) {
            result.push(architecture);
        }
    }
    result.sort_by_key(|architecture| match architecture {
        Architecture::X86_64 => 0,
        Architecture::Arm64 => 1,
    });
    result
}

/// Forget any recorded architectures: only a build may say what it built.
pub(crate) fn clear(stack: &mut Stack) {
    for (_, entry) in stack.resources_mut() {
        set_on(&mut entry.config, Vec::new());
    }
}

/// The Containers, Daemons and Workers built from source: only their images are known
/// to have the build's architectures (a prebuilt image reference says nothing).
pub(crate) fn built_from_source(stack: &Stack) -> Vec<String> {
    stack
        .resources()
        .filter(|(_, entry)| {
            let config = &entry.config;
            config
                .downcast_ref::<Container>()
                .is_some_and(|c| matches!(c.code, alien_core::ContainerCode::Source { .. }))
                || config
                    .downcast_ref::<Daemon>()
                    .is_some_and(|d| matches!(d.code, alien_core::DaemonCode::Source { .. }))
                || config
                    .downcast_ref::<Worker>()
                    .is_some_and(|w| matches!(w.code, alien_core::WorkerCode::Source { .. }))
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Record the architectures on the named resources.
pub(crate) fn stamp(stack: &mut Stack, resource_ids: &[String], architectures: &[Architecture]) {
    for id in resource_ids {
        set(stack, id, architectures.to_vec());
    }
}

/// What one resource of a built stack records. Empty for other resources.
pub(crate) fn recorded(stack: &Stack, resource_id: &str) -> Vec<Architecture> {
    let Some((_, entry)) = stack.resources().find(|(id, _)| *id == resource_id) else {
        return Vec::new();
    };
    if let Some(container) = entry.config.downcast_ref::<Container>() {
        container.built_architectures.clone()
    } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
        daemon.built_architectures.clone()
    } else if let Some(worker) = entry.config.downcast_ref::<Worker>() {
        worker.built_architectures.clone()
    } else {
        Vec::new()
    }
}

/// Replace what one resource records. No-op for other resources.
pub(crate) fn set(stack: &mut Stack, resource_id: &str, architectures: Vec<Architecture>) {
    if let Some((_, entry)) = stack.resources_mut().find(|(id, _)| *id == resource_id) {
        set_on(&mut entry.config, architectures);
    }
}

fn set_on(config: &mut alien_core::Resource, architectures: Vec<Architecture>) {
    if let Some(container) = config.downcast_mut::<Container>() {
        container.built_architectures = architectures;
    } else if let Some(daemon) = config.downcast_mut::<Daemon>() {
        daemon.built_architectures = architectures;
    } else if let Some(worker) = config.downcast_mut::<Worker>() {
        worker.built_architectures = architectures;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ContainerCode, ResourceLifecycle, ResourceSpec};

    fn stack() -> Stack {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "example.test/api:1".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "128Mi".to_string(),
                desired: "128Mi".to_string(),
            })
            .permissions("default".to_string())
            .build();
        Stack::new("test".to_string())
            .add(container, ResourceLifecycle::Live)
            .build()
    }

    #[test]
    fn linux_targets_give_their_architectures_once_and_in_order() {
        assert_eq!(
            architectures_of_targets(&[
                BinaryTarget::LinuxArm64,
                BinaryTarget::LinuxX64,
                BinaryTarget::LinuxX64
            ]),
            vec![Architecture::X86_64, Architecture::Arm64]
        );
        assert!(architectures_of_targets(&[BinaryTarget::DarwinArm64]).is_empty());
    }

    #[test]
    fn stamp_records_on_workloads_and_clear_removes_it() {
        let mut stack = stack();
        assert!(recorded(&stack, "api").is_empty());
        stamp(&mut stack, &["api".to_string()], &[Architecture::X86_64]);
        assert_eq!(recorded(&stack, "api"), vec![Architecture::X86_64]);
        clear(&mut stack);
        assert!(recorded(&stack, "api").is_empty());
    }

    #[test]
    fn a_prebuilt_image_is_not_built_from_source() {
        assert!(built_from_source(&stack()).is_empty());
    }

    #[test]
    fn the_field_is_absent_from_json_until_recorded() {
        let mut stack = stack();
        let before = serde_json::to_string(&stack).unwrap();
        assert!(!before.contains("builtArchitectures"));
        stamp(&mut stack, &["api".to_string()], &[Architecture::X86_64]);
        let after = serde_json::to_string(&stack).unwrap();
        assert!(
            after.contains("\"builtArchitectures\":[\"x86_64\"]"),
            "{after}"
        );
    }
}
