//! Provisioning a sandbox never reaches a live session. On Google Cloud the private endpoint an
//! `internet` sandbox is reached through needs one short-lived probe sandbox to learn the service
//! attachment, so `sandbox/private-endpoint` may create, read, list and delete a session. It must
//! never run code in one, read its files, snapshot it or change it: those verbs belong to other
//! sets. `sandbox/provision` itself holds no session verb at all.

use std::collections::BTreeSet;

fn gcp_session_verbs(id: &str) -> (BTreeSet<String>, bool) {
    let set = alien_permissions::get_permission_set(id).expect("set exists");
    let gcp = set.platforms.gcp.as_ref().expect("gcp entries");
    let permissions: Vec<String> = gcp
        .iter()
        .flat_map(|entry| entry.grant.permissions.iter().flatten().cloned())
        .collect();
    let sessions = permissions
        .iter()
        .filter(|permission| permission.starts_with("aiplatform.sandboxEnvironments."))
        .cloned()
        .collect();
    let executes = permissions
        .iter()
        .any(|permission| permission.ends_with(".execute"));
    (sessions, executes)
}

#[test]
fn sandbox_provision_grants_no_session_verb_on_gcp() {
    let (sessions, executes) = gcp_session_verbs("sandbox/provision");
    assert!(sessions.is_empty(), "{sessions:?}");
    assert!(!executes);
}

#[test]
fn the_private_endpoint_set_grants_only_the_session_lifecycle_verbs_on_gcp() {
    let (sessions, executes) = gcp_session_verbs("sandbox/private-endpoint");
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
        "sandbox/private-endpoint must hold exactly the lifecycle verbs, never execute"
    );
    assert!(!executes);
}
