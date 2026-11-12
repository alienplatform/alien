use alien_permissions::sensitive_content::{
    AZURE_SENSITIVE_CONTENT_ACTIONS as SENSITIVE_IMPLICIT_ACTIONS,
    AZURE_SENSITIVE_CONTENT_DATA_ACTIONS as SENSITIVE_IMPLICIT_DATA_ACTIONS,
    AZURE_SENSITIVE_CONTENT_ROLES as SENSITIVE_IMPLICIT_ROLES,
};
use alien_permissions::list_permission_set_ids;

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
        // Preflight-authored for an Azure sandbox's manager; its name carries no suffix above.
        || permission_set_id == "sandbox/images"
}

/// The manager's disk-image grant reaches no sandbox and has no stack binding, so it can only
/// ever be bound on one sandbox group. A predefined role is refused outright: the one Azure
/// ships for this plane carries every session's contents too.
#[test]
fn sandbox_images_stays_on_disk_images_and_its_group() {
    let set = alien_permissions::get_permission_set("sandbox/images").expect("registered");
    let entries = set.platforms.azure.as_ref().expect("an azure entry");
    for entry in entries {
        assert!(entry.grant.predefined_roles.is_none(), "{entry:?}");
        assert!(entry.grant.actions.is_none(), "{entry:?}");
        for data_action in entry.grant.data_actions.iter().flatten() {
            assert!(
                data_action.starts_with("Microsoft.App/sandboxGroups/diskimages/"),
                "{data_action}"
            );
        }
        assert!(entry.binding.stack.is_none(), "{entry:?}");
    }
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
