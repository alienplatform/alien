//! Sandbox resource for running untrusted code in an isolated environment.
//!
//! A Sandbox is a session-oriented resource: the declaration provisions a durable parent, and
//! the application creates and destroys individual sessions through its binding at runtime.
//!
//! The capability set differs per platform and is published rather than assumed. Calling an
//! unsupported capability is a typed error naming both the platform and the capability, so a
//! portable application can branch on `SandboxCapabilities` before it calls.

use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use crate::resources::ToolchainConfig;
use crate::Platform;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;

/// Specifies where the sandbox's root filesystem comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SandboxCode {
    /// A prebuilt container image used as the sandbox root filesystem.
    #[serde(rename_all = "camelCase")]
    Image {
        /// Image reference (e.g. `ubuntu:24.04`, `ghcr.io/myorg/sandbox:latest`)
        image: String,
    },
    /// Source built into a sandbox image at deploy time.
    #[serde(rename_all = "camelCase")]
    Source {
        /// The source directory to build from
        src: String,
        /// Toolchain configuration with type-safe options
        toolchain: ToolchainConfig,
    },
}

/// Hard ceilings enforced on a sandbox session.
///
/// These are limits, not scheduling requests. Untrusted code does not respect a hint, so every
/// field is enforced by the platform and a platform that cannot enforce one is rejected at plan
/// time rather than silently ignoring it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxLimits {
    /// CPU ceiling in cores or millicores (e.g. `"1"`, `"500m"`)
    pub cpu: String,
    /// Memory ceiling (e.g. `"2Gi"`, `"512Mi"`)
    pub memory: String,
    /// Disk ceiling (e.g. `"20Gi"`)
    pub disk: String,
    /// Maximum number of processes, which bounds fork bombs.
    ///
    /// Optional because only a container runtime has the primitive: Kubernetes sets a pid ceiling
    /// per node, not per pod, and neither AWS MicroVMs nor Azure sandboxes expose one. Declaring
    /// it on a platform that cannot apply it is refused at plan time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_processes: Option<u32>,
}

/// One of the five sizes a Lambda MicroVM can be built at.
///
/// AWS has no ceiling knob: `minimumMemoryInMiB` sets a *baseline* and a running MicroVM bursts
/// vertically to four times it with no way to opt out. A declared ceiling is therefore honoured by
/// picking the tier whose **peak** stays inside it, not the tier whose baseline matches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicrovmTier {
    /// What `minimumMemoryInMiB` is set to.
    pub baseline_memory_mib: i64,
    /// The most memory the MicroVM can reach, in MiB.
    pub peak_memory_mib: i64,
    /// The most vCPU the MicroVM can reach.
    pub peak_vcpu: u32,
    /// The most disk the MicroVM can use, in MiB.
    pub max_disk_mib: i64,
}

/// The published sizes, smallest first. Baseline memory to vCPU is 2 GB per vCPU, peak is four
/// times baseline, and disk is fixed per tier rather than independently selectable.
/// Longest life AWS will run a MicroVM for, from `RunMicrovm`'s `maximumDurationInSeconds`.
const AWS_MAX_SESSION_LIFETIME_SECONDS: u32 = 28_800;

const MICROVM_TIERS: &[MicrovmTier] = &[
    MicrovmTier {
        baseline_memory_mib: 512,
        peak_memory_mib: 2048,
        peak_vcpu: 1,
        max_disk_mib: 8192,
    },
    MicrovmTier {
        baseline_memory_mib: 1024,
        peak_memory_mib: 4096,
        peak_vcpu: 2,
        max_disk_mib: 8192,
    },
    MicrovmTier {
        baseline_memory_mib: 2048,
        peak_memory_mib: 8192,
        peak_vcpu: 4,
        max_disk_mib: 8192,
    },
    MicrovmTier {
        baseline_memory_mib: 4096,
        peak_memory_mib: 16384,
        peak_vcpu: 8,
        max_disk_mib: 16384,
    },
    MicrovmTier {
        baseline_memory_mib: 8192,
        peak_memory_mib: 32768,
        peak_vcpu: 16,
        max_disk_mib: 32768,
    },
];

/// Outbound network policy for a sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "mode")]
pub enum SandboxEgress {
    /// No outbound network access.
    ///
    /// Routed traffic only. Link-local is not outbound and no backend's egress control reaches
    /// it, so this is not a boundary against instance metadata.
    Deny,
    /// Unrestricted outbound access to the public internet, and none to private ranges or the
    /// deployment's own network.
    ///
    /// Link-local carries the same exception as `Deny`.
    Allow,
    /// Outbound access only to the listed hostnames. No backend expresses this yet.
    #[serde(rename_all = "camelCase")]
    AllowDomains {
        /// Hostnames the sandbox may reach
        domains: Vec<String>,
    },
}

/// How long a session may live and when it is suspended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxSessionPolicy {
    /// Wall-clock ceiling on a single session, after which the platform terminates it.
    ///
    /// Optional because not every backend has the primitive: Kubernetes has
    /// `activeDeadlineSeconds` and AWS `maximumDurationInSeconds`, while neither Azure nor Local
    /// expose one, so declaring a ceiling there is refused at plan time rather than accepted and
    /// never applied. AWS caps it at 8 hours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lifetime_seconds: Option<u32>,
    /// Idle period after which the session is suspended, where the platform supports it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_suspend_seconds: Option<u32>,
}

