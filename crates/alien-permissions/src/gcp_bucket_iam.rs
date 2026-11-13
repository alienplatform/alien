//! What a Cloud Storage bucket's IAM policy may grant, derived from the stack: the one rule the
//! controller that writes it and the verifier that checks it share.
//!
//! A new policy may keep or remove any grant the bucket holds now. It may add only what the signed
//! stack grants this bucket (see [`crate::gcp_resource_grants`]: its permission profiles' service
//! accounts and Remote Bindings) and anonymous reads of the bucket's objects for a storage that is
//! `public_read`. Nothing else: not management, the verifier or the setup identity.

use serde_json::{json, Value};

use crate::generators::gcp_runtime::GcpIamBinding;

/// The role that lets anyone read the bucket's objects.
pub const PUBLIC_VIEWER_ROLE: &str = "roles/storage.objectViewer";
/// The member that stands for everyone.
pub const PUBLIC_MEMBER: &str = "allUsers";

/// Why `new`, the policy a controller wants to set, is not one the stack allows given `current`,
/// the policy the bucket holds, or `Ok` when every grant in it is already held, is one of
/// `derived` (what the signed stack grants this bucket) or is the public read the stack declares.
pub fn check_bucket_iam(
    new: &Value,
    current: &Value,
    derived: &[GcpIamBinding],
    public_read: bool,
) -> Result<(), String> {
    let new = new
        .as_object()
        .ok_or("the bucket IAM policy is not a JSON object")?;
    if let Some(key) = new.keys().find(|key| {
        !matches!(
            key.as_str(),
            "version" | "kind" | "resourceId" | "bindings" | "etag"
        )
    }) {
        return Err(format!(
            "the bucket IAM policy has an unknown field '{key}'"
        ));
    }
    let held = bindings(current)?;
    for binding in bindings(&Value::Object(new.clone()))? {
        let (role, condition, members) = binding;
        for member in members {
            let already_held = held
                .iter()
                .any(|(held_role, held_condition, held_members)| {
                    *held_role == role
                        && *held_condition == condition
                        && held_members.contains(&member)
                });
            let derived_grant = derived.iter().any(|grant| {
                grant.role == role
                    && grant.members.contains(&member)
                    && same_condition(&condition, grant)
            });
            let declared_public = public_read
                && role == PUBLIC_VIEWER_ROLE
                && member == PUBLIC_MEMBER
                && condition.is_null();
            if !already_held && !derived_grant && !declared_public {
                return Err(format!(
                    "the bucket IAM policy grants {role} to {member}, which the bucket does not \
                     hold and the signed stack does not grant it"
                ));
            }
        }
    }
    Ok(())
}

/// Whether the condition of a policy binding (`Null` for none) is the one `grant` carries: the
/// same title and expression.
fn same_condition(condition: &Value, grant: &GcpIamBinding) -> bool {
    match (&grant.condition, condition) {
        (None, Value::Null) => true,
        (Some(grant), Value::Object(condition)) => {
            condition.get("title").and_then(Value::as_str) == Some(grant.title.as_str())
                && condition.get("expression").and_then(Value::as_str)
                    == Some(grant.expression.as_str())
        }
        _ => false,
    }
}

/// `(role, condition, members)` of each binding of `policy`.
type ParsedBinding = (String, Value, Vec<String>);

