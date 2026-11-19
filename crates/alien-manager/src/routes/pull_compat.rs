//! What an older Operator can read in a release.
//!
//! An Operator rejects the whole target when it holds a setting, a variant or a
//! resource type its parser does not know, and then never takes the release,
//! not even its own update. So every setting added after the oldest supported
//! Operator is listed in [`FEATURES`] with the version that first reads it and
//! what to do for older Operators:
//!
//! * **Strip**: remove it. Only when an older Operator then behaves exactly as
//!   before the setting existed and nothing the developer declared is weakened
//!   (setup-time or management-only settings, credentials that the Operator
//!   gets from the same place as before).
//! * **Refuse**: nothing is applied and the deployment keeps its current
//!   release. The deployment's update error says which feature the release
//!   uses and which Operator version reads it. Never omit a resource.
//!
//! The Operator's reported version decides. An Operator that reports none is
//! treated as 3.3.31, the oldest supported. A version that does not parse (a
//! development build) is treated as current.
//!
//! To add a feature: add a [`Feature`] with the first release that reads it
//! (the current [`NEXT_RELEASE`] value, written as a literal), detect it in
//! [`apply`], and add that version's definitions to the older-shape fixture in
//! `tests`.

use alien_core::{
    Ai, ArtifactRegistry, Container, Daemon, Postgres, Sandbox, Stack, Storage, Worker,
};

/// The oldest supported Operator, assumed when none reports its version.
const OLDEST_OPERATOR: &str = "3.3.31";

/// The release that first ships features that are on main but in no tag: the
/// version after the newest release tag. The release prepare step bumps it
/// (`.github/scripts/bump-next-release.sh`); CI fails when it is not newer than
/// the latest tag. Feature gates always carry a literal version: for a feature
/// not yet released, write this value, never the constant itself.
pub(crate) const NEXT_RELEASE: &str = "3.3.70";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Strip,
    Refuse,
}

/// A setting, variant or resource type that older Operators cannot read.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Feature {
    /// Plain-language name used in the refusal message.
    pub name: &'static str,
    /// First Operator version that reads it.
    pub since: &'static str,
    pub action: Action,
    /// Why this action is safe or required.
    pub why: &'static str,
}

const fn feature(
    name: &'static str,
    since: &'static str,
    action: Action,
    why: &'static str,
) -> Feature {
    Feature {
        name,
        since,
        action,
        why,
    }
}