/// What a platform's sandbox backend can actually do.
///
/// Published so portable code can branch before calling rather than discovering a gap through
/// an error. Every field here corresponds to a capability that at least one platform lacks;
/// create, exec and terminate are the guaranteed floor and are therefore not listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxCapabilities {
    /// Files can be moved in and out of a session
    ///
    /// Every backend but Azure, whose binding implements no transfer.
    pub files: bool,
    /// A later call can reach a session created by an earlier one
    pub reconnect: bool,
    /// An authenticated, port-scoped capability to reach a service inside the sandbox
    pub preview: bool,
    /// Session state can be suspended and resumed
    pub suspend_resume: bool,
    /// A session's full state can be captured and used to create another
    pub snapshot: bool,
    /// Egress can be restricted to a hostname allowlist
    pub domain_egress_rules: bool,
    /// Whether a declared `deny` is actually enforced, rather than accepted and dropped
    pub egress_deny: bool,
    /// The platform enforces the declared cpu, memory and disk ceilings
    pub enforced_limits: bool,
    /// The platform can cap how many processes a session runs
    pub process_limit: bool,
    /// The platform terminates a session at a declared wall-clock deadline
    pub session_lifetime: bool,
    /// A command runs in its own PID namespace and cannot see or signal the agent's processes.
    ///
    /// Only where an agent runs as root. Creating the namespace needs `CAP_SYS_ADMIN`, and the
    /// Kubernetes sandbox pod drops every capability — which is also what denies `ptrace` by
    /// construction, so granting it there would remove a lock to add one.
    pub supervisor_pid_namespace: bool,
}

impl SandboxCapabilities {
    /// Returns what the given platform's sandbox backend supports.
    ///
    /// Errors for platforms with no sandbox backend, rather than returning an all-false set —
    /// "every capability is missing" and "this platform has no sandboxes" are different
    /// conditions and an application should not have to tell them apart by inspection.
    pub fn for_platform(platform: Platform) -> Result<Self> {
        match platform {
            Platform::Aws => Ok(Self {
                files: true,
                reconnect: true,
                preview: true,
                suspend_resume: true,
                snapshot: false,
                domain_egress_rules: false,
                egress_deny: true,
                enforced_limits: true,
                // Nothing in the API bounds process count.
                process_limit: false,
                // `maximumDurationInSeconds` on `RunMicrovm`, which Lambda enforces by
                // terminating the MicroVM. Capped at 8 hours by the service.
                session_lifetime: true,
                // Measured, not assumed: the agent inside a Lambda MicroVM runs as uid 0 with
                // `CapEff: 00000000a80425fb`, the standard container default set, which excludes
                // `CAP_SYS_ADMIN`. It can drop privilege (`CAP_SETUID`/`CAP_SETGID` are held) and
                // it cannot create a namespace. No backend offers this today.
                supervisor_pid_namespace: false,
            }),
            // Azure the platform has all three — a per-port URL closed to anonymous traffic, a
            // 0.54s resume, and a full-VM snapshot — and the binding provider implements none of
            // them. The capability set describes what a caller can reach, not what the cloud
            // could do, so these stay false until the provider catches up.
            Platform::Azure => Ok(Self {
                files: false,
                reconnect: true,
                preview: false,
                suspend_resume: false,
                snapshot: false,
                domain_egress_rules: false,
                egress_deny: false,
                enforced_limits: false,
                process_limit: false,
                session_lifetime: false,
                // No Alien process inside an Azure sandbox, so there is no supervisor to isolate.
                supervisor_pid_namespace: false,
            }),
            // A Cloud Run sandbox id is scoped to one instance, and session affinity does not
            // hold one across turns. That is the absence of a reconnect guarantee, not a
            // degraded one.
            Platform::Gcp => Ok(Self {
                files: true,
                reconnect: false,
                preview: false,
                suspend_resume: false,
                snapshot: false,
                domain_egress_rules: false,
                egress_deny: true,
                enforced_limits: false,
                process_limit: false,
                session_lifetime: false,
                // A Cloud Run sandbox is a subprocess of the workload; nothing of ours is inside.
                supervisor_pid_namespace: false,
            }),
            // Preview needs a gateway that validates a session-and-port capability, and that
            // gateway does not exist yet.
            Platform::Kubernetes => Ok(Self {
                files: true,
                reconnect: true,
                preview: false,
                suspend_resume: false,
                snapshot: false,
                domain_egress_rules: false,
                egress_deny: true,
                enforced_limits: true,
                // A pid ceiling is a kubelet setting per node, not a pod field.
                process_limit: false,
                // `activeDeadlineSeconds` on the pod, which the kubelet enforces.
                session_lifetime: true,
                // The pod drops every capability, including the `CAP_SYS_ADMIN` the agent would
                // need to unshare. That is also what denies `ptrace`, so this stays false rather
                // than the pod being weakened to make it true.
                supervisor_pid_namespace: false,
            }),
            Platform::Local => Ok(Self {
                files: true,
                reconnect: true,
                preview: true,
                suspend_resume: false,
                snapshot: false,
                domain_egress_rules: false,
                egress_deny: true,
                enforced_limits: true,
                // Docker's `--pids-limit`.
                process_limit: true,
                session_lifetime: false,
                // Local has no in-sandbox agent: the manager drives Docker from outside, so
                // there is no supervisor sharing the sandbox to isolate from.
                supervisor_pid_namespace: false,
            }),
            Platform::Machines | Platform::Test => {
                Err(AlienError::new(ErrorData::SandboxPlatformUnsupported {
                    platform: platform.to_string(),
                }))
            }
        }
    }

