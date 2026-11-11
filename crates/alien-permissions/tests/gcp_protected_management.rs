//! What a protected install's management identity can do on GCP. The verifier alone writes Cloud
//! Run services and jobs, instance templates, managed instance groups and what decides who code
//! runs as, so the management identity must hold none of it, while an unprotected install keeps
//! every grant it had. Checked on the roles rendered for every set an automatic management grant
//! can hold, and on the IAM conditions that decide who reaches the signing inputs.

use std::collections::{BTreeMap, BTreeSet};

use alien_permissions::{
    generators::{gcp_role_without_family_permissions, GcpBindingTargetScope, GcpGrantPlan, GcpRuntimePermissionsGenerator},
    get_permission_set, list_permission_set_ids, BindingTarget, PermissionContext,
};

const PROJECT: &str = "orders-project";
const PROJECT_NUMBER: &str = "123456789012";
const PREFIX: &str = "orders";

fn context(protected: bool) -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix(PREFIX)
        .with_deployment_name("Orders")
        .with_project_name(PROJECT)
        .with_project_number(PROJECT_NUMBER)
        .with_region("us-central1")
        .with_managing_project_id("vendor-project")
        .with_service_account_name("orders-management")
        .with_signing_protected(protected)
}

/// The sets an automatic management grant holds on GCP: every resource type's provision,
/// management, heartbeat and telemetry set that GCP supports at stack scope.
fn management_set_ids() -> Vec<&'static str> {
    list_permission_set_ids()
        .into_iter()
        .filter(|id| {
            let Some((resource_type, operation)) = id.split_once('/') else {
                return false;
            };
            resource_type != "remote-stack-management"
                && matches!(
                    operation,
                    "provision" | "management" | "heartbeat" | "telemetry"
                )
                && get_permission_set(id)
                    .and_then(|set| set.platforms.gcp.as_ref())
                    .is_some_and(|entries| {
                        entries.iter().any(|entry| entry.binding.stack.is_some())
                    })
        })
        .collect()
}

fn plan(id: &str, protected: bool) -> GcpGrantPlan {
    GcpRuntimePermissionsGenerator::new()
        .generate_grant_plan(
            get_permission_set(id).expect("registered set"),
            BindingTarget::Stack,
            &context(protected),
        )
        .unwrap_or_else(|error| panic!("{id} renders: {error}"))
}

/// Every permission the identity holds through a custom role bound on the project, with the
/// condition of the binding (`None` when unconditional), by role ID.
fn project_grants(plan: &GcpGrantPlan) -> BTreeMap<String, (BTreeSet<String>, Option<String>)> {
    plan.bindings_for_target(GcpBindingTargetScope::Project)
        .into_iter()
        .filter_map(|binding| {
            let role = plan
                .custom_roles
                .iter()
                .find(|role| role.name == binding.role)?;
            Some((
                role.role_id.clone(),
                (
                    role.included_permissions.iter().cloned().collect(),
                    binding.condition.map(|condition| condition.expression),
                ),
            ))
        })
        .collect()
}

/// What the verifier alone may do, named independently of the generator: a Cloud Run service or
/// job write, booting, reconfiguring or scaling a machine group, pointing it at a template,
/// acting as or minting tokens for another service account, changing any IAM policy or a custom
/// role, and starting code-carrying builds or engines.
fn verifier_only(permission: &str) -> bool {
    alien_permissions::generators::gcp_family(permission).is_some()
}

