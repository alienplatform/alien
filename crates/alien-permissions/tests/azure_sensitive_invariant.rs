use alien_permissions::list_permission_set_ids;

const SENSITIVE_IMPLICIT_ACTIONS: &[&str] = &[
    "Microsoft.Storage/storageAccounts/listKeys/action",
    "Microsoft.App/containerApps/listSecrets/action",
    "Microsoft.App/managedEnvironments/listSecrets/action",
];

const SENSITIVE_IMPLICIT_DATA_ACTIONS: &[&str] = &[
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
                        !SENSITIVE_IMPLICIT_ROLES.contains(&role.as_str()),
                        "{permission_set_id} Azure entry {index} uses sensitive predefined role {role}"
                    );
                }
            }

            if let Some(actions) = &entry.grant.actions {
                for action in actions {
                    assert!(
                        !SENSITIVE_IMPLICIT_ACTIONS.contains(&action.as_str()),
                        "{permission_set_id} Azure entry {index} grants sensitive action {action}"
                    );
                }
            }

            if let Some(data_actions) = &entry.grant.data_actions {
                for data_action in data_actions {
                    assert!(
                        !SENSITIVE_IMPLICIT_DATA_ACTIONS.contains(&data_action.as_str()),
                        "{permission_set_id} Azure entry {index} grants sensitive data action {data_action}"
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
