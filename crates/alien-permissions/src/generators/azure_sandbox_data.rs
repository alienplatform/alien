//! What a protected install's remote-bindings identity may do inside an Azure sandbox group.
//!
//! The predefined `Container Apps SandboxGroup Data Owner` role is four wildcards over every
//! `Microsoft.App/sandboxGroups` data action. Besides driving a sandbox it holds agent
//! identities, credentials, secrets, connections, egress policies, content packages, volumes,
//! disk images and sandbox creation, which decide what runs, as whom, and who can reach it.
//! A protected install therefore replaces the role with the explicit list below: what a remote
//! caller needs to drive a sandbox that already exists. Sandbox creation goes through the
//! verifier instead.
//!
//! Every operation Azure lists under `sandboxGroups` is classified in exactly one of the two
//! lists. The test at the bottom pins the list Azure published, so a new operation fails it
//! until someone classifies it.

/// The predefined role the replacement stands in for.
pub const AZURE_SANDBOX_DATA_OWNER_ROLE: &str = "Container Apps SandboxGroup Data Owner";

/// Data actions a remote caller keeps: run and stream commands, move files, open ports, read
/// logs and state, stop, resume and delete sandboxes it was handed.
pub const AZURE_SANDBOX_REMOTE_DATA_ACTIONS: &[&str] = &[
    "Microsoft.App/sandboxGroups/diskimages/read",
    "Microsoft.App/sandboxGroups/sandboxes/count/read",
    "Microsoft.App/sandboxGroups/sandboxes/delete",
    "Microsoft.App/sandboxGroups/sandboxes/disable/action",
    "Microsoft.App/sandboxGroups/sandboxes/egress-decisions/read",
    "Microsoft.App/sandboxGroups/sandboxes/egressDecisions/read",
    "Microsoft.App/sandboxGroups/sandboxes/enable/action",
    "Microsoft.App/sandboxGroups/sandboxes/exec/stream/action",
    "Microsoft.App/sandboxGroups/sandboxes/executeCommand/action",
    "Microsoft.App/sandboxGroups/sandboxes/executeShellCommand/action",
    "Microsoft.App/sandboxGroups/sandboxes/files/delete",
    "Microsoft.App/sandboxGroups/sandboxes/files/read",
    "Microsoft.App/sandboxGroups/sandboxes/files/write",
    "Microsoft.App/sandboxGroups/sandboxes/lifecycle/write",
    "Microsoft.App/sandboxGroups/sandboxes/logstream/action",
    "Microsoft.App/sandboxGroups/sandboxes/portforward/stream/action",
    "Microsoft.App/sandboxGroups/sandboxes/ports/delete",
    "Microsoft.App/sandboxGroups/sandboxes/ports/read",
    "Microsoft.App/sandboxGroups/sandboxes/ports/write",
    "Microsoft.App/sandboxGroups/sandboxes/processes/stream/action",
    "Microsoft.App/sandboxGroups/sandboxes/read",
    "Microsoft.App/sandboxGroups/sandboxes/resume/action",
    "Microsoft.App/sandboxGroups/sandboxes/stats/read",
    "Microsoft.App/sandboxGroups/sandboxes/stop/action",
    "Microsoft.App/sandboxGroups/snapshots/count/read",
    "Microsoft.App/sandboxGroups/snapshots/read",
];