/// The permissions this file named by hand before the families existed. Each must be in a family.
fn hand_listed_verifier_only(permission: &str) -> bool {
    permission.ends_with(".setIamPolicy")
        || [
            "run.services.create",
            "run.services.update",
            "run.services.delete",
            "run.services.createTagBinding",
            "run.services.deleteTagBinding",
            "run.jobs.create",
            "run.jobs.update",
            "run.jobs.delete",
            "run.jobs.runWithOverrides",
            "run.jobs.createTagBinding",
            "run.jobs.deleteTagBinding",
            "compute.instanceGroupManagers.create",
            "compute.instanceGroupManagers.update",
            "compute.instanceTemplates.useReadOnly",
            "compute.instances.create",
            "compute.instances.setMetadata",
            "compute.instances.setServiceAccount",
            "compute.instances.setTags",
            "compute.instanceTemplates.create",
            "compute.projects.setCommonInstanceMetadata",
            "iam.serviceAccounts.actAs",
            "iam.serviceAccounts.getAccessToken",
            "iam.serviceAccounts.signBlob",
            "iam.serviceAccounts.signJwt",
            "iam.serviceAccountKeys.create",
            "iam.roles.create",
            "iam.roles.update",
            "iam.roles.undelete",
            "cloudbuild.builds.create",
            "cloudbuild.builds.update",
            "aiplatform.reasoningEngines.create",
            "aiplatform.sandboxEnvironmentTemplates.create",
            "aiplatform.sandboxEnvironmentTemplates.delete",
            "aiplatform.sandboxEnvironments.create",
            "storage.hmacKeys.create",
        ]
        .contains(&permission)
}

#[test]
fn protected_management_holds_nothing_only_the_verifier_may_do() {
    let mut removed = BTreeSet::new();
    for id in management_set_ids() {
        let unprotected = project_grants(&plan(id, false));
        let protected = project_grants(&plan(id, true));
        for (role_id, (permissions, condition)) in &protected {
            let held: Vec<&String> = permissions.iter().filter(|p| verifier_only(p)).collect();
            assert!(held.is_empty(), "{id}: {role_id} still holds {held:?}");
            // A role keeps its ID and binding and only loses permissions, so setup updates it
            // in place. A binding that grants Secret Manager also leaves out the signing inputs.
            // A predefined role that carries a family permission becomes a custom role, so
            // the protected rendering can hold a role the unprotected one doesn't.
            let Some((before, before_condition)) = unprotected.get(role_id) else {
                let replacement: BTreeSet<&str> = [
                    "roles/storage.objectAdmin",
                    "roles/pubsub.subscriber",
                    "roles/serviceusage.serviceUsageAdmin",
                ]
                .into_iter()
                .filter_map(gcp_role_without_family_permissions)
                .flatten()
                .copied()
                .collect();
                assert!(
                    permissions.iter().all(|p| replacement.contains(p.as_str())),
                    "{id}: {role_id} is new in the protected rendering and holds {permissions:?}"
                );
                continue;
            };
            let exclusion = r#"!resource.name.extract("/secrets/{name}").startsWith("signed-")"#;
            let expected = if permissions.iter().any(|p| p.starts_with("secretmanager.")) {
                Some(match before_condition {
                    Some(before) => format!("({before}) && {exclusion}"),
                    None => exclusion.to_string(),
                })
            } else {
                before_condition.clone()
            };
            assert_eq!(condition, &expected, "{id}: {role_id}");
            assert!(permissions.is_subset(before), "{id}: {role_id}");
        }
        for (role_id, (before, _)) in &unprotected {
            let kept = protected
                .get(role_id)
                .map(|(p, _)| p.clone())
                .unwrap_or_default();
            for permission in before.difference(&kept) {
                assert!(
                    verifier_only(permission),
                    "{id}: {role_id} lost {permission}, which management needs"
                );
                removed.insert(permission.clone());
            }
        }
    }
    // The unprotected grants still hold every one of these, so the lock removes real powers.
    assert_eq!(
        removed,
        [
            "aiplatform.reasoningEngines.create",
            "aiplatform.sandboxEnvironmentTemplates.create",
            "aiplatform.sandboxEnvironments.create",
            "cloudbuild.builds.create",
            "cloudbuild.builds.update",
            "compute.instanceGroupManagers.create",
            "compute.instanceGroupManagers.update",
            "compute.instanceTemplates.create",
            "compute.instanceTemplates.useReadOnly",
            "compute.instances.create",
            "compute.instances.setMetadata",
            "compute.instances.setTags",
            "iam.roles.create",
            "iam.roles.undelete",
            "iam.roles.update",
            "iam.serviceAccounts.actAs",
            "iam.serviceAccounts.setIamPolicy",
            "pubsub.subscriptions.setIamPolicy",
            "pubsub.topics.setIamPolicy",
            "resourcemanager.projects.setIamPolicy",
            "run.jobs.create",
            "run.jobs.createTagBinding",
            "run.jobs.deleteTagBinding",
            "run.jobs.setIamPolicy",
            "run.jobs.update",
            "run.services.create",
            "run.services.createTagBinding",
            "run.services.deleteTagBinding",
            "run.services.setIamPolicy",
            "run.services.update",
            "storage.buckets.setIamPolicy",
            "storage.hmacKeys.create",
            "storage.managedFolders.setIamPolicy",
            "artifactregistry.repositories.setIamPolicy",
            // Added by the families: sandbox lifecycle and snapshots, Cloud SQL network fields,
            // snapshot and health-check reads that let a group name another install's, and
            // subscriptions that deliver the install's messages elsewhere.
            "aiplatform.sandboxEnvironments.pause",
            "aiplatform.sandboxEnvironments.resume",
            "aiplatform.sandboxEnvironments.snapshot",
            "cloudsql.instances.create",
            "cloudsql.instances.update",
            "compute.healthChecks.useReadOnly",
            "compute.snapshots.useReadOnly",
            "pubsub.subscriptions.create",
            "pubsub.subscriptions.update",
            "pubsub.topics.attachSubscription",
            "storage.hmacKeys.update",
        ]
        .into_iter()
        .map(String::from)
        .collect::<BTreeSet<_>>()
    );
}