pub(crate) const COMMANDS_CAPABILITY_FILE: Feature = feature(
    "command capability files",
    "3.3.63",
    Action::Strip,
    "the receiver gets its commands key from the manager, as older Operators always did",
);
pub(crate) const RUNTIME_IDENTITY_EXCHANGE: Feature = feature(
    "runtime identity exchange",
    "3.3.63",
    Action::Strip,
    "a worker keeps the runtime credentials it read before identity exchange",
);
pub(crate) const RESOURCE_CHOICES: Feature = feature(
    "container resource choices",
    "3.3.32",
    Action::Strip,
    "the choice is resolved at setup into the deployment's settings; the fixed sizes stay in the stack",
);
pub(crate) const REPLICATION_REGIONS_INPUT: Feature = feature(
    "registry replication inputs",
    "3.3.65",
    Action::Strip,
    "setup reads the input; the pull Operator never renders replication",
);
pub(crate) const AI_MODELS: Feature = feature(
    "AI model checks",
    "3.3.61",
    Action::Strip,
    "an availability check that management runs at setup; the Operator does not read it",
);
/// Not a setting of its own: pod security settings an Operator from before
/// pod security profiles (unreleased) cannot read, which delivery removes. Used only to refuse a signed
/// release, whose stack is never altered.
pub(crate) const UNREADABLE_SECURITY: Feature = feature(
    "pod security settings",
    "3.3.65",
    Action::Strip,
    "older Operators apply no pod security context; delivery removes the settings",
);
pub(crate) const PRIORITY: Feature = feature(
    "container priority",
    "3.3.61",
    Action::Refuse,
    "a scheduling guarantee (preemption) the developer declared",
);
pub(crate) const EGRESS: Feature = feature(
    "container egress rules",
    "3.3.35",
    Action::Refuse,
    "a restriction the Operator would not apply",
);
pub(crate) const WRITABLE_PATHS: Feature = feature(
    "writable paths for restricted containers",
    "3.3.65",
    Action::Refuse,
    "an older Operator would mount a read-only root without them",
);
pub(crate) const DAEMON_SECURITY: Feature = feature(
    "restricted security settings for daemons",
    "3.3.65",
    Action::Refuse,
    "a declared restriction; older Operators cannot apply it",
);
pub(crate) const WORKER_SECURITY: Feature = feature(
    "restricted security settings for workers",
    "3.3.65",
    Action::Refuse,
    "a declared restriction; older Operators cannot apply it",
);
pub(crate) const DATABASE_ENCRYPTION_KEY: Feature = feature(
    "database encryption keys",
    "3.3.33",
    Action::Refuse,
    "an encryption guarantee",
);
pub(crate) const STORAGE_NOTIFICATIONS: Feature = feature(
    "storage notifications",
    "3.3.47",
    Action::Refuse,
    "declared behavior an older Operator would silently not provide",
);
pub(crate) const STORAGE_UPLOAD_ORIGINS: Feature = feature(
    "storage upload origins",
    "3.3.59",
    Action::Refuse,
    "declared behavior an older Operator would silently not provide",
);
pub(crate) const SANDBOX_POLICY: Feature = feature(
    "sandbox links and egress policies",
    "3.3.35",
    Action::Refuse,
    "network policy depends on them",
);
pub(crate) const SANDBOX_IMAGE_USER: Feature = feature(
    "sandbox image users",
    "3.3.41",
    Action::Refuse,
    "the sandbox would run as another user",
);
pub(crate) const SANDBOX_IMAGE_PERMISSIONS: Feature = feature(
    "sandbox image permissions and lifecycle steps",
    "3.3.70",
    Action::Refuse,
    "mounts, direct connections and pause or resume steps the developer declared; older Operators cannot parse them",
);
pub(crate) const REDIS: Feature = feature(
    "Redis resources",
    "3.3.59",
    Action::Refuse,
    "older Operators cannot parse the resource type",
);
pub(crate) const SIGNING_VERIFIER: Feature = feature(
    "signing verifier resources",
    "3.3.65",
    Action::Refuse,
    "older Operators cannot parse the resource type",
);
pub(crate) const AZURE_SANDBOX_INTERNET: Feature = feature(
    "internet egress for Azure sandboxes",
    "3.3.69",
    Action::Refuse,
    "the setup-created gateway's binding carries replica addresses and a protocol floor older Operators reject, and they have no Azure gateway to enforce the rules",
);
pub(crate) const AMBIENT_AI_BINDING: Feature = feature(
    "external AI bindings that use the workload's cloud identity (Bedrock)",
    "3.3.69",
    Action::Refuse,
    "older Operators read an external AI binding only as a provider and an API key, so one without them fails to parse and they reject the whole target",
);
pub(crate) const EXTENSIONS: Feature = feature(
    "extensions",
    "3.3.70",
    Action::Refuse,
    "older Operators cannot load an extension or parse the resource types it adds",
);
pub(crate) const CUSTOM_BASE_DOMAIN: Feature = feature(
    "custom base domains for Kubernetes endpoints",
    "3.3.44",
    Action::Refuse,
    "older Operators cannot parse the exposure mode",
);

/// Every feature, for the documentation test.
pub(crate) const FEATURES: &[&Feature] = &[
    &COMMANDS_CAPABILITY_FILE,
    &RUNTIME_IDENTITY_EXCHANGE,
    &RESOURCE_CHOICES,
    &REPLICATION_REGIONS_INPUT,
    &AI_MODELS,
    &UNREADABLE_SECURITY,
    &PRIORITY,
    &EGRESS,
    &WRITABLE_PATHS,
    &DAEMON_SECURITY,
    &WORKER_SECURITY,
    &DATABASE_ENCRYPTION_KEY,
    &STORAGE_NOTIFICATIONS,
    &STORAGE_UPLOAD_ORIGINS,
    &SANDBOX_POLICY,
    &SANDBOX_IMAGE_USER,
    &SANDBOX_IMAGE_PERMISSIONS,
    &REDIS,
    &SIGNING_VERIFIER,
    &AZURE_SANDBOX_INTERNET,
    &AMBIENT_AI_BINDING,
    &CUSTOM_BASE_DOMAIN,
    &EXTENSIONS,
];

/// The Operator version a sync reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperatorVersion {
    Known(semver::Version),
    /// A build that reports a version we cannot order: assumed to read
    /// everything this manager sends.
    Current,
}

impl OperatorVersion {
    pub(crate) fn from_report(reported: Option<&str>) -> Self {
        match reported {
            None => Self::Known(semver::Version::parse(OLDEST_OPERATOR).expect("constant")),
            Some(version) => semver::Version::parse(version.trim_start_matches('v'))
                .map(Self::Known)
                .unwrap_or(Self::Current),
        }
    }