    /// Returns a typed error if the named capability is absent on this platform.
    pub fn require(&self, capability: SandboxCapability, platform: Platform) -> Result<()> {
        let available = match capability {
            SandboxCapability::Files => self.files,
            SandboxCapability::Reconnect => self.reconnect,
            SandboxCapability::Preview => self.preview,
            SandboxCapability::SuspendResume => self.suspend_resume,
            SandboxCapability::Snapshot => self.snapshot,
            SandboxCapability::DomainEgressRules => self.domain_egress_rules,
            SandboxCapability::EgressDeny => self.egress_deny,
            SandboxCapability::EnforcedLimits => self.enforced_limits,
            SandboxCapability::ProcessLimit => self.process_limit,
            SandboxCapability::SessionLifetime => self.session_lifetime,
            SandboxCapability::SupervisorPidNamespace => self.supervisor_pid_namespace,
        };

        if available {
            return Ok(());
        }

        Err(AlienError::new(ErrorData::SandboxCapabilityUnsupported {
            capability: capability.as_str().to_string(),
            platform: platform.to_string(),
        }))
    }
}

/// Names a single sandbox capability, so an unsupported call can report which one it needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum SandboxCapability {
    /// Moving files in and out of a session
    Files,
    /// Reaching a session created by an earlier call
    Reconnect,
    /// An authenticated, port-scoped ingress capability
    Preview,
    /// Suspending and resuming session state
    SuspendResume,
    /// Capturing full session state
    Snapshot,
    /// Restricting egress to a hostname allowlist
    DomainEgressRules,
    /// Refusing outbound access when a sandbox declares none
    EgressDeny,
    /// Platform-enforced resource ceilings
    EnforcedLimits,
    /// A ceiling on the number of processes a session may run
    ProcessLimit,
    /// A wall-clock ceiling on a session, applied by the platform rather than by a caller
    SessionLifetime,
    /// A command runs in its own PID namespace, isolated from the agent supervising it
    SupervisorPidNamespace,
}

impl SandboxCapability {
    /// Returns the stable identifier used in errors and capability queries.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Reconnect => "reconnect",
            Self::Preview => "preview",
            Self::SuspendResume => "suspendResume",
            Self::Snapshot => "snapshot",
            Self::DomainEgressRules => "domainEgressRules",
            Self::EgressDeny => "egressDeny",
            Self::EnforcedLimits => "enforcedLimits",
            Self::ProcessLimit => "processLimit",
            Self::SessionLifetime => "sessionLifetime",
            Self::SupervisorPidNamespace => "supervisorPidNamespace",
        }
    }
}

/// An isolated environment for running untrusted code, created per session at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Sandbox {
    /// Identifier for the sandbox. Must contain only alphanumeric characters, hyphens, and
    /// underscores ([A-Za-z0-9-_]). Maximum 64 characters.
    #[builder(start_fn)]
    pub id: String,
    /// Where the sandbox's root filesystem comes from
    pub code: SandboxCode,
    /// Enforced resource ceilings.
    ///
    /// Optional because not every platform can enforce them, and a declaration that names none
    /// takes the platform's own defaults. Naming them on a platform that cannot enforce them is
    /// rejected at plan time rather than silently ignored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<SandboxLimits>,
    /// Outbound network policy
    pub egress: SandboxEgress,
    /// Session lifetime and idle behaviour
    pub session: SandboxSessionPolicy,
    /// Ports eligible for a preview capability. A port not listed here can never be exposed,
    /// so an application cannot widen its own ingress at runtime.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preview_ports: Vec<u16>,
}

/// Whether the artifact being rendered restricts which network modes it accepts.
///
/// A sandbox is not emitted on a Kubernetes target, so nothing there routes egress through a
/// connector and the default network stays a working answer. Every site that withholds the mode,
/// explains the restriction, or renders a branch for it has to ask this one question — asking the
/// stack directly is how they came to disagree.
pub fn restricts_network_mode(stack: &crate::Stack, targets_kubernetes: bool) -> bool {
    !targets_kubernetes && stack_needs_named_subnets_at_setup(stack)
}

/// Whether any sandbox in the stack forces setup to name subnets.
///
/// A restricted sandbox routes session egress through a VPC connector, and neither generator can
/// enumerate the account default VPC's subnets, so that mode leaves the connector without any and
/// it fails at create. Callers that render an artifact want [`restricts_network_mode`] instead:
/// this one answers for the declaration, which on a Kubernetes target is not what gets emitted.
pub fn stack_needs_named_subnets_at_setup(stack: &crate::Stack) -> bool {
    stack.resources().any(|(_resource_id, resource)| {
        resource
            .config
            .downcast_ref::<Sandbox>()
            .is_some_and(|sandbox| !matches!(sandbox.egress, SandboxEgress::Allow))
    })
}

impl Sandbox {
    /// The resource type identifier for Sandbox
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("sandbox");

    /// Returns the sandbox's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The declared ceilings, or the defaults a platform applies when none were named.
    ///
    /// Backends want a concrete set: a sandbox with no declared ceilings still runs inside
    /// whatever the platform gives it, and a backend that had to branch on `None` would end up
    /// inventing its own default anyway.
    pub fn resolved_limits(&self) -> SandboxLimits {
        self.limits.clone().unwrap_or_else(default_limits)
    }