#[test]
fn protected_management_keeps_scaling_reads_and_infrastructure() {
    let held: BTreeSet<String> = management_set_ids()
        .into_iter()
        .flat_map(|id| project_grants(&plan(id, true)).into_values())
        .flat_map(|(permissions, _)| permissions)
        .collect();
    for kept in [
        // Reading machine groups and the verifier's templates; scaling goes through the verifier.
        "compute.instanceGroupManagers.get",
        "compute.instanceTemplates.get",
        "compute.instances.get",
        "compute.instances.getSerialPortOutput",
        "run.services.get",
        "run.services.getIamPolicy",
        "run.revisions.delete",
        "secretmanager.versions.add",
        "storage.buckets.create",
        "pubsub.topics.create",
        "cloudbuild.workerpools.update",
    ] {
        assert!(held.contains(kept), "{kept}");
    }
}

#[test]
fn protected_management_is_never_bound_to_an_impersonation_role() {
    let error = GcpRuntimePermissionsGenerator::new()
        .generate_grant_plan(
            get_permission_set("service-account/impersonate").expect("registered set"),
            BindingTarget::Stack,
            &context(true),
        )
        .expect_err("a token creator binding lets management act as any service account");
    assert_eq!(error.code, "GENERATOR_ERROR");
    assert!(
        error.message.contains("roles/iam.serviceAccountTokenCreator"),
        "{error}"
    );
    assert!(GcpRuntimePermissionsGenerator::new()
        .generate_grant_plan(
            get_permission_set("service-account/impersonate").expect("registered set"),
            BindingTarget::Stack,
            &context(false),
        )
        .is_ok());
}

/// Evaluates the IAM condition subset the catalog uses: `resource.name.startsWith("…")`,
/// `resource.type == "…"`, `!`, `&&`, `||` and parentheses.
struct Condition<'a> {
    text: &'a str,
    name: &'a str,
    kind: &'a str,
}