fn bindings(policy: &Value) -> Result<Vec<ParsedBinding>, String> {
    let Some(list) = policy.get("bindings") else {
        return Ok(Vec::new());
    };
    let list = list
        .as_array()
        .ok_or("the policy's bindings are not a list")?;
    list.iter()
        .map(|binding| {
            let role = binding
                .get("role")
                .and_then(Value::as_str)
                .ok_or("a binding has no role")?;
            let members = binding
                .get("members")
                .and_then(Value::as_array)
                .map(|members| {
                    members
                        .iter()
                        .map(|member| member.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                })
                .unwrap_or(Some(Vec::new()))
                .ok_or("a binding member is not a string")?;
            Ok((
                role.to_string(),
                binding.get("condition").cloned().unwrap_or(Value::Null),
                members,
            ))
        })
        .collect()
}

/// Why `value`, the public access prevention a controller wants to set, is not one the stack
/// allows: `enforced` is always fine; `inherited` lets the bucket serve anonymous reads, so only a
/// storage declared `public_read` may have it.
pub fn check_public_access_prevention(value: &str, public_read: bool) -> Result<(), String> {
    match value {
        "enforced" => Ok(()),
        "inherited" if public_read => Ok(()),
        "inherited" => Err(
            "public access prevention is set to inherited, and the signed stack does not declare \
             this storage public"
                .to_string(),
        ),
        other => Err(format!("unknown public access prevention '{other}'")),
    }
}

/// The grant that makes a bucket's objects public, for a storage declared `public_read`.
pub fn public_viewer_binding() -> Value {
    json!({"role": PUBLIC_VIEWER_ROLE, "members": [PUBLIC_MEMBER]})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> Value {
        json!({"etag": "a", "bindings": [
            {"role": "roles/storage.objectAdmin", "members": ["serviceAccount:app@p.iam"]},
            {"role": "roles/storage.objectViewer", "members": ["serviceAccount:app@p.iam"]},
            {"role": "roles/storage.objectViewer", "members": ["user:x@y.z"],
             "condition": {"title": "t", "expression": "true"}},
        ]})
    }

    #[test]
    fn the_policy_the_bucket_holds_is_accepted_unchanged() {
        check_bucket_iam(&current(), &current(), &[], false).expect("unchanged");
    }

    #[test]
    fn removing_grants_is_free() {
        let new = json!({"bindings": [
            {"role": "roles/storage.objectAdmin", "members": ["serviceAccount:app@p.iam"]}]});
        check_bucket_iam(&new, &current(), &[], false).expect("removal");
        check_bucket_iam(&json!({}), &current(), &[], false).expect("empty");
    }

    #[test]
    fn public_read_is_accepted_only_when_declared() {
        let mut new = current();
        new["bindings"]
            .as_array_mut()
            .unwrap()
            .push(public_viewer_binding());
        check_bucket_iam(&new, &current(), &[], true).expect("declared");
        let error = check_bucket_iam(&new, &current(), &[], false).expect_err("not declared");
        assert!(error.contains("allUsers"), "{error}");
    }

    #[test]
    fn a_new_member_or_role_is_refused_even_when_public_read_is_declared() {
        for binding in [
            json!({"role": "roles/storage.objectAdmin", "members": ["user:attacker@x.y"]}),
            json!({"role": "roles/storage.admin", "members": ["serviceAccount:app@p.iam"]}),
            json!({"role": "roles/storage.objectAdmin", "members": ["allUsers"]}),
            json!({"role": "roles/storage.objectViewer", "members": ["allAuthenticatedUsers"]}),
            json!({"role": "roles/storage.objectViewer", "members": ["allUsers"],
                   "condition": {"title": "t", "expression": "true"}}),
            // The held condition is for another member.
            json!({"role": "roles/storage.objectViewer", "members": ["serviceAccount:app@p.iam"],
                   "condition": {"title": "t", "expression": "true"}}),
        ] {
            let mut new = current();
            new["bindings"]
                .as_array_mut()
                .unwrap()
                .push(binding.clone());
            check_bucket_iam(&new, &current(), &[], true).expect_err(&binding.to_string());
        }
    }

    #[test]
    fn an_unknown_field_or_malformed_binding_is_refused() {
        check_bucket_iam(&json!({"auditConfigs": []}), &current(), &[], false).expect_err("field");
        check_bucket_iam(
            &json!({"bindings": [{"members": []}]}),
            &current(),
            &[],
            false,
        )
        .expect_err("no role");
        check_bucket_iam(&json!("x"), &current(), &[], false).expect_err("not an object");
    }

    #[test]
    fn public_access_prevention_follows_the_declaration() {
        check_public_access_prevention("enforced", false).expect("enforced");
        check_public_access_prevention("inherited", true).expect("declared");
        check_public_access_prevention("inherited", false).expect_err("not declared");
        check_public_access_prevention("unspecified", true).expect_err("unknown");
    }
}

#[cfg(test)]
mod derived_tests {
    use super::*;
    use crate::generators::gcp_runtime::GcpBindingTargetScope;

    fn grant(role: &str, member: &str) -> GcpIamBinding {
        GcpIamBinding {
            role: role.to_string(),
            members: vec![member.to_string()],
            target: GcpBindingTargetScope::CurrentResource,
            target_resource_name: None,
            resource_kind: None,
            condition: None,
        }
    }

    #[test]
    fn a_grant_the_stack_derives_is_accepted_and_nothing_else_is() {
        let derived = [grant(
            "projects/p/roles/app_storage_data_write",
            "serviceAccount:app@p.iam",
        )];
        let held = json!({});
        let new = json!({"bindings": [{"role": "projects/p/roles/app_storage_data_write",
            "members": ["serviceAccount:app@p.iam"]}]});
        check_bucket_iam(&new, &held, &derived, false).expect("derived grant");
        for (name, binding) in [
            (
                "a foreign member",
                json!({"role": "projects/p/roles/app_storage_data_write",
                "members": ["user:attacker@x.y"]}),
            ),
            // The derived role bound to the member of another resource (management, the verifier,
            // the setup identity or another profile's account).
            (
                "another account",
                json!({"role": "projects/p/roles/app_storage_data_write",
                "members": ["serviceAccount:other@p.iam"]}),
            ),
            (
                "another role",
                json!({"role": "roles/storage.admin",
                "members": ["serviceAccount:app@p.iam"]}),
            ),
            (
                "a condition the stack does not know",
                json!({
                "role": "projects/p/roles/app_storage_data_write",
                "members": ["serviceAccount:app@p.iam"],
                "condition": {"title": "t", "expression": "true"}}),
            ),
        ] {
            check_bucket_iam(
                &json!({"bindings": [binding.clone()]}),
                &held,
                &derived,
                true,
            )
            .expect_err(name);
        }
    }

    #[test]
    fn a_derived_condition_must_match_by_title_and_expression() {
        let mut conditioned = grant("projects/p/roles/r", "serviceAccount:app@p.iam");
        conditioned.condition = Some(crate::generators::gcp_runtime::GcpIamCondition {
            title: "t".to_string(),
            description: "d".to_string(),
            expression: "resource.name.startsWith('x')".to_string(),
        });
        let held = json!({});
        let with = |expression: &str| {
            json!({"bindings": [{"role": "projects/p/roles/r", "members": ["serviceAccount:app@p.iam"],
                "condition": {"title": "t", "description": "d", "expression": expression}}]})
        };
        check_bucket_iam(
            &with("resource.name.startsWith('x')"),
            &held,
            &[conditioned.clone()],
            false,
        )
        .expect("same condition");
        check_bucket_iam(&with("true"), &held, &[conditioned.clone()], false)
            .expect_err("other expression");
        // The unconditioned grant is not the conditioned one.
        let plain = json!({"bindings": [{"role": "projects/p/roles/r",
            "members": ["serviceAccount:app@p.iam"]}]});
        check_bucket_iam(&plain, &held, &[conditioned], false).expect_err("dropped condition");
    }
}