    /// Whether an Operator at this version reads a feature first shipped in `since`.
    pub(crate) fn reads(&self, feature: &Feature) -> bool {
        match self {
            Self::Current => true,
            Self::Known(version) => semver::Version::parse(feature.since)
                .map(|since| {
                    // Pre-release builds of a release read what that release ships.
                    let mut base = version.clone();
                    base.pre = semver::Prerelease::EMPTY;
                    base >= since
                })
                .unwrap_or(false),
        }
    }
}

/// Removes what older Operators do without and returns what they cannot: the
/// features `operator` does not read that the release uses and that must be
/// refused. The stack is changed only by strips.
///
/// A `signed` release is never altered: where a strip would change it, the
/// feature is refused instead, so a signature the Operator verifies always
/// covers the stack it receives. (No released Operator verifies signatures;
/// the ones that do read everything listed here, so nothing is stripped for
/// them either way.)
///
/// A container's resource choices are first resolved against the deployment's
/// selection into the fixed cpu and memory an older Operator reads; a
/// selection that cannot be expressed that way is refused.
pub(crate) fn apply(
    stack: &mut Stack,
    settings: Option<&alien_core::StackSettings>,
    operator: &OperatorVersion,
    signed: bool,
) -> Vec<&'static Feature> {
    let mut refused: Vec<&'static Feature> = Vec::new();
    let mut refuse = |feature: &'static Feature| {
        if !operator.reads(feature) && !refused.contains(&feature) {
            refused.push(feature);
        }
    };
    if !stack.extensions.is_empty() {
        refuse(&EXTENSIONS);
    }
    let has_choices = stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<Container>()
            .is_some_and(|container| container.resource_choices.is_some())
    });
    if has_choices && !operator.reads(&RESOURCE_CHOICES) {
        if signed {
            refuse(&RESOURCE_CHOICES);
        } else {
            let mut resolved = stack.clone();
            match alien_core::container_resources::resolve_container_resources(
                &mut resolved,
                settings.and_then(|settings| settings.compute.as_ref()),
            ) {
                Ok(()) => *stack = resolved,
                Err(_) => refuse(&RESOURCE_CHOICES),
            }
        }
    }
    for (_, entry) in stack.resources_mut() {
        let config = &mut entry.config;
        if let Some(container) = config.downcast_mut::<Container>() {
            if !operator.reads(&COMMANDS_CAPABILITY_FILE) && container.commands_capability_file {
                if signed {
                    refuse(&COMMANDS_CAPABILITY_FILE);
                } else {
                    container.commands_capability_file = false;
                }
            }
            if !operator.reads(&RESOURCE_CHOICES) && !signed {
                container.resource_choices = None;
            }
            if container.priority.is_some() {
                refuse(&PRIORITY);
            }
            if container.egress.is_some() {
                refuse(&EGRESS);
            }
            if container
                .security
                .as_ref()
                .is_some_and(|security| !security.writable_paths.is_empty())
            {
                refuse(&WRITABLE_PATHS);
            }
        } else if let Some(daemon) = config.downcast_mut::<Daemon>() {
            if !operator.reads(&COMMANDS_CAPABILITY_FILE) && daemon.commands_capability_file {
                if signed {
                    refuse(&COMMANDS_CAPABILITY_FILE);
                } else {
                    daemon.commands_capability_file = false;
                }
            }
            if daemon.priority.is_some() {
                refuse(&PRIORITY);
            }
            if restricted(daemon.security.as_ref()) {
                refuse(&DAEMON_SECURITY);
            }
        } else if let Some(worker) = config.downcast_mut::<Worker>() {
            if !operator.reads(&RUNTIME_IDENTITY_EXCHANGE) && worker.runtime_identity_exchange {
                if signed {
                    refuse(&RUNTIME_IDENTITY_EXCHANGE);
                } else {
                    worker.runtime_identity_exchange = false;
                }
            }
            if restricted(worker.security.as_ref()) {
                refuse(&WORKER_SECURITY);
            }
        } else if let Some(registry) = config.downcast_mut::<ArtifactRegistry>() {
            if !operator.reads(&REPLICATION_REGIONS_INPUT)
                && registry.replication_regions_input.is_some()
            {
                if signed {
                    refuse(&REPLICATION_REGIONS_INPUT);
                } else {
                    registry.replication_regions_input = None;
                }
            }
        } else if let Some(ai) = config.downcast_mut::<Ai>() {
            if !operator.reads(&AI_MODELS) && !ai.models.is_empty() {
                if signed {
                    refuse(&AI_MODELS);
                } else {
                    ai.models.clear();
                }
            }
        } else if let Some(postgres) = config.downcast_ref::<Postgres>() {
            if postgres.encryption_key.is_some() {
                refuse(&DATABASE_ENCRYPTION_KEY);
            }
        } else if let Some(storage) = config.downcast_ref::<Storage>() {
            if !storage.notifications.is_empty() {
                refuse(&STORAGE_NOTIFICATIONS);
            }
            if !storage.cors_upload_origins.is_empty() {
                refuse(&STORAGE_UPLOAD_ORIGINS);
            }
        } else if let Some(sandbox) = config.downcast_ref::<Sandbox>() {
            if !sandbox.links.is_empty() || sandbox.default_policy.is_some() {
                refuse(&SANDBOX_POLICY);
            }
            if sandbox.image_user.is_some() {
                refuse(&SANDBOX_IMAGE_USER);
            }
            if sandbox.declares_image_permissions() {
                refuse(&SANDBOX_IMAGE_PERMISSIONS);
            }
        } else if config.resource_type() == alien_core::Redis::RESOURCE_TYPE {
            refuse(&REDIS);
        } else if config.resource_type() == alien_core::SigningVerifier::RESOURCE_TYPE {
            refuse(&SIGNING_VERIFIER);
        }
    }
    if settings
        .and_then(|settings| settings.kubernetes.as_ref())
        .and_then(|kubernetes| kubernetes.exposure.as_ref())
        .is_some_and(|exposure| {
            matches!(
                exposure,
                alien_core::KubernetesExposureSettings::CustomBaseDomain { .. }
            )
        })
    {
        refuse(&CUSTOM_BASE_DOMAIN);
    }
    // External bindings travel in the deployment's settings and its
    // configuration; an Operator that cannot read one rejects the whole target.
    if settings
        .and_then(|settings| settings.external_bindings.as_ref())
        .is_some_and(|bindings| {
            bindings.0.values().any(|binding| {
                matches!(
                    binding,
                    alien_core::ExternalBinding::Ai(alien_core::ExternalAi::Ambient(_))
                )
            })
        })
    {
        refuse(&AMBIENT_AI_BINDING);
    }
    refused
}