impl Condition<'_> {
    fn eval(text: &str, name: &str, kind: &str) -> bool {
        let mut condition = Condition { text, name, kind };
        let value = condition.or();
        assert!(
            condition.text.trim().is_empty(),
            "unparsed: {}",
            condition.text
        );
        value
    }

    fn eat(&mut self, token: &str) -> bool {
        let rest = self.text.trim_start();
        match rest.strip_prefix(token) {
            Some(rest) => {
                self.text = rest;
                true
            }
            None => false,
        }
    }

    fn string(&mut self) -> String {
        assert!(self.eat("\""), "expected a string at {}", self.text);
        let end = self.text.find('"').expect("closed string");
        let value = self.text[..end].to_string();
        self.text = &self.text[end + 1..];
        value
    }

    fn or(&mut self) -> bool {
        let mut value = self.and();
        while self.eat("||") {
            value |= self.and();
        }
        value
    }

    fn and(&mut self) -> bool {
        let mut value = self.not();
        while self.eat("&&") {
            value &= self.not();
        }
        value
    }

    fn not(&mut self) -> bool {
        if self.eat("!") {
            return !self.not();
        }
        if self.eat("(") {
            let value = self.or();
            assert!(self.eat(")"), "unclosed parenthesis");
            return value;
        }
        if self.eat("resource.name.startsWith(") {
            let prefix = self.string();
            assert!(self.eat(")"));
            return self.name.starts_with(&prefix);
        }
        if self.eat("resource.name.extract(") {
            // `extract("<before>{name}<after>")`: the text between, or "" when absent.
            let template = self.string();
            assert!(self.eat(")"));
            assert!(self.eat(".startsWith("));
            let prefix = self.string();
            assert!(self.eat(")"));
            let (before, after) = template.split_once("{name}").expect("one {name}");
            let extracted = self
                .name
                .find(before)
                .map(|start| {
                    let rest = &self.name[start + before.len()..];
                    match after {
                        "" => rest,
                        after => rest.split(after).next().unwrap_or(""),
                    }
                })
                .unwrap_or("");
            return extracted.starts_with(&prefix);
        }
        if self.eat("resource.type ==") {
            return self.string() == self.kind;
        }
        panic!("unsupported condition at {}", self.text)
    }
}

/// Whether some project binding in `plans` grants `permission` on the resource `name` of type
/// `kind`.
fn allowed(plans: &[GcpGrantPlan], permission: &str, name: &str, kind: &str) -> bool {
    plans.iter().any(|plan| {
        project_grants(plan)
            .into_values()
            .any(|(permissions, condition)| {
                permissions.contains(permission)
                    && condition.is_none_or(|condition| Condition::eval(&condition, name, kind))
            })
    })
}

const SECRET: &str = "secretmanager.googleapis.com/Secret";
const VERSION: &str = "secretmanager.googleapis.com/SecretVersion";

#[test]
fn protected_management_holds_no_parameter_manager_permission() {
    for protected in [false, true] {
        for id in management_set_ids() {
            for (role_id, (permissions, _)) in project_grants(&plan(id, protected)) {
                let held: Vec<&String> = permissions
                    .iter()
                    .filter(|permission| permission.starts_with("parametermanager."))
                    .collect();
                assert!(held.is_empty(), "{id}: {role_id} holds {held:?}");
            }
        }
    }
}