    /// Validates the declaration against what the target platform can enforce.
    ///
    /// Runs at plan time so an unenforceable limit or an unsupported egress mode fails before
    /// anything is provisioned, rather than at the first exec.
    pub fn validate_for_platform(&self, platform: Platform) -> Result<()> {
        let capabilities = SandboxCapabilities::for_platform(platform)?;

        // No backend builds a sandbox image from source. Kubernetes turned this into an empty
        // image string and a pod that could never schedule, which is the silent no-op the
        // capability contract forbids — the failure has to land here instead.
        if let SandboxCode::Source { .. } = &self.code {
            return Err(AlienError::new(ErrorData::SandboxLimitInvalid {
                resource_id: self.id.clone(),
                field: "code".to_string(),
                value: "source".to_string(),
                reason: "no sandbox backend builds an image from source yet; give code.image a \
                         prebuilt reference"
                    .to_string(),
            }));
        }

        let Some(limits) = self.limits.as_ref() else {
            // Nothing declared, so nothing to enforce and nothing to reject.
            return self.validate_capabilities(&capabilities, platform);
        };

        validate_quantity(&self.id, "cpu", &limits.cpu)?;
        validate_quantity(&self.id, "memory", &limits.memory)?;
        validate_quantity(&self.id, "disk", &limits.disk)?;

        if let Some(max_processes) = limits.max_processes {
            if max_processes == 0 {
                return Err(AlienError::new(ErrorData::SandboxLimitInvalid {
                    resource_id: self.id.clone(),
                    field: "maxProcesses".to_string(),
                    value: "0".to_string(),
                    reason: "a sandbox that may run no processes cannot run code".to_string(),
                }));
            }
            capabilities.require(SandboxCapability::ProcessLimit, platform)?;
        }

        // Declaring limits a platform ignores is worse than not declaring them: the stack reads
        // as bounded while the sandbox is not.
        capabilities.require(SandboxCapability::EnforcedLimits, platform)?;

        if platform == Platform::Aws {
            // Refused here rather than at emit so a customer sees it while planning, and so both
            // package formats inherit the same answer.
            self.microvm_tier()?;

            // The ceiling is Lambda's, and it rejects the run rather than clamping — so a value
            // outside it would pass planning, render into the package, and fail at the first
            // session. Kubernetes takes the same field with no such bound, which is why this
            // sits under the AWS gate rather than on the type.
            if let Some(seconds) = self.session.max_lifetime_seconds {
                if !(1..=AWS_MAX_SESSION_LIFETIME_SECONDS).contains(&seconds) {
                    return Err(AlienError::new(ErrorData::SandboxLimitInvalid {
                        resource_id: self.id.clone(),
                        field: "maxLifetimeSeconds".to_string(),
                        value: seconds.to_string(),
                        reason: format!(
                            "AWS runs a MicroVM for between 1 and \
                             {AWS_MAX_SESSION_LIFETIME_SECONDS} seconds"
                        ),
                    }));
                }
            }
        }

        self.validate_capabilities(&capabilities, platform)
    }

    /// The MicroVM size that keeps every declared ceiling, or why none does.
    ///
    /// AWS sizes are discrete and a running MicroVM bursts to four times its baseline, so the
    /// only tier that honours a ceiling is one whose peak fits inside it. A declaration no tier
    /// satisfies is refused: shipping the nearest size would give the customer a sandbox that
    /// exceeds the bound they wrote down.
    pub fn microvm_tier(&self) -> Result<MicrovmTier> {
        let Some(limits) = self.limits.as_ref() else {
            // Nothing declared: AWS's own default baseline, which is also `default_limits`.
            return Ok(MICROVM_TIERS[2]);
        };

        let memory_mib = quantity_mib(&limits.memory).ok_or_else(|| {
            AlienError::new(ErrorData::SandboxLimitInvalid {
                resource_id: self.id.clone(),
                field: "memory".to_string(),
                value: limits.memory.clone(),
                reason: "AWS sizes a MicroVM in whole MiB".to_string(),
            })
        })?;
        let disk_mib = quantity_mib(&limits.disk).ok_or_else(|| {
            AlienError::new(ErrorData::SandboxLimitInvalid {
                resource_id: self.id.clone(),
                field: "disk".to_string(),
                value: limits.disk.clone(),
                reason: "AWS sizes a MicroVM's disk in whole MiB".to_string(),
            })
        })?;
        let cpu_millicores = millicores(&limits.cpu).ok_or_else(|| {
            AlienError::new(ErrorData::SandboxLimitInvalid {
                resource_id: self.id.clone(),
                field: "cpu".to_string(),
                value: limits.cpu.clone(),
                reason: "expected cores or millicores".to_string(),
            })
        })?;

        // Memory and disk choose the size; cpu is then checked rather than used to choose.
        // AWS couples cpu to memory at 2 GB per vCPU, so letting a low cpu ceiling select the
        // size too would quietly hand back a machine four times smaller than the memory ceiling
        // asked for, with nothing to indicate it.
        let sized = |tier: &&MicrovmTier| {
            tier.peak_memory_mib <= memory_mib && tier.max_disk_mib <= disk_mib
        };

        let tier = MICROVM_TIERS
            .iter()
            .rev()
            .find(sized)
            .copied()
            .ok_or_else(|| {
                AlienError::new(ErrorData::SandboxLimitInvalid {
                    resource_id: self.id.clone(),
                    field: "memory".to_string(),
                    value: limits.memory.clone(),
                    reason: format!(
                        "a Lambda MicroVM bursts to four times its baseline, so the smallest \
                         ceiling AWS can hold is 2Gi memory with 8Gi disk; '{}' memory and '{}' \
                         disk fit no size",
                        limits.memory, limits.disk
                    ),
                })
            })?;

        let required_millicores = i64::from(tier.peak_vcpu) * 1000;
        if cpu_millicores < required_millicores {
            return Err(AlienError::new(ErrorData::SandboxLimitInvalid {
                resource_id: self.id.clone(),
                field: "cpu".to_string(),
                value: limits.cpu.clone(),
                reason: format!(
                    "AWS allocates one vCPU per 2GB, so a MicroVM sized to a '{}' memory ceiling \
                     reaches {} vCPU; declare cpu '{}' or lower the memory ceiling",
                    limits.memory, tier.peak_vcpu, tier.peak_vcpu
                ),
            }));
        }

        Ok(tier)
    }