/// The refusal for a feature that depends on the platform, which `apply` does
/// not see: an Azure sandbox with `internet` egress gets a gateway binding with
/// replica addresses and a protocol floor (`SandboxEgressReplicas.addresses`,
/// `.min_protocol`). Operators before this release reject that state and have
/// no Azure gateway, so the release is refused for them, never delivered
/// without its enforcement.
pub(crate) fn refuse_for_platform(
    stack: &Stack,
    platform: alien_core::Platform,
    operator: &OperatorVersion,
) -> Option<&'static Feature> {
    let internet = platform == alien_core::Platform::Azure
        && stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Sandbox>()
                .is_some_and(|sandbox| sandbox.egress == alien_core::SandboxEgress::Internet)
        });
    (internet && !operator.reads(&AZURE_SANDBOX_INTERNET)).then_some(&AZURE_SANDBOX_INTERNET)
}

/// Whether a declared `security` setting is one an older Operator would have
/// to apply. `unrestricted` is the opt-out; older Operators apply no context,
/// so it is dropped with the other pod security settings.
fn restricted(security: Option<&alien_core::ContainerSecurity>) -> bool {
    security.is_some_and(|security| {
        security.profile != alien_core::ContainerSecurityProfile::Unrestricted
    })
}

/// The refusal text saved on the deployment.
pub(crate) fn refusal_message(refused: &[&Feature]) -> String {
    let newest = refused
        .iter()
        .filter_map(|feature| semver::Version::parse(feature.since).ok())
        .max()
        .map(|version| version.to_string())
        .unwrap_or_else(|| NEXT_RELEASE.to_string());
    let names: Vec<&str> = refused.iter().map(|feature| feature.name).collect();
    format!(
        "This deployment needs {}; the deployment's Operator must be {newest} or newer. Update the Operator, then deploy the release again. The deployment keeps running its current release.",
        names.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        ContainerCode, ContainerSecurity, ContainerSecurityProfile, DaemonCode, ResourceLifecycle,
        ResourceSpec,
    };

    fn container() -> Container {
        Container::new("c".to_string())
            .code(ContainerCode::Image {
                image: "img".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("p".to_string())
            .build()
    }

    fn daemon(security: Option<ContainerSecurity>) -> Daemon {
        let mut daemon = Daemon::new("d".to_string())
            .permissions("p".to_string())
            .code(DaemonCode::Image {
                image: "img".to_string(),
            })
            .build();
        daemon.security = security;
        daemon
    }

    fn stack_with(container: Container, daemon: Daemon) -> Stack {
        Stack::new("app".to_string())
            .add(container, ResourceLifecycle::Live)
            .add(daemon, ResourceLifecycle::Live)
            .build()
    }

    fn refused_for(stack: &Stack, version: Option<&str>) -> Vec<&'static str> {
        apply(
            &mut stack.clone(),
            None,
            &OperatorVersion::from_report(version),
            false,
        )
        .into_iter()
        .map(|feature| feature.name)
        .collect()
    }

    fn restricted_security(writable_paths: Vec<String>) -> ContainerSecurity {
        ContainerSecurity {
            profile: ContainerSecurityProfile::Restricted,
            run_as_user: Some(1000),
            run_as_group: Some(1000),
            read_only_root_filesystem: Some(true),
            writable_paths,
        }
    }

    #[test]
    fn every_feature_is_documented_and_has_a_release() {
        for feature in FEATURES {
            assert!(!feature.why.is_empty(), "{}", feature.name);
            assert!(
                semver::Version::parse(feature.since).is_ok(),
                "{}",
                feature.name
            );
        }
    }

    #[test]
    fn a_release_without_new_settings_is_read_by_every_operator() {
        let stack = stack_with(container(), daemon(None));
        for version in [None, Some("3.3.31"), Some("3.3.62"), Some("dev")] {
            assert!(refused_for(&stack, version).is_empty());
        }
    }

    #[test]
    fn a_release_that_lists_an_extension_needs_the_operator_that_loads_extensions() {
        let mut stack = stack_with(container(), daemon(None));
        stack.extensions.push(alien_core::ExtensionManifest {
            name: "acme".to_string(),
            version: "1.0.0".to_string(),
            digest: "sha256:0".to_string(),
            resource_types: Default::default(),
            egress: Vec::new(),
        });
        for version in [
            None,
            Some("3.3.31"),
            Some("3.3.62"),
            Some("3.3.65"),
            Some("3.3.67"),
            Some("3.3.68"),
            Some("3.3.69"),
        ] {
            assert_eq!(refused_for(&stack, version), ["extensions"]);
        }
        assert!(refused_for(&stack, Some(NEXT_RELEASE)).is_empty());
        assert!(refused_for(&stack, Some("dev")).is_empty());
    }

    #[test]
    fn sandbox_image_permissions_are_refused_for_operators_that_cannot_parse_them() {
        let declared =
            |lifecycle: alien_core::SandboxLifecyclePolicy,
             permissions: Option<alien_core::SandboxImagePermissions>| {
                let mut sandbox = Sandbox::new("agents".to_string())
                    .code(alien_core::SandboxCode::Image {
                        image: "s3://bundles/agents.zip".to_string(),
                        bundled_from: None,
                    })
                    .egress(alien_core::SandboxEgress::None)
                    .lifecycle(lifecycle)
                    .build();
                sandbox.image_permissions = permissions;
                Stack::new("app".to_string())
                    .add(sandbox, alien_core::ResourceLifecycle::Live)
                    .build()
            };
        let mounts = declared(
            Default::default(),
            Some(alien_core::SandboxImagePermissions {
                mounts: vec![alien_core::SandboxImageMount {
                    mount_type: alien_core::SandboxImageMountType::Tmpfs,
                    path: "/scratch".into(),
                    size: Some("1Gi".into()),
                    from: None,
                }],
                connections: vec![],
            }),
        );
        let steps = declared(
            alien_core::SandboxLifecyclePolicy {
                on_resume: Some(vec!["/bin/true".into()]),
                ..Default::default()
            },
            None,
        );
        for stack in [&mounts, &steps] {
            for version in [None, Some("3.3.62"), Some("3.3.69")] {
                assert_eq!(
                    refused_for(stack, version),
                    ["sandbox image permissions and lifecycle steps"]
                );
            }
            assert!(refused_for(stack, Some(NEXT_RELEASE)).is_empty());
        }
        assert!(refused_for(&declared(Default::default(), None), None).is_empty());
    }

    #[test]
    fn egress_rules_need_3_3_35() {
        let mut c = container();
        c.egress = Some(alien_core::ContainerEgress::Internet);
        let stack = stack_with(c, daemon(None));
        assert_eq!(refused_for(&stack, None), ["container egress rules"]);
        assert_eq!(
            refused_for(&stack, Some("3.3.31")),
            ["container egress rules"]
        );
        assert!(refused_for(&stack, Some("3.3.62")).is_empty());
    }

    #[test]
    fn a_missing_version_is_the_oldest_and_an_unparsable_one_is_current() {
        let mut c = container();
        c.egress = Some(alien_core::ContainerEgress::Internet);
        let stack = stack_with(c, daemon(None));
        assert_eq!(refused_for(&stack, None).len(), 1);
        for dev in ["dev", "latest", "", "3.x"] {
            assert!(refused_for(&stack, Some(dev)).is_empty(), "{dev}");
        }
        // A pre-release of a release reads what that release ships.
        assert!(refused_for(&stack, Some("3.3.35-rc.1")).is_empty());
    }

    #[test]
    fn declared_priority_is_refused_below_3_3_61() {
        let mut c = container();
        c.priority = Some(alien_core::WorkloadPriority::High);
        let stack = stack_with(c, daemon(None));
        assert_eq!(refused_for(&stack, Some("3.3.60")), ["container priority"]);
        assert!(refused_for(&stack, Some("3.3.61")).is_empty());
    }

    #[test]
    fn restricted_daemon_security_is_refused_and_the_opt_out_is_not() {
        let stack = stack_with(container(), daemon(Some(restricted_security(Vec::new()))));
        assert_eq!(
            refused_for(&stack, Some("3.3.64")),
            ["restricted security settings for daemons"]
        );
        let opt_out = ContainerSecurity {
            profile: ContainerSecurityProfile::Unrestricted,
            run_as_user: None,
            run_as_group: None,
            read_only_root_filesystem: None,
            writable_paths: Vec::new(),
        };
        let stack = stack_with(container(), daemon(Some(opt_out)));
        assert!(refused_for(&stack, Some("3.3.64")).is_empty());
    }

    #[test]
    fn writable_paths_are_refused_for_every_released_operator() {
        let mut c = container();
        c.security = Some(restricted_security(vec!["/cache".to_string()]));
        let stack = stack_with(c, daemon(None));
        assert_eq!(
            refused_for(&stack, Some("3.3.64")),
            ["writable paths for restricted containers"]
        );
    }

    #[test]
    fn stripped_settings_leave_the_stack_readable_by_older_operators() {
        let mut registry = ArtifactRegistry::new("r".to_string()).build();
        registry.replication_regions_input = Some("regions".to_string());
        let mut c = container();
        c.resource_choices = Some(Default::default());
        c.commands_capability_file = true;
        let mut stack = Stack::new("app".to_string())
            .add(c, ResourceLifecycle::Live)
            .add(registry, ResourceLifecycle::Frozen)
            .build();
        let operator = OperatorVersion::from_report(Some("3.3.31"));
        assert!(apply(&mut stack, None, &operator, false).is_empty());
        let text = serde_json::to_string(&stack).unwrap();
        for gone in [
            "resourceChoices",
            "commandsCapabilityFile",
            "replicationRegionsInput",
        ] {
            assert!(!text.contains(gone), "{gone} still sent");
        }
    }

    /// The record of built architectures lives on the stack, which older Operators
    /// read without refusing unknown fields, so no release is stripped or refused for it.
    #[test]
    fn built_architectures_reach_every_operator_unchanged() {
        let mut stack = stack_with(container(), daemon(None));
        stack.built_architectures.insert(
            "c".to_string(),
            vec![alien_core::instance_catalog::Architecture::X86_64],
        );
        let before = serde_json::to_string(&stack).unwrap();
        assert!(before.contains("builtArchitectures"));
        for version in [None, Some("3.3.31"), Some("3.3.64")] {
            for signed in [false, true] {
                let mut stack = stack.clone();
                let operator = OperatorVersion::from_report(version);
                assert!(apply(&mut stack, None, &operator, signed).is_empty());
                assert_eq!(serde_json::to_string(&stack).unwrap(), before);
            }
        }
    }

    fn choices_stack() -> Stack {
        let mut c = container();
        c.resource_choices = Some(alien_core::ContainerResourceChoices {
            cpu: Some(alien_core::ResourceChoiceRange {
                min: "0.5".into(),
                max: "4".into(),
                default: "1".into(),
            }),
            memory: Some(alien_core::ResourceChoiceRange {
                min: "512Mi".into(),
                max: "8Gi".into(),
                default: "1Gi".into(),
            }),
        });
        Stack::new("app".to_string())
            .add(c, ResourceLifecycle::Live)
            .build()
    }

    fn selecting(cpu: serde_json::Number, memory: &str) -> alien_core::StackSettings {
        alien_core::StackSettings {
            compute: Some(alien_core::ComputeSettings {
                containers: [(
                    "c".to_string(),
                    alien_core::ContainerResourceSelection {
                        cpu: Some(cpu),
                        memory: Some(memory.to_string()),
                        ..Default::default()
                    },
                )]
                .into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn sizes(stack: &Stack) -> (String, String) {
        let c = stack
            .resources()
            .next()
            .unwrap()
            .1
            .config
            .downcast_ref::<Container>()
            .unwrap();
        (c.cpu.desired.clone(), c.memory.desired.clone())
    }

    #[test]
    fn a_selected_size_reaches_the_cpu_and_memory_an_older_operator_reads() {
        let mut stack = choices_stack();
        let settings = selecting(serde_json::Number::from(2), "4Gi");
        let operator = OperatorVersion::from_report(Some("3.3.31"));
        assert!(apply(&mut stack, Some(&settings), &operator, false).is_empty());
        assert_eq!(sizes(&stack), ("2".to_string(), "4Gi".to_string()));
        assert!(!serde_json::to_string(&stack)
            .unwrap()
            .contains("resourceChoices"));

        // No selection: the choices' defaults, as the setup would resolve.
        let mut stack = choices_stack();
        assert!(apply(&mut stack, None, &operator, false).is_empty());
        assert_eq!(sizes(&stack), ("1".to_string(), "1Gi".to_string()));

        // An Operator that reads the choices gets them untouched.
        let mut stack = choices_stack();
        let reads = OperatorVersion::from_report(Some("3.3.64"));
        assert!(apply(&mut stack, Some(&settings), &reads, false).is_empty());
        assert!(serde_json::to_string(&stack)
            .unwrap()
            .contains("resourceChoices"));
        assert_eq!(sizes(&stack), ("1".to_string(), "1Gi".to_string()));
    }

    #[test]
    fn a_selection_that_cannot_be_fixed_sizes_is_refused() {
        let mut stack = choices_stack();
        // Outside the allowed range.
        let settings = selecting(serde_json::Number::from(64), "4Gi");
        let operator = OperatorVersion::from_report(Some("3.3.31"));
        let refused = apply(&mut stack, Some(&settings), &operator, false);
        assert_eq!(
            refused.iter().map(|f| f.name).collect::<Vec<_>>(),
            ["container resource choices"]
        );
    }

    #[test]
    fn a_signed_release_is_never_altered() {
        let operator = OperatorVersion::from_report(Some("3.3.31"));
        let mut stack = choices_stack();
        let before = serde_json::to_string(&stack).unwrap();
        let refused = apply(&mut stack, None, &operator, true);
        assert_eq!(
            refused.iter().map(|f| f.name).collect::<Vec<_>>(),
            ["container resource choices"]
        );
        assert_eq!(serde_json::to_string(&stack).unwrap(), before);

        let mut registry = ArtifactRegistry::new("r".to_string()).build();
        registry.replication_regions_input = Some("regions".to_string());
        let stack = Stack::new("app".to_string())
            .add(registry, ResourceLifecycle::Frozen)
            .build();
        for version in ["3.3.31", "3.3.64"] {
            let mut copy = stack.clone();
            let refused = apply(
                &mut copy,
                None,
                &OperatorVersion::from_report(Some(version)),
                true,
            );
            assert_eq!(refused.len(), 1, "{version}");
            assert_eq!(
                serde_json::to_string(&copy).unwrap(),
                serde_json::to_string(&stack).unwrap()
            );
        }
        // Operators that read everything (the ones that verify signatures) strip nothing.
        let mut copy = stack.clone();
        let refused = apply(
            &mut copy,
            None,
            &OperatorVersion::from_report(Some(NEXT_RELEASE)),
            true,
        );
        assert!(refused.is_empty());
        assert_eq!(
            serde_json::to_string(&copy).unwrap(),
            serde_json::to_string(&stack).unwrap()
        );
    }

    fn internet_sandbox() -> Stack {
        let sandbox = Sandbox::new("box".to_string())
            .code(alien_core::SandboxCode::Image {
                image: "img".to_string(),
                bundled_from: None,
            })
            .egress(alien_core::SandboxEgress::Internet)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
                ..Default::default()
            })
            .build();
        Stack::new("app".to_string())
            .add(sandbox, ResourceLifecycle::Live)
            .build()
    }

    #[test]
    fn an_azure_internet_sandbox_is_refused_for_operators_before_its_gateway_binding() {
        use alien_core::Platform;
        let stack = internet_sandbox();
        for version in [None, Some("3.3.31"), Some("3.3.62"), Some("3.3.68")] {
            let operator = OperatorVersion::from_report(version);
            assert_eq!(
                refuse_for_platform(&stack, Platform::Azure, &operator).map(|feature| feature.name),
                Some("internet egress for Azure sandboxes"),
                "{version:?}"
            );
            // AWS and Kubernetes gateways were read by these Operators already.
            assert!(refuse_for_platform(&stack, Platform::Aws, &operator).is_none());
            assert!(refuse_for_platform(&stack, Platform::Kubernetes, &operator).is_none());
        }
        let reads = OperatorVersion::from_report(Some("3.3.69"));
        assert!(refuse_for_platform(&stack, Platform::Azure, &reads).is_none());
        let mut none = stack.clone();
        for (_, entry) in none.resources_mut() {
            if let Some(sandbox) = entry.config.downcast_mut::<Sandbox>() {
                sandbox.egress = alien_core::SandboxEgress::None;
            }
        }
        assert!(
            refuse_for_platform(&none, Platform::Azure, &OperatorVersion::from_report(None))
                .is_none()
        );
    }

    /// The v3.3.68 shape of an external AI binding (`alien-core`
    /// `ExternalBinding::Ai(ExternalAiBinding)`), copied so the failure older
    /// Operators have is reproduced, not assumed.
    #[derive(Debug, serde::Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum OldExternalBinding {
        #[allow(dead_code)]
        Ai(OldExternalAiBinding),
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    #[allow(dead_code)]
    struct OldExternalAiBinding {
        provider: String,
        api_key: serde_json::Value,
    }

    fn bedrock_settings() -> alien_core::StackSettings {
        let binding: alien_core::ExternalBinding = serde_json::from_value(serde_json::json!({
            "type": "ai", "service": "bedrock", "region": "us-east-1"
        }))
        .unwrap();
        alien_core::StackSettings {
            external_bindings: Some(alien_core::ExternalBindings(
                [("llm".to_string(), binding)].into(),
            )),
            ..Default::default()
        }
    }

    #[test]
    fn an_ambient_bedrock_binding_is_refused_for_operators_that_cannot_parse_it() {
        let settings = bedrock_settings();
        // What a 3.3.68 Operator does with the binding this manager would send.
        let sent =
            serde_json::to_value(&settings.external_bindings.as_ref().unwrap().0["llm"]).unwrap();
        let old = serde_json::from_value::<OldExternalBinding>(sent);
        let error = old.unwrap_err().to_string();
        assert!(error.contains("missing field `provider`"), "{error}");

        let stack = stack_with(container(), daemon(None));
        for version in [None, Some("3.3.31"), Some("3.3.65"), Some("3.3.68")] {
            let refused = apply(
                &mut stack.clone(),
                Some(&settings),
                &OperatorVersion::from_report(version),
                false,
            );
            assert_eq!(
                refused.iter().map(|f| f.since).collect::<Vec<_>>(),
                ["3.3.69"],
                "{version:?}"
            );
        }
        assert!(apply(
            &mut stack.clone(),
            Some(&settings),
            &OperatorVersion::from_report(Some("3.3.69")),
            false
        )
        .is_empty());
        // A bring-your-own-key binding was always readable.
        let key: alien_core::ExternalBinding = serde_json::from_value(serde_json::json!({
            "type": "ai", "provider": "openai", "apiKey": "k"
        }))
        .unwrap();
        let settings = alien_core::StackSettings {
            external_bindings: Some(alien_core::ExternalBindings([("llm".into(), key)].into())),
            ..Default::default()
        };
        assert!(apply(
            &mut stack.clone(),
            Some(&settings),
            &OperatorVersion::from_report(None),
            false
        )
        .is_empty());
    }

    #[test]
    fn the_message_names_the_features_and_the_newest_version() {
        let message = refusal_message(&[&STORAGE_NOTIFICATIONS, &REDIS]);
        assert!(message.contains("This deployment needs storage notifications, Redis resources"));
        assert!(message.contains("must be 3.3.59 or newer"));
        assert!(message.contains("keeps running its current release"));
    }
}
