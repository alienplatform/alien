//! What a protected deployment's management identity can do on GCP. The gate alone writes Cloud
//! Run services, instance templates and what decides who code runs as, so the management
//! identity must hold none of it, while an unprotected deployment keeps every grant it had.
//! Checked on the roles rendered for every set an automatic management grant can hold, and on
//! the IAM conditions that decide who reaches the anchor's secret.

use std::collections::{BTreeMap, BTreeSet};

use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpGrantPlan, GcpRuntimePermissionsGenerator},
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

/// What the gate alone may do, named independently of the generator: a Cloud Run service or job
/// write, booting or reconfiguring a machine, acting as or minting tokens for another service
/// account, changing any IAM policy or a custom role, and starting code-carrying builds or
/// engines.
fn gate_only(permission: &str) -> bool {
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
            "compute.instances.create",
            "compute.instances.setMetadata",
            "compute.instances.setServiceAccount",
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
        ]
        .contains(&permission)
}

#[test]
fn protected_management_holds_nothing_only_the_gate_may_do() {
    let mut removed = BTreeSet::new();
    for id in management_set_ids() {
        let unprotected = project_grants(&plan(id, false));
        let protected = project_grants(&plan(id, true));
        for (role_id, (permissions, condition)) in &protected {
            let held: Vec<&String> = permissions.iter().filter(|p| gate_only(p)).collect();
            assert!(held.is_empty(), "{id}: {role_id} still holds {held:?}");
            // A role keeps its ID and binding and only loses permissions, so setup updates it
            // in place.
            let (before, before_condition) = unprotected
                .get(role_id)
                .unwrap_or_else(|| panic!("{id}: {role_id} is new in the protected rendering"));
            assert_eq!(condition, before_condition, "{id}: {role_id}");
            assert!(permissions.is_subset(before), "{id}: {role_id}");
        }
        for (role_id, (before, _)) in &unprotected {
            let kept = protected
                .get(role_id)
                .map(|(p, _)| p.clone())
                .unwrap_or_default();
            for permission in before.difference(&kept) {
                assert!(
                    gate_only(permission),
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
            "cloudbuild.builds.create",
            "cloudbuild.builds.update",
            "compute.instanceTemplates.create",
            "compute.instances.create",
            "compute.instances.setMetadata",
            "iam.roles.create",
            "iam.roles.undelete",
            "iam.roles.update",
            "iam.serviceAccounts.actAs",
            "iam.serviceAccounts.setIamPolicy",
            "pubsub.subscriptions.setIamPolicy",
            "pubsub.topics.setIamPolicy",
            "resourcemanager.projects.setIamPolicy",
            "run.services.create",
            "run.services.createTagBinding",
            "run.services.delete",
            "run.services.deleteTagBinding",
            "run.services.setIamPolicy",
            "run.services.update",
            "storage.buckets.setIamPolicy",
            "storage.managedFolders.setIamPolicy",
            "artifactregistry.repositories.setIamPolicy",
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
        // MIG resize and recreating instances from the gate's templates.
        "compute.instanceGroupManagers.update",
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
    assert_eq!(error.code, "PROTECTED_MANAGEMENT_ROLE_REFUSED");
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
fn only_machines_read_the_anchor_and_nobody_but_setup_writes_it() {
    let anchor = format!("projects/{PROJECT_NUMBER}/secrets/_release-trust-{PREFIX}");
    let inputs = format!("projects/{PROJECT_NUMBER}/secrets/_deployer-inputs-{PREFIX}");
    let node = [plan("compute-cluster/execute", false)];
    for secret in [&anchor, &inputs] {
        assert!(
            allowed(
                &node,
                "secretmanager.versions.access",
                &format!("{secret}/versions/1"),
                VERSION
            ),
            "{secret}"
        );
    }
    // Another deployment whose prefix extends this one has its own anchor.
    let other = format!("projects/{PROJECT_NUMBER}/secrets/_release-trust-{PREFIX}-eu/versions/1");
    assert!(!allowed(
        &node,
        "secretmanager.versions.access",
        &other,
        VERSION
    ));
    // A secret of the deployment's own is not the node's to read through this grant.
    let own = format!("projects/{PROJECT_NUMBER}/secrets/{PREFIX}-vault-key/versions/1");
    assert!(!allowed(
        &node,
        "secretmanager.versions.access",
        &own,
        VERSION
    ));

    for protected in [false, true] {
        let management: Vec<GcpGrantPlan> = management_set_ids()
            .into_iter()
            .map(|id| plan(id, protected))
            .collect();
        // The grants do reach the deployment's own secrets, so the conditions are evaluated.
        let own_secret = format!("projects/{PROJECT_NUMBER}/secrets/{PREFIX}-otlp");
        assert!(allowed(
            &management,
            "secretmanager.secrets.delete",
            &own_secret,
            SECRET
        ));
        for secret in [&anchor, &inputs] {
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
                    "protected={protected}: {permission} on {name}"
                );
            }
        }
    }
}
