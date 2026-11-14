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
//! ([`NEXT_RELEASE`] until it ships), detect it in [`apply`], and add that
//! version's definitions to the older-shape fixture in `tests`.

use alien_core::{
    Ai, ArtifactRegistry, Container, Daemon, Postgres, Sandbox, Stack, Storage, Worker,
};

/// The oldest supported Operator, assumed when none reports its version.
const OLDEST_OPERATOR: &str = "3.3.31";

/// The release that first ships features that are on main but in no tag. Set it
/// to the version the next release gets.
pub(crate) const NEXT_RELEASE: &str = "3.3.65";

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
    NEXT_RELEASE,
    Action::Strip,
    "setup reads the input; the pull Operator never renders replication",
);
pub(crate) const AI_MODELS: Feature = feature(
    "AI model checks",
    "3.3.61",
    Action::Strip,
    "an availability check that management runs at setup; the Operator does not read it",
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
    NEXT_RELEASE,
    Action::Refuse,
    "an older Operator would mount a read-only root without them",
);
pub(crate) const DAEMON_SECURITY: Feature = feature(
    "restricted security settings for daemons",
    NEXT_RELEASE,
    Action::Refuse,
    "a declared restriction; older Operators cannot apply it",
);
pub(crate) const WORKER_SECURITY: Feature = feature(
    "restricted security settings for workers",
    NEXT_RELEASE,
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
pub(crate) const REDIS: Feature = feature(
    "Redis resources",
    "3.3.59",
    Action::Refuse,
    "older Operators cannot parse the resource type",
);
pub(crate) const SIGNING_VERIFIER: Feature = feature(
    "signing verifier resources",
    NEXT_RELEASE,
    Action::Refuse,
    "older Operators cannot parse the resource type",
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
    &REDIS,
    &SIGNING_VERIFIER,
    &CUSTOM_BASE_DOMAIN,
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
pub(crate) fn apply(
    stack: &mut Stack,
    settings: Option<&alien_core::StackSettings>,
    operator: &OperatorVersion,
) -> Vec<&'static Feature> {
    let mut refused: Vec<&'static Feature> = Vec::new();
    let mut refuse = |feature: &'static Feature| {
        if !operator.reads(feature) && !refused.contains(&feature) {
            refused.push(feature);
        }
    };
    for (_, entry) in stack.resources_mut() {
        let config = &mut entry.config;
        if let Some(container) = config.downcast_mut::<Container>() {
            if !operator.reads(&COMMANDS_CAPABILITY_FILE) {
                container.commands_capability_file = false;
            }
            if !operator.reads(&RESOURCE_CHOICES) {
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
            if !operator.reads(&COMMANDS_CAPABILITY_FILE) {
                daemon.commands_capability_file = false;
            }
            if daemon.priority.is_some() {
                refuse(&PRIORITY);
            }
            if restricted(daemon.security.as_ref()) {
                refuse(&DAEMON_SECURITY);
            }
        } else if let Some(worker) = config.downcast_mut::<Worker>() {
            if !operator.reads(&RUNTIME_IDENTITY_EXCHANGE) {
                worker.runtime_identity_exchange = false;
            }
            if restricted(worker.security.as_ref()) {
                refuse(&WORKER_SECURITY);
            }
        } else if let Some(registry) = config.downcast_mut::<ArtifactRegistry>() {
            if !operator.reads(&REPLICATION_REGIONS_INPUT) {
                registry.replication_regions_input = None;
            }
        } else if let Some(ai) = config.downcast_mut::<Ai>() {
            if !operator.reads(&AI_MODELS) {
                ai.models.clear();
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
    refused
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
        "This release uses {}; the deployment's Operator must be {newest} or newer. Update the Operator, then deploy the release again. The deployment keeps running its current release.",
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
        assert!(apply(&mut stack, None, &operator).is_empty());
        let text = serde_json::to_string(&stack).unwrap();
        for gone in [
            "resourceChoices",
            "commandsCapabilityFile",
            "replicationRegionsInput",
        ] {
            assert!(!text.contains(gone), "{gone} still sent");
        }
    }

    #[test]
    fn the_message_names_the_features_and_the_newest_version() {
        let message = refusal_message(&[&STORAGE_NOTIFICATIONS, &REDIS]);
        assert!(message.contains("storage notifications, Redis resources"));
        assert!(message.contains("must be 3.3.59 or newer"));
        assert!(message.contains("keeps running its current release"));
    }
}
