//! Build planning for native-runner CI.
//!
//! Derives the build targets each supported platform needs and groups them by the native
//! GitHub runner that can build them, so the generated workflow builds on native runners
//! (no emulation) and uses the fewest runners.

use alien_core::{
    BinaryTarget, Daemon, DaemonCode, Platform, Stack, ToolchainConfig, Worker, WorkerCode,
};
use serde::Serialize;

/// One platform/target pair to build within a runner group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildJob {
    /// Platform id (e.g. "aws").
    pub platform: String,
    /// `alien build --targets` name (e.g. "linux-arm64").
    pub target: String,
}

/// Builds that share one native runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerGroup {
    /// Group name — the target name, also the output-dir suffix.
    pub name: String,
    /// GitHub runner label.
    pub runner: String,
    /// The platforms this group builds for, in `builds` order. A convenience for the
    /// generated workflow: it can run `alien build --platforms <a,b,c> --targets <name>`
    /// directly, without re-deriving the list from `builds`.
    pub platforms: Vec<String>,
    /// The platform/target pairs this runner builds.
    pub builds: Vec<BuildJob>,
}

/// Build targets a platform needs in a *release* (distribution) plan. Same as
/// `defaults_for_platform`, except `local`: it always needs both linux arches, plus the
/// darwin/windows host targets when the stack has a native-host-binary resource — see
/// [`stack_targets_native_host_binaries`].
fn release_targets_for_platform(
    platform: Platform,
    local_includes_host_binaries: bool,
) -> Vec<BinaryTarget> {
    match platform {
        Platform::Local if local_includes_host_binaries => vec![
            BinaryTarget::LinuxArm64,
            BinaryTarget::LinuxX64,
            BinaryTarget::DarwinArm64,
            BinaryTarget::WindowsX64,
        ],
        Platform::Local => vec![BinaryTarget::LinuxArm64, BinaryTarget::LinuxX64],
        other => BinaryTarget::defaults_for_platform(other),
    }
}

/// True when the stack has a compute resource that builds to a native host binary — a
/// Worker or Daemon built from Rust or TypeScript source. On `local` those run as native
/// processes, so a distributable build needs a per-OS binary (darwin/windows as well as
/// linux). Containers, prebuilt images, and Docker-toolchain workers are linux-only images
/// even on `local`, so a stack with only those needs just the two linux targets.
pub fn stack_targets_native_host_binaries(stack: &Stack) -> bool {
    stack.resources().any(|(_, entry)| {
        let toolchain = if let Some(worker) = entry.config.downcast_ref::<Worker>() {
            match &worker.code {
                WorkerCode::Source { toolchain, .. } => Some(toolchain),
                WorkerCode::Image { .. } => None,
            }
        } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
            match &daemon.code {
                DaemonCode::Source { toolchain, .. } => Some(toolchain),
                DaemonCode::Image { .. } => None,
            }
        } else {
            None
        };
        matches!(
            toolchain,
            Some(ToolchainConfig::Rust { .. } | ToolchainConfig::TypeScript { .. })
        )
    })
}

/// The `alien build --targets` name for a target (also the group name / output-dir suffix).
fn target_name(target: BinaryTarget) -> &'static str {
    match target {
        BinaryTarget::LinuxArm64 => "linux-arm64",
        BinaryTarget::LinuxX64 => "linux-x64",
        BinaryTarget::DarwinArm64 => "darwin-arm64",
        BinaryTarget::WindowsX64 => "windows-x64",
    }
}

/// Native GitHub runner that can build a target without emulation.
fn runner_for(target: BinaryTarget) -> &'static str {
    match target {
        BinaryTarget::LinuxArm64 => "ubuntu-24.04-arm",
        BinaryTarget::LinuxX64 => "ubuntu-24.04",
        BinaryTarget::DarwinArm64 => "macos-latest",
        BinaryTarget::WindowsX64 => "windows-latest",
    }
}