/// Data actions a remote caller never holds, with why.
pub const AZURE_SANDBOX_REMOTE_DENIED_DATA_ACTIONS: &[(&str, &str)] = &[
    // IDENTITY: who a sandbox runs as, and the credentials it can present.
    (
        "Microsoft.App/sandboxGroups/agentIdentities/read",
        "identity",
    ),
    (
        "Microsoft.App/sandboxGroups/agentIdentities/write",
        "identity",
    ),
    (
        "Microsoft.App/sandboxGroups/agentIdentities/delete",
        "identity",
    ),
    ("Microsoft.App/sandboxGroups/credentials/read", "identity"),
    ("Microsoft.App/sandboxGroups/credentials/write", "identity"),
    ("Microsoft.App/sandboxGroups/credentials/delete", "identity"),
    ("Microsoft.App/sandboxGroups/secrets/read", "identity"),
    (
        "Microsoft.App/sandboxGroups/secrets/peek/action",
        "identity",
    ),
    ("Microsoft.App/sandboxGroups/secrets/write", "identity"),
    ("Microsoft.App/sandboxGroups/secrets/delete", "identity"),
    // REACH: who can talk to a sandbox and what it can talk to.
    ("Microsoft.App/sandboxGroups/connections/read", "reach"),
    ("Microsoft.App/sandboxGroups/connections/write", "reach"),
    ("Microsoft.App/sandboxGroups/connections/delete", "reach"),
    ("Microsoft.App/sandboxGroups/egressPolicies/read", "reach"),
    ("Microsoft.App/sandboxGroups/egressPolicies/write", "reach"),
    ("Microsoft.App/sandboxGroups/egressPolicies/delete", "reach"),
    (
        "Microsoft.App/sandboxGroups/sandboxes/connections/write",
        "reach",
    ),
    (
        "Microsoft.App/sandboxGroups/sandboxes/egresspolicy/write",
        "reach",
    ),
    // RUN: what a sandbox boots from or mounts, and creating one at all.
    ("Microsoft.App/sandboxGroups/contentpackages/read", "run"),
    ("Microsoft.App/sandboxGroups/contentpackages/write", "run"),
    ("Microsoft.App/sandboxGroups/contentpackages/delete", "run"),
    ("Microsoft.App/sandboxGroups/diskimages/write", "run"),
    ("Microsoft.App/sandboxGroups/diskimages/delete", "run"),
    ("Microsoft.App/sandboxGroups/volumes/read", "run"),
    ("Microsoft.App/sandboxGroups/volumes/write", "run"),
    ("Microsoft.App/sandboxGroups/volumes/delete", "run"),
    ("Microsoft.App/sandboxGroups/volumes/count/read", "run"),
    ("Microsoft.App/sandboxGroups/volumes/files/read", "run"),
    ("Microsoft.App/sandboxGroups/volumes/files/write", "run"),
    ("Microsoft.App/sandboxGroups/volumes/files/delete", "run"),
    ("Microsoft.App/sandboxGroups/sandboxes/volumes/write", "run"),
    (
        "Microsoft.App/sandboxGroups/sandboxes/pod-volumes/write",
        "run",
    ),
    (
        "Microsoft.App/sandboxGroups/sandboxes/podVolumes/write",
        "run",
    ),
    ("Microsoft.App/sandboxGroups/sandboxes/write", "run"),
    ("Microsoft.App/sandboxGroups/sandboxes/batch/write", "run"),
    (
        "Microsoft.App/sandboxGroups/sandboxes/snapshot/action",
        "run",
    ),
    ("Microsoft.App/sandboxGroups/sandboxes/commit/action", "run"),
    (
        "Microsoft.App/sandboxGroups/sandboxes/downloadContentPackage/action",
        "run",
    ),
    ("Microsoft.App/sandboxGroups/snapshots/delete", "run"),
];

