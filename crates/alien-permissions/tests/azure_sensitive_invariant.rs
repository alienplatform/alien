use alien_permissions::{list_permission_set_ids, AZURE_SANDBOX_DATA_PLANE_ROLE};

const SENSITIVE_IMPLICIT_ACTIONS: &[&str] = &[
    "Microsoft.Storage/storageAccounts/listKeys/action",
    // The account API key, which is the whole inference surface. Azure spells it `listkeys` in
    // the role definition and `listKeys` in its docs, which is why every comparison below is
    // case-insensitive.
    "Microsoft.CognitiveServices/accounts/listKeys/action",
    "Microsoft.App/containerApps/listSecrets/action",
    "Microsoft.App/managedEnvironments/listSecrets/action",
];

const SENSITIVE_IMPLICIT_DATA_ACTIONS: &[&str] = &[
    // Reaching inside a live sandbox session: running a command, or moving its files. Session
    // lifecycle is separate on Azure and is not listed here.
    "Microsoft.App/sandboxGroups/sandboxes/executeCommand/action",
    "Microsoft.App/sandboxGroups/sandboxes/executeShellCommand/action",
    "Microsoft.App/sandboxGroups/sandboxes/exec/stream/action",
    "Microsoft.App/sandboxGroups/sandboxes/files/read",
    "Microsoft.App/sandboxGroups/sandboxes/files/write",
    "Microsoft.App/sandboxGroups/sandboxes/files/delete",
    "Microsoft.App/sandboxGroups/sandboxes/downloadContentPackage/action",
    "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read",
    "Microsoft.Storage/storageAccounts/tableServices/tables/entities/read",
    "Microsoft.KeyVault/vaults/secrets/read",
    "Microsoft.ServiceBus/namespaces/queues/messages/receive/action",
    "Microsoft.ServiceBus/namespaces/topics/subscriptions/messages/receive/action",
    "Microsoft.ServiceBus/namespaces/queues/messages/peek/action",
    "Microsoft.ServiceBus/namespaces/topics/subscriptions/messages/peek/action",
];

const SENSITIVE_IMPLICIT_ROLES: &[&str] = &[
    "AcrPull",
    "AcrPush",
    // Carries `Microsoft.CognitiveServices/accounts/listkeys/action` and a
    // `Microsoft.CognitiveServices/*` dataAction, so it hands over the account key.
    "Cognitive Services User",
    // Carries the whole sandbox data plane, session contents included, so it belongs only to sets
    // meant to reach inside a session — `sandbox/execute` and `sandbox/remote-execute`. Named
    // from the shared constant so this list and the reach predicate cannot disagree.
    AZURE_SANDBOX_DATA_PLANE_ROLE,
    "Azure Service Bus Data Receiver",
    "Key Vault Secrets User",
    "Storage Blob Data Contributor",
    "Storage Blob Data Reader",
    "Storage Table Data Contributor",
    "Storage Table Data Reader",
];

#[test]
fn azure_implicit_management_sets_do_not_grant_sensitive_content() {
    for permission_set_id in list_permission_set_ids() {
        if !is_implicit_management_set(permission_set_id) {
            continue;
        }

        let permission_set = alien_permissions::get_permission_set(permission_set_id)
            .expect("permission set exists");
        let Some(azure_entries) = &permission_set.platforms.azure else {
            continue;
        };

        for (index, entry) in azure_entries.iter().enumerate() {
            if let Some(roles) = &entry.grant.predefined_roles {
                for role in roles {
                    assert!(
                        !names_a_sensitive_entry(role, SENSITIVE_IMPLICIT_ROLES),
                        "{permission_set_id} Azure entry {index} uses sensitive predefined role {role}"
                    );
                }
            }

            if let Some(actions) = &entry.grant.actions {
                for action in actions {
                    assert!(
                        !names_a_sensitive_entry(action, SENSITIVE_IMPLICIT_ACTIONS),
                        "{permission_set_id} Azure entry {index} grants sensitive action {action}"
                    );
                    assert!(
                        !wildcard_covers(action, SENSITIVE_IMPLICIT_ACTIONS),
                        "{permission_set_id} Azure entry {index} covers a sensitive action \
                         through the wildcard {action}"
                    );
                }
            }

            if let Some(data_actions) = &entry.grant.data_actions {
                for data_action in data_actions {
                    assert!(
                        !names_a_sensitive_entry(data_action, SENSITIVE_IMPLICIT_DATA_ACTIONS),
                        "{permission_set_id} Azure entry {index} grants sensitive data action {data_action}"
                    );
                    // A wildcard covers every action it grants without matching any by string, so
                    // the list above alone would miss it. Narrower than "reaches a session":
                    // creating/deleting is reach for the single-tenancy gate but not another
                    // session's contents, so `sandbox/management`'s lifecycle verbs still pass.
                    assert!(
                        !wildcard_covers(data_action, SENSITIVE_IMPLICIT_DATA_ACTIONS),
                        "{permission_set_id} Azure entry {index} covers a sensitive data action \
                         through the wildcard {data_action}"
                    );
                }
            }
        }
    }
}

fn is_implicit_management_set(permission_set_id: &str) -> bool {
    permission_set_id.ends_with("/heartbeat")
        || permission_set_id.ends_with("/management")
        || permission_set_id.ends_with("/provision")
}

/// Whether a granted action carrying a `*` covers anything on `sensitive`. Compared as a prefix
/// — that's what a wildcard means — and case-insensitively, since Azure spells the same action
/// both ways itself (`listkeys` in role definitions, `listKeys` in docs).
fn wildcard_covers(granted: &str, sensitive: &[&str]) -> bool {
    let Some((literal, _)) = granted.split_once('*') else {
        return false;
    };
    let literal = literal.to_ascii_lowercase();
    sensitive
        .iter()
        .any(|entry| entry.to_ascii_lowercase().starts_with(&literal))
}

/// Whether `granted` names something on `sensitive`, ignoring case for the same reason.
fn names_a_sensitive_entry(granted: &str, sensitive: &[&str]) -> bool {
    sensitive
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(granted))
}
