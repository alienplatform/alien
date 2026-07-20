//! Build planning for native-runner CI.
//!
//! Derives the build targets each supported platform needs and groups them by the native
//! GitHub runner that can build them, so the generated workflow builds on native runners
//! (no emulation) and uses the fewest runners.

use alien_core::{
    compute_planner::capacity_group_requirements,
    instance_catalog::{self, Architecture},
    BinaryTarget, ComputeCluster, Daemon, DaemonCode, Platform, Stack, ToolchainConfig, Worker,
    WorkerCode,
};
use alien_error::AlienError;
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

/// Plans native runners while keeping cloud image and compute architectures aligned.
pub fn plan_runner_groups_for_stack(
    supported: &[Platform],
    stack: &Stack,
) -> crate::error::Result<Vec<RunnerGroup>> {
    let local_includes_host_binaries = stack_targets_native_host_binaries(stack);
    let mut groups = plan_runner_groups(supported, local_includes_host_binaries);
    let machines_architecture = exact_compute_architecture(stack)?;

    for platform in supported.iter().copied().filter(|platform| {
        matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            || (*platform == Platform::Machines && machines_architecture.is_some())
    }) {
        let required_target = resolve_targets_for_stack_platform(stack, platform, None)?[0];
        let target = target_name(required_target);
        for group in &mut groups {
            group
                .builds
                .retain(|build| build.platform != platform.as_str());
            group.platforms.retain(|name| name != platform.as_str());
        }
        let group = if let Some(group) = groups.iter_mut().find(|group| group.name == target) {
            group
        } else {
            groups.push(RunnerGroup {
                name: target.to_string(),
                runner: runner_for(required_target).to_string(),
                platforms: Vec::new(),
                builds: Vec::new(),
            });
            let index = groups.len() - 1;
            &mut groups[index]
        };
        group.builds.push(BuildJob {
            platform: platform.as_str().to_string(),
            target: target.to_string(),
        });
        group.platforms.push(platform.as_str().to_string());
    }
    groups.retain(|group| !group.builds.is_empty());
    groups.sort_by_key(|group| match group.name.as_str() {
        "linux-arm64" => 0,
        "linux-x64" => 1,
        "darwin-arm64" => 2,
        "windows-x64" => 3,
        _ => 4,
    });
    Ok(groups)
}

/// Resolves cloud image targets from the stack's compute architecture.
pub fn resolve_targets_for_stack_platform(
    stack: &Stack,
    platform: Platform,
    requested: Option<&[BinaryTarget]>,
) -> crate::error::Result<Vec<BinaryTarget>> {
    if platform == Platform::Machines && exact_compute_architecture(stack)?.is_none() {
        return Ok(requested
            .map(<[BinaryTarget]>::to_vec)
            .unwrap_or_else(|| BinaryTarget::defaults_for_platform(platform)));
    }
    if !matches!(
        platform,
        Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Machines
    ) {
        return Ok(requested
            .map(<[BinaryTarget]>::to_vec)
            .unwrap_or_else(|| BinaryTarget::defaults_for_platform(platform)));
    }

    let architecture = if platform == Platform::Machines {
        exact_compute_architecture(stack)?.ok_or_else(|| {
            AlienError::new(crate::error::ErrorData::BuildConfigInvalid {
                message: "machines architecture could not be resolved".to_string(),
            })
        })?
    } else {
        resolved_compute_architecture(stack, platform)?
    };
    let required = match architecture {
        Architecture::Arm64 => BinaryTarget::LinuxArm64,
        Architecture::X86_64 => BinaryTarget::LinuxX64,
    };
    let resolved = requested
        .map(<[BinaryTarget]>::to_vec)
        .unwrap_or_else(|| vec![required]);
    if resolved.as_slice() != [required] {
        return Err(AlienError::new(crate::error::ErrorData::BuildConfigInvalid {
            message: format!(
                "build targets {resolved:?} do not match {platform} compute architecture {architecture:?}; expected {required:?}"
            ),
        }));
    }
    Ok(resolved)
}

fn exact_compute_architecture(stack: &Stack) -> crate::error::Result<Option<Architecture>> {
    let mut architectures = stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<ComputeCluster>())
        .flat_map(|cluster| cluster.capacity_groups.iter())
        .filter_map(|group| group.profile.as_ref()?.architecture)
        .collect::<Vec<_>>();
    architectures.sort_by_key(|architecture| match architecture {
        Architecture::Arm64 => 0,
        Architecture::X86_64 => 1,
    });
    architectures.dedup();
    match architectures.as_slice() {
        [] => Ok(None),
        [architecture] => Ok(Some(*architecture)),
        _ => Err(AlienError::new(crate::error::ErrorData::BuildConfigInvalid {
            message: "compute pools require mixed CPU architectures; one platform image cannot satisfy both".to_string(),
        })),
    }
}

