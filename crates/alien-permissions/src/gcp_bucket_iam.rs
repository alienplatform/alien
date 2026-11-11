//! What a Cloud Storage bucket's IAM policy may grant, derived from the stack: the one rule the
//! controller that writes it and the verifier that checks it share.
//!
//! A new policy may keep or remove any grant the bucket holds now. It may add exactly one thing,
//! and only when the stack declares it: anonymous reads of the bucket's objects for a storage that
//! is `public_read`. Every other principal's access comes from the grants setup renders.

use serde_json::{json, Value};

/// The role that lets anyone read the bucket's objects.
pub const PUBLIC_VIEWER_ROLE: &str = "roles/storage.objectViewer";
/// The member that stands for everyone.
pub const PUBLIC_MEMBER: &str = "allUsers";

/// Why `new`, the policy a controller wants to set, is not one the stack allows given `current`,
/// the policy the bucket holds, or `Ok` when every grant in it is already held or is the public
/// read the stack declares.
pub fn check_bucket_iam(new: &Value, current: &Value, public_read: bool) -> Result<(), String> {
    let new = new
        .as_object()
        .ok_or("the bucket IAM policy is not a JSON object")?;
    if let Some(key) = new.keys().find(|key| {
        !matches!(
            key.as_str(),
            "version" | "kind" | "resourceId" | "bindings" | "etag"
        )
    }) {
        return Err(format!("the bucket IAM policy has an unknown field '{key}'"));
    }
    let held = bindings(current)?;
    for binding in bindings(&Value::Object(new.clone()))? {
        let (role, condition, members) = binding;
        for member in members {
            let already_held = held.iter().any(|(held_role, held_condition, held_members)| {
                *held_role == role && *held_condition == condition && held_members.contains(&member)
            });
            let declared_public = public_read
                && role == PUBLIC_VIEWER_ROLE
                && member == PUBLIC_MEMBER
                && condition.is_null();
            if !already_held && !declared_public {
                return Err(format!(
                    "the bucket IAM policy grants {role} to {member}, which the bucket does not \
                     hold and the signed stack does not declare"
                ));
            }
        }
    }
    Ok(())
}

/// `(role, condition, members)` of each binding of `policy`.
type ParsedBinding = (String, Value, Vec<String>);

fn bindings(policy: &Value) -> Result<Vec<ParsedBinding>, String> {
    let Some(list) = policy.get("bindings") else {
        return Ok(Vec::new());
    };
    let list = list.as_array().ok_or("the policy's bindings are not a list")?;
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
        check_bucket_iam(&current(), &current(), false).expect("unchanged");
    }

    #[test]
    fn removing_grants_is_free() {
        let new = json!({"bindings": [
            {"role": "roles/storage.objectAdmin", "members": ["serviceAccount:app@p.iam"]}]});
        check_bucket_iam(&new, &current(), false).expect("removal");
        check_bucket_iam(&json!({}), &current(), false).expect("empty");
    }

    #[test]
    fn public_read_is_accepted_only_when_declared() {
        let mut new = current();
        new["bindings"]
            .as_array_mut()
            .unwrap()
            .push(public_viewer_binding());
        check_bucket_iam(&new, &current(), true).expect("declared");
        let error = check_bucket_iam(&new, &current(), false).expect_err("not declared");
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
            new["bindings"].as_array_mut().unwrap().push(binding.clone());
            check_bucket_iam(&new, &current(), true).expect_err(&binding.to_string());
        }
    }

    #[test]
    fn an_unknown_field_or_malformed_binding_is_refused() {
        check_bucket_iam(&json!({"auditConfigs": []}), &current(), false).expect_err("field");
        check_bucket_iam(&json!({"bindings": [{"members": []}]}), &current(), false)
            .expect_err("no role");
        check_bucket_iam(&json!("x"), &current(), false).expect_err("not an object");
    }

    #[test]
    fn public_access_prevention_follows_the_declaration() {
        check_public_access_prevention("enforced", false).expect("enforced");
        check_public_access_prevention("inherited", true).expect("declared");
        check_public_access_prevention("inherited", false).expect_err("not declared");
        check_public_access_prevention("unspecified", true).expect_err("unknown");
    }
}