#[test]
fn protected_management_cannot_touch_signing_inputs_or_the_ledger() {
    // Inputs of this install and of one whose prefix extends this one's, and the ledger: a
    // version added to it would let management raise or reset what the install accepted.
    let inputs = [
        format!("projects/{PROJECT_NUMBER}/secrets/signed-{PREFIX}-input-api-key"),
        format!("projects/{PROJECT_NUMBER}/secrets/signed-{PREFIX}-eu-input-api-key"),
        format!("projects/{PROJECT_NUMBER}/secrets/signed-{PREFIX}-state"),
    ];
    let own_secret = format!("projects/{PROJECT_NUMBER}/secrets/{PREFIX}-otlp");
    for protected in [false, true] {
        let management: Vec<GcpGrantPlan> = management_set_ids()
            .into_iter()
            .map(|id| plan(id, protected))
            .collect();
        // The grants do reach the install's own secrets, so the conditions are evaluated.
        for permission in ["secretmanager.secrets.delete", "secretmanager.versions.add"] {
            assert!(
                allowed(&management, permission, &own_secret, SECRET),
                "protected={protected}: {permission} on {own_secret}"
            );
        }
        if !protected {
            continue;
        }
        for secret in &inputs {
            for (permission, name, kind) in [
                ("secretmanager.versions.add", secret.clone(), SECRET),
                ("secretmanager.secrets.delete", secret.clone(), SECRET),
                ("secretmanager.secrets.update", secret.clone(), SECRET),
                ("secretmanager.secrets.setIamPolicy", secret.clone(), SECRET),
                (
                    "secretmanager.versions.access",
                    format!("{secret}/versions/1"),
                    VERSION,
                ),
                (
                    "secretmanager.versions.destroy",
                    format!("{secret}/versions/1"),
                    VERSION,
                ),
                (
                    "secretmanager.versions.disable",
                    format!("{secret}/versions/1"),
                    VERSION,
                ),
            ] {
                assert!(
                    !allowed(&management, permission, &name, kind),
                    "{permission} on {name}"
                );
            }
        }
    }
}

#[test]
fn unprotected_management_conditions_are_unchanged() {
    // The exclusion renders only for protected installs.
    for id in management_set_ids() {
        for (role_id, (_, condition)) in project_grants(&plan(id, false)) {
            assert!(
                !condition.as_deref().unwrap_or("").contains("signed-"),
                "{id}: {role_id}"
            );
        }
    }
}

#[test]
fn every_permission_this_file_names_by_hand_is_in_a_family() {
    // The list of what only the verifier may do was written by hand; the families replace it.
    let mut checked = 0;
    for id in alien_permissions::list_permission_set_ids() {
        let set = alien_permissions::get_permission_set(id).expect("listed set");
        for entry in set.platforms.gcp.iter().flatten() {
            for permission in entry.grant.permissions.iter().flatten() {
                // Deletes are out of scope and in no family; the verifier still does its own.
                if hand_listed_verifier_only(permission) && !permission.ends_with(".delete") {
                    assert!(verifier_only(permission), "{id}: {permission}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 10, "only {checked} hand-listed permissions found");
}

/// Every predefined role the registry's sets bind is either on the built-in list (and so has no
/// family permission by review), is replaced by a custom role without family permissions, or is
/// the one role management is never bound to.
#[test]
fn every_predefined_role_in_the_registry_is_accounted_for() {
    use alien_permissions::generators::{gcp_family, GCP_BUILT_IN_ROLES};

    for id in list_permission_set_ids() {
        let Some(entries) = get_permission_set(id).and_then(|set| set.platforms.gcp.as_ref())
        else {
            continue;
        };
        for entry in entries {
            for role in entry.grant.predefined_roles.iter().flatten() {
                let accounted = GCP_BUILT_IN_ROLES.contains(&role.as_str())
                    || gcp_role_without_family_permissions(role).is_some()
                    // Only on sets management never holds: impersonation and remote execution.
                    || id.starts_with("service-account/impersonate")
                    || id.ends_with("/remote-execute")
                    || id.ends_with("/execute")
                    || id.ends_with("/invoke");
                assert!(accounted, "{id} binds {role}, which no list accounts for");
            }
        }
    }
    for role in ["roles/storage.objectAdmin", "roles/pubsub.subscriber", "roles/serviceusage.serviceUsageAdmin"] {
        for permission in gcp_role_without_family_permissions(role).expect("replaced") {
            assert_eq!(gcp_family(permission), None, "{role}: {permission}");
        }
    }
}