fn resolved_compute_architecture(
    stack: &Stack,
    platform: Platform,
) -> crate::error::Result<Architecture> {
    let architectures = stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<ComputeCluster>())
        .flat_map(|cluster| cluster.capacity_groups.iter())
        .map(|group| {
            instance_catalog::select_instance_type(platform, &capacity_group_requirements(group))
                .map_err(|message| {
                    AlienError::new(crate::error::ErrorData::BuildConfigInvalid {
                        message: format!(
                            "cannot resolve compute pool '{}' for {platform}: {message}",
                            group.group_id
                        ),
                    })
                })?
                .profile
                .architecture
                .ok_or_else(|| {
                    AlienError::new(crate::error::ErrorData::BuildConfigInvalid {
                        message: format!(
                            "selected compute pool '{}' machine has no CPU architecture",
                            group.group_id
                        ),
                    })
                })
        })
        .collect::<crate::error::Result<Vec<_>>>()?;
    if architectures.is_empty() {
        return instance_catalog::default_architecture(platform).ok_or_else(|| {
            AlienError::new(crate::error::ErrorData::BuildConfigInvalid {
                message: format!("managed cloud {platform} has no default compute architecture"),
            })
        });
    }
    let mut architectures = architectures.into_iter().collect::<Vec<_>>();
    architectures.sort_by_key(|architecture| match architecture {
        Architecture::Arm64 => 0,
        Architecture::X86_64 => 1,
    });
    architectures.dedup();
    match architectures.as_slice() {
        [architecture] => Ok(*architecture),
        _ => Err(AlienError::new(crate::error::ErrorData::BuildConfigInvalid {
            message: "compute pools require mixed CPU architectures; one platform image cannot satisfy both".to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::Platform::*;
    use alien_core::{
        CapacityGroup, ComputeCluster, Container, ContainerCode, MachineProfile, ResourceLifecycle,
        ResourceSpec,
    };

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

    fn stack_with_architectures(architectures: &[Architecture]) -> Stack {
        let mut cluster = ComputeCluster::new("runtime".to_string());
        for (index, architecture) in architectures.iter().enumerate() {
            cluster = cluster.capacity_group(CapacityGroup {
                group_id: format!("pool-{index}"),
                instance_type: None,
                profile: Some(MachineProfile {
                    cpu: "1".to_string(),
                    memory_bytes: 2 * 1024 * 1024 * 1024,
                    ephemeral_storage_bytes: 20 * 1024 * 1024 * 1024,
                    architecture: Some(*architecture),
                    gpu: None,
                }),
                min_size: 1,
                max_size: 1,
                scale_policy: None,
                nested_virtualization: None,
            });
        }
        stack_with(cluster.build())
    }

    #[test]
    fn aws_explicit_x86_uses_x64_runner() {
        let stack = stack_with_architectures(&[Architecture::X86_64]);
        let groups = plan_runner_groups_for_stack(&[Aws], &stack).expect("plan should build");

        assert_eq!(groups[0].name, "linux-x64");
    }

    #[test]
    fn aws_without_compute_constraint_keeps_arm_default() {
        let stack = Stack::new("unconstrained".to_string()).build();

        assert_eq!(
            resolve_targets_for_stack_platform(&stack, Aws, None).expect("target should resolve"),
            vec![BinaryTarget::LinuxArm64]
        );
    }

    #[test]
    fn gcp_and_azure_without_compute_constraint_keep_x64_default() {
        let stack = Stack::new("unconstrained".to_string()).build();

        for platform in [Gcp, Azure] {
            assert_eq!(
                resolve_targets_for_stack_platform(&stack, platform, None)
                    .expect("target should resolve"),
                vec![BinaryTarget::LinuxX64]
            );
        }
    }

    #[test]
    fn machines_exact_x86_narrows_images_and_ci_runner() {
        let stack = stack_with_architectures(&[Architecture::X86_64]);

        assert_eq!(
            resolve_targets_for_stack_platform(&stack, Machines, None)
                .expect("target should resolve"),
            vec![BinaryTarget::LinuxX64]
        );
        let groups = plan_runner_groups_for_stack(&[Machines], &stack).expect("plan should build");
        assert_eq!(
            groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            vec!["linux-x64"]
        );
    }

    #[test]
    fn machines_without_constraint_preserves_multi_arch_images() {
        let stack = Stack::new("unconstrained".to_string()).build();

        assert_eq!(
            resolve_targets_for_stack_platform(&stack, Machines, None)
                .expect("targets should resolve"),
            BinaryTarget::LINUX
        );
    }

    #[test]
    fn aws_explicit_arm_uses_arm_runner() {
        let stack = stack_with_architectures(&[Architecture::Arm64]);
        let groups = plan_runner_groups_for_stack(&[Aws], &stack).expect("plan should build");

        assert_eq!(groups[0].name, "linux-arm64");
    }

    #[test]
    fn gcp_rejects_explicit_arm_before_build() {
        let stack = stack_with_architectures(&[Architecture::Arm64]);

        assert!(plan_runner_groups_for_stack(&[Gcp], &stack).is_err());
    }

    #[test]
    fn mixed_compute_architectures_are_rejected_before_build() {
        let stack = stack_with_architectures(&[Architecture::Arm64, Architecture::X86_64]);

        assert!(plan_runner_groups_for_stack(&[Aws], &stack).is_err());
    }

    #[test]
    fn requested_target_must_match_compute_architecture() {
        let stack = stack_with_architectures(&[Architecture::X86_64]);

        assert!(
            resolve_targets_for_stack_platform(&stack, Aws, Some(&[BinaryTarget::LinuxArm64]),)
                .is_err()
        );
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