    /// The capability checks that do not depend on declared limits.
    fn validate_capabilities(
        &self,
        capabilities: &SandboxCapabilities,
        platform: Platform,
    ) -> Result<()> {
        if matches!(self.egress, SandboxEgress::AllowDomains { .. }) {
            capabilities.require(SandboxCapability::DomainEgressRules, platform)?;
        }

        // `allow` asks for no restriction, so a backend that ignores it fails loudly on the first
        // blocked connection. `deny` asks for one, and a backend that ignores it puts untrusted
        // code on the internet with nothing to notice — so only this direction is gated.
        if matches!(self.egress, SandboxEgress::Deny) {
            capabilities.require(SandboxCapability::EgressDeny, platform)?;
        }

        if !self.preview_ports.is_empty() {
            capabilities.require(SandboxCapability::Preview, platform)?;
        }

        if self.session.idle_suspend_seconds.is_some() {
            capabilities.require(SandboxCapability::SuspendResume, platform)?;
        }

        if self.session.max_lifetime_seconds.is_some() {
            capabilities.require(SandboxCapability::SessionLifetime, platform)?;
        }

        Ok(())
    }
}

/// Ceilings applied when a declaration names none.
///
/// Modest on purpose: an undeclared sandbox is one whose author did not think about sizing, and
/// the safe reading of that is a small box rather than a generous one.
fn default_limits() -> SandboxLimits {
    SandboxLimits {
        cpu: "1".to_string(),
        memory: "2Gi".to_string(),
        disk: "8Gi".to_string(),
        max_processes: None,
    }
}

/// Validates a Kubernetes-style resource quantity such as `500m`, `2Gi` or `1`.
fn validate_quantity(resource_id: &str, field: &str, value: &str) -> Result<()> {
    let invalid = |reason: &str| {
        AlienError::new(ErrorData::SandboxLimitInvalid {
            resource_id: resource_id.to_string(),
            field: field.to_string(),
            value: value.to_string(),
            reason: reason.to_string(),
        })
    };

    let digits_end = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let (number, suffix) = value.split_at(digits_end);

    let parsed: f64 = number
        .parse()
        .map_err(|_| invalid("expected a number, optionally followed by a unit suffix"))?;

    if parsed <= 0.0 {
        return Err(invalid("must be greater than zero"));
    }

    const SUFFIXES: &[&str] = &["", "m", "k", "M", "G", "T", "Ki", "Mi", "Gi", "Ti"];
    if !SUFFIXES.contains(&suffix) {
        return Err(invalid(
            "unit must be one of m, k, M, G, T, Ki, Mi, Gi, Ti, or absent",
        ));
    }

    Ok(())
}

/// Splits a quantity into its number and unit suffix.
fn split_quantity(value: &str) -> Option<(f64, &str)> {
    let trimmed = value.trim();
    let digits_end = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(trimmed.len());
    let (number, suffix) = trimmed.split_at(digits_end);
    number.parse().ok().map(|number| (number, suffix))
}

/// A memory or disk quantity in whole MiB, rounded down.
///
/// Every suffix `validate_quantity` accepts is handled here. Reading only `Gi` and `Mi` and
/// falling back for the rest would turn a declared `4G` into a different size than the customer
/// asked for, which for a ceiling means a sandbox larger than its bound.
pub fn quantity_mib(value: &str) -> Option<i64> {
    let (number, suffix) = split_quantity(value)?;
    let bytes = match suffix {
        "" => number,
        "k" => number * 1e3,
        "M" => number * 1e6,
        "G" => number * 1e9,
        "T" => number * 1e12,
        "Ki" => number * 1024.0,
        "Mi" => number * 1024.0 * 1024.0,
        "Gi" => number * 1024.0 * 1024.0 * 1024.0,
        "Ti" => number * 1024.0 * 1024.0 * 1024.0 * 1024.0,
        // `m` is a millicore suffix; memory has no use for it.
        _ => return None,
    };
    Some((bytes / (1024.0 * 1024.0)) as i64)
}

/// A CPU quantity in millicores.
pub fn millicores(value: &str) -> Option<i64> {
    let (number, suffix) = split_quantity(value)?;
    match suffix {
        "" => Some((number * 1000.0) as i64),
        "m" => Some(number as i64),
        _ => None,
    }
}

/// Outputs generated by a successfully provisioned Sandbox parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SandboxOutputs {
    /// Name of the durable parent that sessions are created inside
    pub parent_name: String,
    /// Platform-specific identifier for the parent (image ARN, sandbox group id, namespace)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// Data-plane endpoint sessions are created through, where the platform has one
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