/// Group the build work for `supported` platforms by native runner — one group per target
/// any platform needs. arm64 is listed before amd64 so it's preferred where a platform allows
/// either (cheaper); amd64 still gets its own group when a platform requires it.
///
/// `local_includes_host_binaries` is [`stack_targets_native_host_binaries`] for the stack:
/// when false (e.g. a Container-only stack), `local` contributes only the two linux targets,
/// so no macOS/Windows runner group is created for builds that would just be linux images.
pub fn plan_runner_groups(
    supported: &[Platform],
    local_includes_host_binaries: bool,
) -> Vec<RunnerGroup> {
    const ORDER: [BinaryTarget; 4] = [
        BinaryTarget::LinuxArm64,
        BinaryTarget::LinuxX64,
        BinaryTarget::DarwinArm64,
        BinaryTarget::WindowsX64,
    ];
    ORDER
        .iter()
        .filter_map(|&target| {
            let builds: Vec<BuildJob> = supported
                .iter()
                .filter(|p| {
                    release_targets_for_platform(**p, local_includes_host_binaries)
                        .contains(&target)
                })
                .map(|p| BuildJob {
                    platform: p.as_str().to_string(),
                    target: target_name(target).to_string(),
                })
                .collect();
            (!builds.is_empty()).then(|| RunnerGroup {
                name: target_name(target).to_string(),
                runner: runner_for(target).to_string(),
                platforms: builds.iter().map(|b| b.platform.clone()).collect(),
                builds,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::Platform::*;
    use alien_core::{Container, ContainerCode, ResourceLifecycle, ResourceSpec};

    fn platforms(group: &RunnerGroup) -> Vec<&str> {
        assert_eq!(
            group.platforms,
            group
                .builds
                .iter()
                .map(|b| b.platform.clone())
                .collect::<Vec<_>>(),
            "platforms field must mirror builds order"
        );
        group.platforms.iter().map(|s| s.as_str()).collect()
    }

    #[test]
    fn local_with_host_binaries_groups_every_native_runner() {
        let groups = plan_runner_groups(&[Aws, Gcp, Azure, Kubernetes, Local], true);

        assert_eq!(
            groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            vec!["linux-arm64", "linux-x64", "darwin-arm64", "windows-x64"]
        );

        assert_eq!(groups[0].runner, "ubuntu-24.04-arm");
        assert_eq!(platforms(&groups[0]), vec!["aws", "kubernetes", "local"]);

        assert_eq!(groups[1].runner, "ubuntu-24.04");
        assert_eq!(
            platforms(&groups[1]),
            vec!["gcp", "azure", "kubernetes", "local"]
        );

        assert_eq!(groups[2].runner, "macos-latest");
        assert_eq!(platforms(&groups[2]), vec!["local"]);

        assert_eq!(groups[3].runner, "windows-latest");
        assert_eq!(platforms(&groups[3]), vec!["local"]);
    }

    #[test]
    fn local_without_host_binaries_is_linux_only() {
        // A Container-only local stack: no native host binary, so no macOS/Windows runner —
        // those builds would just be linux images and collide on merge.
        let groups = plan_runner_groups(&[Aws, Gcp, Azure, Kubernetes, Local], false);
        assert_eq!(
            groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            vec!["linux-arm64", "linux-x64"]
        );
        assert_eq!(platforms(&groups[0]), vec!["aws", "kubernetes", "local"]);
        assert_eq!(
            platforms(&groups[1]),
            vec!["gcp", "azure", "kubernetes", "local"]
        );
    }

    #[test]
    fn no_local_is_two_linux_groups() {
        let groups = plan_runner_groups(&[Aws, Gcp], false);
        assert_eq!(
            groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            vec!["linux-arm64", "linux-x64"]
        );
        assert_eq!(platforms(&groups[0]), vec!["aws"]);
        assert_eq!(platforms(&groups[1]), vec!["gcp"]);
    }

    #[test]
    fn each_build_job_carries_platform_and_target() {
        let groups = plan_runner_groups(&[Aws], false);
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].builds,
            vec![BuildJob {
                platform: "aws".into(),
                target: "linux-arm64".into()
            }]
        );
    }

    fn worker_with(code: WorkerCode) -> Worker {
        Worker::new("api".to_string())
            .permissions("api".to_string())
            .code(code)
            .build()
    }

    fn rust_source() -> WorkerCode {
        WorkerCode::Source {
            src: ".".to_string(),
            toolchain: ToolchainConfig::Rust {
                binary_name: "api".to_string(),
            },
        }
    }

    fn stack_with<T: alien_core::ResourceDefinition>(resource: T) -> Stack {
        Stack::new("plan-test".to_string())
            .add(resource, ResourceLifecycle::Live)
            .build()
    }

    fn container() -> Container {
        Container::new("web".to_string())
            .code(ContainerCode::Image {
                image: "ghcr.io/x/web:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .permissions("web".to_string())
            .build()
    }

    #[test]
    fn predicate_true_for_source_built_worker_or_daemon() {
        assert!(stack_targets_native_host_binaries(&stack_with(
            worker_with(rust_source())
        )));
        assert!(stack_targets_native_host_binaries(&stack_with(
            worker_with(WorkerCode::Source {
                src: ".".to_string(),
                toolchain: ToolchainConfig::TypeScript { binary_name: None },
            })
        )));
        let daemon = Daemon::new("agent".to_string())
            .permissions("agent".to_string())
            .code(DaemonCode::Source {
                src: ".".to_string(),
                toolchain: ToolchainConfig::Rust {
                    binary_name: "agent".to_string(),
                },
            })
            .build();
        assert!(stack_targets_native_host_binaries(&stack_with(daemon)));
    }

    #[test]
    fn predicate_false_for_containers_and_images() {
        // Container → Docker (linux) image even on local.
        assert!(!stack_targets_native_host_binaries(
            &stack_with(container())
        ));
        // Prebuilt-image worker → linux image, not a host binary.
        assert!(!stack_targets_native_host_binaries(&stack_with(
            worker_with(WorkerCode::Image {
                image: "ghcr.io/x/api:latest".to_string(),
            })
        )));
        // Docker-toolchain worker → linux image, not a host binary.
        assert!(!stack_targets_native_host_binaries(&stack_with(
            worker_with(WorkerCode::Source {
                src: ".".to_string(),
                toolchain: ToolchainConfig::Docker {
                    dockerfile: None,
                    build_args: None,
                    target: None
                },
            })
        )));
    }
}
