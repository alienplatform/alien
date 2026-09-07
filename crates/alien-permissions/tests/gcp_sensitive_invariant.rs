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
    // Runs code and reads files inside a live sandbox session; belongs to sandbox/execute alone.
    "aiplatform.sandboxEnvironments.execute",
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

/// The two permission sets that may reach inside a sandbox session on GCP.
///
/// `execute` serves a workload in the customer's own cloud; `remote-execute` serves a hosted
/// caller across the Remote Bindings boundary, and is engine-scoped for that reason.
const SESSION_REACHING_SANDBOX_SETS: &[&str] = &["sandbox/execute", "sandbox/remote-execute"];

/// The execute verb reaches session content, so it must appear in those two sets and nowhere
/// else. Positive and negative in one: the collected set is asserted to equal exactly that list.
#[test]
fn gcp_sandbox_execute_permission_is_confined_to_the_session_reaching_sets() {
    const EXECUTE_PERMISSION: &str = "aiplatform.sandboxEnvironments.execute";

    let mut sets_granting_execute: Vec<&str> = Vec::new();
    for permission_set_id in list_permission_set_ids() {
        let permission_set = alien_permissions::get_permission_set(permission_set_id)
            .expect("permission set exists");
        let Some(gcp_entries) = &permission_set.platforms.gcp else {
            continue;
        };

        // Scan both lists unconditionally — an entry setting `permissions` and
        // `residualPermissions` together could otherwise hide the grant in the unscanned one.
        let grants_execute = gcp_entries.iter().any(|entry| {
            let permissions = entry.grant.permissions.as_deref().unwrap_or(&[]);
            let residual = entry.grant.residual_permissions.as_deref().unwrap_or(&[]);
            permissions
                .iter()
                .chain(residual)
                .any(|permission| permission == EXECUTE_PERMISSION)
        });
        if grants_execute {
            sets_granting_execute.push(permission_set_id);
        }
    }

    sets_granting_execute.sort_unstable();
    let mut expected = SESSION_REACHING_SANDBOX_SETS.to_vec();
    expected.sort_unstable();
    assert_eq!(
        sets_granting_execute, expected,
        "{EXECUTE_PERMISSION} reaches session content; a new set granting it reaches inside a \
         session"
    );
}

/// A heartbeat addresses the sandbox's parent and never a session of it.
///
/// Stricter than the reach predicate on purpose, and heartbeat-only: `sandbox/management` names
/// `aiplatform.sandboxEnvironments.create` and the rest of the session lifecycle because that is
/// what it is for, while a heartbeat that reads a session has crossed into a resource whose
/// contents it has no business near. Agent Platform's own health signal is the parent template's
/// lifecycle state, which is what the controller reads.
#[test]
fn a_gcp_heartbeat_reads_the_parent_and_never_a_session() {
    const SESSION_NAMESPACE: &str = "aiplatform.sandboxEnvironments.";

    for permission_set_id in list_permission_set_ids() {
        if !permission_set_id.ends_with("/heartbeat") {
            continue;
        }
        let permission_set = alien_permissions::get_permission_set(permission_set_id)
            .expect("permission set exists");
        let Some(gcp_entries) = &permission_set.platforms.gcp else {
            continue;
        };

        for (index, entry) in gcp_entries.iter().enumerate() {
            let permissions = entry
                .grant
                .permissions
                .iter()
                .flatten()
                .chain(entry.grant.residual_permissions.iter().flatten());
            for permission in permissions {
                assert!(
                    !permission.starts_with(SESSION_NAMESPACE),
                    "{permission_set_id} GCP entry {index} reads a sandbox session through \
                     {permission}; a heartbeat reads the parent's state"
                );
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