impl ResourceOutputsDefinition for SandboxOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Sandbox::RESOURCE_TYPE
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<SandboxOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceDefinition for Sandbox {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        Vec::new()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_sandbox = new_config
            .as_any()
            .downcast_ref::<Sandbox>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        if self.id != new_sandbox.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<Sandbox>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox_with(egress: SandboxEgress, preview_ports: Vec<u16>) -> Sandbox {
        Sandbox::new("agent-sbx".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu:24.04".to_string(),
            })
            .limits(SandboxLimits {
                cpu: "1".to_string(),
                memory: "2Gi".to_string(),
                disk: "20Gi".to_string(),
                max_processes: None,
            })
            .egress(egress)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .preview_ports(preview_ports)
            .build()
    }

    #[test]
    fn resource_type_is_stable() {
        assert_eq!(Sandbox::RESOURCE_TYPE.as_ref(), "sandbox");
    }

    #[test]
    fn capability_sets_are_per_platform() {
        let gcp = SandboxCapabilities::for_platform(Platform::Gcp).expect("gcp is supported");
        assert!(
            !gcp.reconnect,
            "a GCP session id is scoped to one instance, so reconnect is absent"
        );
        assert!(!gcp.preview);
        assert!(!gcp.enforced_limits);

        let azure = SandboxCapabilities::for_platform(Platform::Azure).expect("azure is supported");
        assert!(
            !azure.files,
            "the Azure binding implements no file transfer"
        );
        assert!(gcp.files, "every other backend moves files");
        // The Azure binding renders neither an egress policy nor a ceiling, so a declaration of
        // either is refused rather than accepted and dropped.
        assert!(!azure.domain_egress_rules);
        assert!(!azure.egress_deny);
        assert!(!azure.enforced_limits);
        // Azure the cloud has snapshot, preview and resume; the binding provider returns
        // unsupported for all three. What a caller can reach is what the set describes.
        assert!(!azure.snapshot);
        assert!(!azure.preview);
        assert!(!azure.suspend_resume);

        let aws = SandboxCapabilities::for_platform(Platform::Aws).expect("aws is supported");
        assert!(!aws.snapshot, "AWS has no user-callable session snapshot");
        assert!(aws.suspend_resume);

        let k8s =
            SandboxCapabilities::for_platform(Platform::Kubernetes).expect("k8s is supported");
        assert!(
            !k8s.preview,
            "the session-scoped ingress gateway does not exist yet"
        );
    }

    #[test]
    fn platforms_without_a_backend_are_an_error_not_an_empty_set() {
        let error = SandboxCapabilities::for_platform(Platform::Machines)
            .expect_err("Machines has no sandbox backend");
        assert_eq!(error.code, "SANDBOX_PLATFORM_UNSUPPORTED");
    }

    #[test]
    fn unsupported_capability_names_platform_and_capability() {
        let capabilities = SandboxCapabilities::for_platform(Platform::Gcp).expect("supported");
        let error = capabilities
            .require(SandboxCapability::Preview, Platform::Gcp)
            .expect_err("GCP has no preview");

        assert_eq!(error.code, "SANDBOX_CAPABILITY_UNSUPPORTED");
        let rendered = error.to_string();
        assert!(
            rendered.contains("preview"),
            "names the capability: {rendered}"
        );
        assert!(rendered.contains("gcp"), "names the platform: {rendered}");
    }

    /// No backend expresses a hostname allowlist: AWS and Kubernetes match CIDRs, and the Azure
    /// binding renders no egress policy at all. Accepting one anywhere would leave a stack
    /// reading as restricted while the sandbox reaches the whole internet.
    #[test]
    fn a_hostname_allowlist_is_refused_on_every_backend() {
        let sandbox = sandbox_with(
            SandboxEgress::AllowDomains {
                domains: vec!["example.com".to_string()],
            },
            vec![],
        );

        for platform in [
            Platform::Aws,
            Platform::Azure,
            Platform::Gcp,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            let error = sandbox
                .validate_for_platform(platform)
                .expect_err("no backend expresses a hostname allowlist");
            assert_eq!(
                error.code, "SANDBOX_CAPABILITY_UNSUPPORTED",
                "on {platform:?}"
            );
        }
    }

    /// `deny` is the declaration that carries a security promise, so a backend that cannot keep
    /// it has to refuse rather than accept it and run the code with open egress.
    #[test]
    fn a_denied_egress_is_refused_where_it_would_not_be_enforced() {
        let sandbox = sandbox_with(SandboxEgress::Deny, vec![]);

        // GCP is asserted at the capability rather than through validation: this sandbox declares
        // ceilings GCP cannot enforce, so it is refused for a reason unrelated to egress.
        assert!(
            SandboxCapabilities::for_platform(Platform::Gcp)
                .expect("supported")
                .egress_deny
        );

        for platform in [Platform::Aws, Platform::Kubernetes, Platform::Local] {
            sandbox
                .validate_for_platform(platform)
                .expect("deny is enforced here");
        }

        // Declares no ceilings, so the only thing left for Azure to refuse is the egress mode.
        let egress_only = Sandbox::new("sbx".to_string())
            .code(SandboxCode::Image {
                image: "alpine:3.20".to_string(),
            })
            .egress(SandboxEgress::Deny)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .build();

        let error = egress_only
            .validate_for_platform(Platform::Azure)
            .expect_err("the Azure binding renders no egress policy, so deny cannot be kept");
        assert_eq!(error.code, "SANDBOX_CAPABILITY_UNSUPPORTED");
        assert!(
            error.message.contains("egressDeny"),
            "names the capability: {}",
            error.message
        );
    }

    /// Ceilings are rejected per-platform where unsupported — rejected when *declared*. With
    /// limits mandatory that would read as "GCP sandboxes cannot exist", contradicting the
    /// create, exec, files and terminate GCP does support.
    #[test]
    fn a_platform_that_cannot_enforce_limits_still_takes_a_sandbox_without_them() {
        let declared = sandbox_with(SandboxEgress::Deny, Vec::new());
        declared
            .validate_for_platform(Platform::Gcp)
            .expect_err("declaring ceilings GCP cannot enforce is rejected");

        let undeclared = Sandbox::new("sbx".to_string())
            .code(SandboxCode::Image {
                image: "alpine:3.20".to_string(),
            })
            .egress(SandboxEgress::Deny)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .build();

        undeclared
            .validate_for_platform(Platform::Gcp)
            .expect("a sandbox naming no ceilings takes the platform's own");

        // A backend still gets a concrete set, so nothing downstream has to invent one.
        assert_eq!(undeclared.resolved_limits().cpu, "1");
    }

    #[test]
    fn preview_ports_require_the_preview_capability() {
        let sandbox = sandbox_with(SandboxEgress::Deny, vec![8080]);

        sandbox
            .validate_for_platform(Platform::Aws)
            .expect("AWS mints a port-scoped JWE");

        let error = sandbox
            .validate_for_platform(Platform::Kubernetes)
            .expect_err("Kubernetes preview is deferred");
        assert_eq!(error.code, "SANDBOX_CAPABILITY_UNSUPPORTED");
    }

    #[test]
    fn gcp_rejects_a_sandbox_declaring_enforced_limits() {
        let sandbox = sandbox_with(SandboxEgress::Allow, vec![]);
        let error = sandbox
            .validate_for_platform(Platform::Gcp)
            .expect_err("GCP cannot enforce ceilings on a subprocess sandbox");
        assert_eq!(error.code, "SANDBOX_CAPABILITY_UNSUPPORTED");
    }

    #[test]
    fn invalid_quantities_are_rejected_with_the_offending_field() {
        let mut sandbox = sandbox_with(SandboxEgress::Deny, vec![]);
        sandbox
            .limits
            .as_mut()
            .expect("the fixture declares limits")
            .memory = "2Gb".to_string();

        let error = sandbox
            .validate_for_platform(Platform::Aws)
            .expect_err("Gb is not a valid suffix");
        assert_eq!(error.code, "SANDBOX_LIMIT_INVALID");
        assert!(error.to_string().contains("memory"));

        sandbox
            .limits
            .as_mut()
            .expect("the fixture declares limits")
            .memory = "2Gi".to_string();
        sandbox
            .limits
            .as_mut()
            .expect("the fixture declares limits")
            .cpu = "0".to_string();
        let error = sandbox
            .validate_for_platform(Platform::Aws)
            .expect_err("zero cpu is not a ceiling");
        assert_eq!(error.code, "SANDBOX_LIMIT_INVALID");
    }

    #[test]
    fn zero_max_processes_is_rejected() {
        let mut sandbox = sandbox_with(SandboxEgress::Deny, vec![]);
        sandbox
            .limits
            .as_mut()
            .expect("the fixture declares limits")
            .max_processes = Some(0);

        let error = sandbox
            .validate_for_platform(Platform::Local)
            .expect_err("a sandbox must be able to run at least one process");
        assert_eq!(error.code, "SANDBOX_LIMIT_INVALID");
        assert!(error.to_string().contains("maxProcesses"));
    }

    /// A process ceiling needs a container runtime. Kubernetes sets one per node rather than per
    /// pod, and neither MicroVMs nor Azure sandboxes expose one, so accepting the declaration
    /// anywhere else would mean carrying a bound nothing applies.
    #[test]
    fn a_process_ceiling_is_accepted_only_where_a_runtime_can_apply_it() {
        let mut sandbox = sandbox_with(SandboxEgress::Deny, vec![]);
        sandbox
            .limits
            .as_mut()
            .expect("the fixture declares limits")
            .max_processes = Some(256);

        sandbox
            .validate_for_platform(Platform::Local)
            .expect("Docker takes a pids limit");

        for platform in [Platform::Aws, Platform::Azure, Platform::Kubernetes] {
            let error = sandbox
                .validate_for_platform(platform)
                .expect_err("a process ceiling nothing applies must be refused");
            assert_eq!(error.code, "SANDBOX_CAPABILITY_UNSUPPORTED");
        }
    }

    /// Lambda rejects a run outside 1–28,800 rather than clamping it, so a value beyond that
    /// would pass planning, render into the package, and fail at the first session. Kubernetes
    /// takes the same field with no such bound, so the check is AWS's alone.
    #[test]
    fn a_lifetime_aws_would_reject_is_refused_while_planning() {
        let mut sandbox = sandbox_with(SandboxEgress::Deny, vec![]);

        for seconds in [0, 28_801, 100_000] {
            sandbox.session.max_lifetime_seconds = Some(seconds);
            let error = sandbox
                .validate_for_platform(Platform::Aws)
                .expect_err("a lifetime outside what AWS runs is refused");
            assert_eq!(error.code, "SANDBOX_LIMIT_INVALID", "{seconds}s");

            // Kubernetes has no such ceiling, so the same declaration is fine there.
            sandbox
                .validate_for_platform(Platform::Kubernetes)
                .expect("the kubelet takes any activeDeadlineSeconds");
        }

        sandbox.session.max_lifetime_seconds = Some(28_800);
        sandbox
            .validate_for_platform(Platform::Aws)
            .expect("the ceiling itself is allowed");
    }

    /// A deadline is accepted only where the platform itself terminates on it — the kubelet's
    /// `activeDeadlineSeconds` and Lambda's `maximumDurationInSeconds`. Everywhere else it would
    /// need a reaper that does not exist, so it is refused rather than accepted and dropped.
    #[test]
    fn a_session_deadline_is_accepted_only_where_the_platform_applies_it() {
        let mut sandbox = sandbox_with(SandboxEgress::Deny, vec![]);
        sandbox.session.max_lifetime_seconds = Some(3600);

        sandbox
            .validate_for_platform(Platform::Kubernetes)
            .expect("the kubelet enforces activeDeadlineSeconds");
        sandbox
            .validate_for_platform(Platform::Aws)
            .expect("Lambda terminates the MicroVM at maximumDurationInSeconds");

        for platform in [Platform::Azure, Platform::Local] {
            let error = sandbox
                .validate_for_platform(platform)
                .expect_err("a deadline nothing applies must be refused");
            assert_eq!(error.code, "SANDBOX_CAPABILITY_UNSUPPORTED");
        }
    }

    /// A MicroVM bursts to four times its baseline with no way to opt out, so a ceiling is kept
    /// by choosing the size whose *peak* fits inside it. Sizing by baseline would hand back a
    /// sandbox that can reach four times what the customer declared.
    #[test]
    fn an_aws_size_is_chosen_so_its_peak_stays_inside_the_declared_ceiling() {
        let sandbox = sandbox_with(SandboxEgress::Deny, vec![]);
        let tier = sandbox
            .microvm_tier()
            .expect("2Gi/1cpu/20Gi is satisfiable");

        assert_eq!(
            tier.peak_memory_mib, 2048,
            "the peak is the declared ceiling"
        );
        assert_eq!(
            tier.baseline_memory_mib, 512,
            "which is a quarter of it as the baseline"
        );
        assert!(tier.max_disk_mib <= 20 * 1024);
    }

    /// AWS allocates one vCPU per 2GB, so a cpu ceiling below what the memory ceiling implies
    /// cannot be honoured together with it. Letting cpu choose the size instead would hand back a
    /// machine four times smaller than the memory asked for, with nothing to indicate it.
    #[test]
    fn a_cpu_ceiling_below_what_the_memory_implies_is_refused_not_quietly_downsized() {
        let mut sandbox = sandbox_with(SandboxEgress::Deny, vec![]);
        {
            let limits = sandbox
                .limits
                .as_mut()
                .expect("the fixture declares limits");
            limits.cpu = "1".to_string();
            limits.memory = "8Gi".to_string();
        }

        let error = sandbox
            .microvm_tier()
            .expect_err("1 cpu and 8Gi cannot both be ceilings on AWS");
        assert!(
            error.to_string().contains("4 vCPU"),
            "the refusal must say what the memory ceiling implies: {error}"
        );

        sandbox
            .limits
            .as_mut()
            .expect("the fixture declares limits")
            .cpu = "4".to_string();
        let tier = sandbox.microvm_tier().expect("4 cpu matches 8Gi");
        assert_eq!(tier.peak_memory_mib, 8192);
    }

    /// Below AWS's smallest peak there is no size that holds the ceiling, and rounding up to the
    /// nearest one would silently exceed it.
    #[test]
    fn an_aws_ceiling_smaller_than_any_size_is_refused_rather_than_rounded() {
        let mut sandbox = sandbox_with(SandboxEgress::Deny, vec![]);
        sandbox
            .limits
            .as_mut()
            .expect("the fixture declares limits")
            .memory = "1Gi".to_string();

        let error = sandbox
            .validate_for_platform(Platform::Aws)
            .expect_err("no MicroVM size peaks at or below 1Gi");
        assert_eq!(error.code, "SANDBOX_LIMIT_INVALID");
        assert!(
            error.to_string().contains("2Gi"),
            "the refusal must say what the smallest holdable ceiling is: {error}"
        );
    }

    /// `Source` is a public part of the type that no backend builds. Kubernetes used to turn it
    /// into an empty image string, producing a pod that could never schedule — the refusal has to
    /// happen at plan time and on every platform, not in one emitter.
    #[test]
    fn source_code_is_refused_everywhere_rather_than_producing_a_broken_manifest() {
        let sandbox = Sandbox::new("agent".to_string())
            .code(SandboxCode::Source {
                src: "./sandbox".to_string(),
                toolchain: ToolchainConfig::Docker {
                    dockerfile: None,
                    build_args: None,
                    target: None,
                },
            })
            .egress(SandboxEgress::Deny)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .build();

        for platform in [
            Platform::Aws,
            Platform::Azure,
            Platform::Gcp,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            let error = sandbox
                .validate_for_platform(platform)
                .expect_err("no backend builds a sandbox image from source");
            assert_eq!(error.code, "SANDBOX_LIMIT_INVALID");
            assert!(
                error.to_string().contains("code.image"),
                "the refusal must say what to write instead: {error}"
            );
        }
    }

    /// `validate_quantity` accepts nine suffixes. Reading only `Gi` and `Mi` would size a
    /// declared `4G` as though it were `4Gi`, which for a ceiling means exceeding it.
    #[test]
    fn every_accepted_unit_converts_rather_than_falling_back() {
        assert_eq!(quantity_mib("2Gi"), Some(2048));
        assert_eq!(quantity_mib("512Mi"), Some(512));
        assert_eq!(quantity_mib("4G"), Some(3814));
        assert_eq!(quantity_mib("1Ti"), Some(1024 * 1024));
        assert_eq!(millicores("1"), Some(1000));
        assert_eq!(millicores("500m"), Some(500));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = r#"{
            "id": "sbx",
            "code": {"type": "image", "image": "ubuntu:24.04"},
            "limits": {"cpu": "1", "memory": "2Gi", "disk": "20Gi"},
            "egress": {"mode": "deny"},
            "session": {},
            "unexpected": true
        }"#;

        serde_json::from_str::<Sandbox>(json).expect_err("deny_unknown_fields must reject");
    }

    #[test]
    fn serialization_roundtrips() {
        let sandbox = sandbox_with(
            SandboxEgress::AllowDomains {
                domains: vec!["example.com".to_string()],
            },
            vec![8080, 9090],
        );

        let json = serde_json::to_string(&sandbox).expect("serializes");
        let restored: Sandbox = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(sandbox, restored);
    }

    #[test]
    fn id_is_immutable_across_updates() {
        let original = sandbox_with(SandboxEgress::Deny, vec![]);
        let renamed = Sandbox::new("other".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu:24.04".to_string(),
            })
            .limits(
                original
                    .limits
                    .clone()
                    .expect("the fixture declares limits"),
            )
            .egress(SandboxEgress::Deny)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .build();

        original
            .validate_update(&original.clone())
            .expect("an unchanged config is a valid update");
        original
            .validate_update(&renamed)
            .expect_err("renaming a sandbox is not an update");
    }
}
