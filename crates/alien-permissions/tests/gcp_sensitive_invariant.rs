use alien_permissions::list_permission_set_ids;

const SENSITIVE_IMPLICIT_PERMISSIONS: &[&str] = &[
    "storage.objects.get",
    "datastore.entities.get",
    "datastore.entities.list",
    "secretmanager.versions.access",
    "pubsub.subscriptions.consume",
    "artifactregistry.repositories.downloadArtifacts",
    "cloudbuild.builds.get",
    "cloudbuild.builds.list",
];

const SENSITIVE_IMPLICIT_ROLES: &[&str] = &[
    "roles/storage.admin",
    "roles/storage.objectViewer",
    "roles/storage.objectUser",
    "roles/storage.objectAdmin",
    "roles/datastore.viewer",
    "roles/datastore.user",
    "roles/datastore.owner",
    "roles/datastore.admin",
    "roles/secretmanager.secretAccessor",
    "roles/secretmanager.admin",
    "roles/pubsub.subscriber",
    "roles/pubsub.editor",
    "roles/pubsub.admin",
    "roles/artifactregistry.reader",
    "roles/artifactregistry.writer",
    "roles/artifactregistry.repoAdmin",
    "roles/artifactregistry.admin",
    "roles/cloudbuild.builds.viewer",
    "roles/cloudbuild.builds.editor",
    "roles/cloudbuild.admin",
];

#[test]
fn gcp_implicit_management_sets_do_not_grant_sensitive_content() {
    for permission_set_id in list_permission_set_ids() {
        if !is_implicit_management_set(permission_set_id) {
            continue;
        }

        let permission_set = alien_permissions::get_permission_set(permission_set_id)
            .expect("permission set exists");
        let Some(gcp_entries) = &permission_set.platforms.gcp else {
            continue;
        };

        for (index, entry) in gcp_entries.iter().enumerate() {
            if let Some(roles) = &entry.grant.predefined_roles {
                for role in roles {
                    assert!(
                        !SENSITIVE_IMPLICIT_ROLES.contains(&role.as_str()),
                        "{permission_set_id} GCP entry {index} uses sensitive predefined role {role}"
                    );
                }
            }

            let residual_permissions = entry
                .grant
                .residual_permissions
                .as_ref()
                .or(entry.grant.permissions.as_ref());
            if let Some(permissions) = residual_permissions {
                for permission in permissions {
                    assert!(
                        !SENSITIVE_IMPLICIT_PERMISSIONS.contains(&permission.as_str()),
                        "{permission_set_id} GCP entry {index} grants sensitive permission {permission}"
                    );
                }
            }
        }
    }
}

fn is_implicit_management_set(permission_set_id: &str) -> bool {
    permission_set_id.ends_with("/heartbeat")
        || permission_set_id.ends_with("/management")
        || permission_set_id.ends_with("-management")
        || permission_set_id.ends_with("/provision")
}