/// The data actions that stand in for `role` on a protected install's remote-bindings identity.
pub fn azure_sandbox_role_substitute(role: &str) -> Option<&'static [&'static str]> {
    AZURE_SANDBOX_DATA_OWNER_ROLE
        .eq_ignore_ascii_case(role)
        .then_some(AZURE_SANDBOX_REMOTE_DATA_ACTIONS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The data actions Azure listed under `Microsoft.App/sandboxGroups` when this was
    /// classified (`az provider operation show --namespace Microsoft.App`, data actions only).
    const PUBLISHED: &[&str] = &[
        "agentIdentities/read",
        "agentIdentities/write",
        "agentIdentities/delete",
        "connections/read",
        "connections/write",
        "connections/delete",
        "contentpackages/read",
        "contentpackages/write",
        "contentpackages/delete",
        "diskimages/read",
        "diskimages/write",
        "diskimages/delete",
        "egressPolicies/read",
        "egressPolicies/write",
        "egressPolicies/delete",
        "sandboxes/count/read",
        "sandboxes/read",
        "sandboxes/write",
        "sandboxes/delete",
        "sandboxes/executeCommand/action",
        "sandboxes/executeShellCommand/action",
        "sandboxes/snapshot/action",
        "sandboxes/stop/action",
        "sandboxes/enable/action",
        "sandboxes/disable/action",
        "sandboxes/commit/action",
        "sandboxes/resume/action",
        "sandboxes/downloadContentPackage/action",
        "sandboxes/logstream/action",
        "sandboxes/batch/write",
        "sandboxes/ports/read",
        "sandboxes/ports/write",
        "sandboxes/ports/delete",
        "sandboxes/connections/write",
        "sandboxes/volumes/write",
        "sandboxes/pod-volumes/write",
        "sandboxes/egress-decisions/read",
        "sandboxes/stats/read",
        "sandboxes/egresspolicy/write",
        "sandboxes/lifecycle/write",
        "sandboxes/files/read",
        "sandboxes/files/write",
        "sandboxes/files/delete",
        "sandboxes/exec/stream/action",
        "sandboxes/processes/stream/action",
        "secrets/read",
        "secrets/peek/action",
        "secrets/write",
        "secrets/delete",
        "snapshots/count/read",
        "snapshots/read",
        "snapshots/delete",
        "volumes/read",
        "volumes/write",
        "volumes/delete",
        "volumes/count/read",
        "volumes/files/read",
        "volumes/files/write",
        "volumes/files/delete",
        "sandboxes/podVolumes/write",
        "sandboxes/egressDecisions/read",
        "credentials/read",
        "credentials/write",
        "credentials/delete",
        "sandboxes/portforward/stream/action",
    ];

    fn full(operation: &str) -> String {
        format!("Microsoft.App/sandboxGroups/{operation}")
    }

    /// Every published operation is in exactly one list, and neither list names one Azure does
    /// not publish. A new Azure operation fails this until it is classified.
    #[test]
    fn every_published_sandbox_group_operation_is_classified_once() {
        let published: BTreeSet<String> = PUBLISHED.iter().map(|op| full(op)).collect();
        let allowed: BTreeSet<String> = AZURE_SANDBOX_REMOTE_DATA_ACTIONS
            .iter()
            .map(|a| a.to_string())
            .collect();
        let denied: BTreeSet<String> = AZURE_SANDBOX_REMOTE_DENIED_DATA_ACTIONS
            .iter()
            .map(|(a, _)| a.to_string())
            .collect();
        assert_eq!(
            allowed.len(),
            AZURE_SANDBOX_REMOTE_DATA_ACTIONS.len(),
            "no duplicates"
        );
        assert_eq!(
            denied.len(),
            AZURE_SANDBOX_REMOTE_DENIED_DATA_ACTIONS.len(),
            "no duplicates"
        );
        assert!(
            allowed.is_disjoint(&denied),
            "{:?}",
            allowed.intersection(&denied)
        );
        let classified: BTreeSet<String> = allowed.union(&denied).cloned().collect();
        assert_eq!(
            classified,
            published,
            "unclassified: {:?}; not published: {:?}",
            published.difference(&classified).collect::<Vec<_>>(),
            classified.difference(&published).collect::<Vec<_>>()
        );
    }

    /// The remote role holds nothing that creates a sandbox, changes what one boots from or
    /// mounts, or touches an identity, credential, secret, connection or egress policy.
    #[test]
    fn the_remote_role_holds_no_identity_reach_or_run_operation() {
        for action in AZURE_SANDBOX_REMOTE_DATA_ACTIONS {
            for forbidden in [
                "agentIdentities",
                "credentials",
                "secrets",
                "connections",
                "egressPolicies",
                "egresspolicy",
                "contentpackages",
                "volumes",
                "podVolumes",
                "pod-volumes",
                "snapshot/",
                "commit/",
                "batch/",
                "diskimages/write",
                "diskimages/delete",
            ] {
                assert!(!action.contains(forbidden), "{action} contains {forbidden}");
            }
            assert_ne!(*action, "Microsoft.App/sandboxGroups/sandboxes/write");
        }
    }

    #[test]
    fn only_the_data_owner_role_is_substituted() {
        assert!(azure_sandbox_role_substitute("container apps sandboxgroup data owner").is_some());
        assert!(azure_sandbox_role_substitute("Reader").is_none());
    }
}
