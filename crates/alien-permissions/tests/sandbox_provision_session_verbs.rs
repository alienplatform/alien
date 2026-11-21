//! Provisioning a sandbox may create, read, list and delete a sandbox session (the Internet path
//! needs one short-lived probe sandbox to learn the service attachment). It must never run code
//! in a session, read its files, snapshot it or change it: those verbs belong to other sets.

use std::collections::BTreeSet;

#[test]
fn sandbox_provision_grants_only_the_session_lifecycle_verbs_on_gcp() {
    let set = alien_permissions::get_permission_set("sandbox/provision").expect("set exists");
    let gcp = set.platforms.gcp.as_ref().expect("gcp entries");
    let sessions: BTreeSet<String> = gcp
        .iter()
        .flat_map(|entry| entry.grant.permissions.iter().flatten().cloned())
        .filter(|permission| permission.starts_with("aiplatform.sandboxEnvironments."))
        .collect();
    let allowed: BTreeSet<String> = [
        "aiplatform.sandboxEnvironments.create",
        "aiplatform.sandboxEnvironments.get",
        "aiplatform.sandboxEnvironments.list",
        "aiplatform.sandboxEnvironments.delete",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(
        sessions, allowed,
        "sandbox/provision must hold exactly the lifecycle verbs, never execute"
    );
    assert!(gcp.iter().all(|entry| entry
        .grant
        .permissions
        .iter()
        .flatten()
        .all(|permission| !permission.ends_with(".execute"))));
}
