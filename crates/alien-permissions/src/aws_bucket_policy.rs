//! What an S3 bucket policy may allow, derived from the stack: the one rule the controller that
//! writes a bucket policy, the verifier that checks it and the setup emitters share.
//!
//! A policy may deny anything. It may allow only what the stack declares: anonymous reads of the
//! bucket's objects when the storage is `public_read`, and nothing else. Every other principal's
//! access comes from IAM grants the stack's permission profiles render, not from the bucket
//! policy.

use serde_json::{json, Value};

/// The statement that lets anyone read the bucket's objects, for a storage declared `public_read`.
pub fn public_read_statement(bucket_arn: &str) -> Value {
    json!({
        "Sid": "AllowPublicRead",
        "Effect": "Allow",
        "Principal": "*",
        "Action": "s3:GetObject",
        "Resource": format!("{bucket_arn}/*"),
    })
}

/// Why `policy` is not one the stack allows, or `Ok` when every Allow in it is the statement the
/// stack declares for `bucket_arn`.
///
/// Deny statements are free: they only remove access. An Allow must equal
/// [`public_read_statement`] and only when `public_read` is declared, which also refuses
/// `NotPrincipal`, `NotAction`, a foreign principal, a service principal and a condition.
pub fn check_bucket_policy(
    policy: &Value,
    bucket_arn: &str,
    public_read: bool,
) -> Result<(), String> {
    let policy = policy
        .as_object()
        .ok_or("the bucket policy is not a JSON object")?;
    if let Some(key) = policy
        .keys()
        .find(|key| !matches!(key.as_str(), "Version" | "Id" | "Statement"))
    {
        return Err(format!("the bucket policy has an unknown field '{key}'"));
    }
    let statements = match policy.get("Statement") {
        Some(Value::Array(statements)) => statements.iter().collect::<Vec<_>>(),
        Some(statement @ Value::Object(_)) => vec![statement],
        _ => return Err("the bucket policy has no statements".to_string()),
    };
    for statement in statements {
        match statement.get("Effect").and_then(Value::as_str) {
            Some("Deny") => {}
            Some("Allow") => {
                if !public_read {
                    return Err(format!(
                        "the bucket policy allows access ({statement}), and the signed stack does \
                         not declare this storage public"
                    ));
                }
                if *statement != public_read_statement(bucket_arn) {
                    return Err(format!(
                        "the bucket policy allows something other than anonymous reads of the \
                         bucket's objects: {statement}"
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "a bucket policy statement has no valid Effect: {statement}"
                ))
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUCKET: &str = "arn:aws:s3:::acme-assets";

    fn deny_insecure() -> Value {
        json!({
            "Sid": "DenyInsecureTransport", "Effect": "Deny", "Principal": "*",
            "Action": "s3:*", "Resource": [BUCKET, format!("{BUCKET}/*")],
            "Condition": {"Bool": {"aws:SecureTransport": "false"}}
        })
    }

    fn policy(statements: Vec<Value>) -> Value {
        json!({"Version": "2012-10-17", "Statement": statements})
    }

    #[test]
    fn the_policy_the_controller_builds_is_accepted_unchanged() {
        check_bucket_policy(&policy(vec![deny_insecure()]), BUCKET, false).expect("private");
        check_bucket_policy(
            &policy(vec![deny_insecure(), public_read_statement(BUCKET)]),
            BUCKET,
            true,
        )
        .expect("public read, declared");
    }

    #[test]
    fn a_deny_of_any_kind_is_accepted() {
        let deny_management = json!({
            "Effect": "Deny", "NotPrincipal": {"AWS": "arn:aws:iam::111122223333:role/m"},
            "Action": "s3:*", "Resource": BUCKET
        });
        check_bucket_policy(&policy(vec![deny_management]), BUCKET, false).expect("deny is free");
    }

    #[test]
    fn an_allow_the_stack_does_not_declare_is_refused() {
        let foreign_account = json!({
            "Effect": "Allow", "Principal": {"AWS": "arn:aws:iam::999988887777:root"},
            "Action": "s3:*", "Resource": [BUCKET, format!("{BUCKET}/*")]
        });
        let management = json!({
            "Effect": "Allow",
            "Principal": {"AWS": "arn:aws:iam::111122223333:role/acme-management"},
            "Action": "s3:PutObject", "Resource": format!("{BUCKET}/*")
        });
        let service_without_source = json!({
            "Effect": "Allow", "Principal": {"Service": "sns.amazonaws.com"},
            "Action": "s3:PutObject", "Resource": format!("{BUCKET}/*")
        });
        let not_principal = json!({
            "Effect": "Allow", "NotPrincipal": {"AWS": "arn:aws:iam::111122223333:root"},
            "Action": "s3:GetObject", "Resource": format!("{BUCKET}/*")
        });
        let not_action = json!({
            "Effect": "Allow", "Principal": "*",
            "NotAction": "s3:DeleteBucket", "Resource": format!("{BUCKET}/*")
        });
        for statement in [
            foreign_account,
            management,
            service_without_source,
            not_principal,
            not_action,
        ] {
            for public_read in [false, true] {
                check_bucket_policy(
                    &policy(vec![deny_insecure(), statement.clone()]),
                    BUCKET,
                    public_read,
                )
                .expect_err(&format!("{statement} (public_read={public_read})"));
            }
        }
    }

    #[test]
    fn a_wildcard_principal_needs_the_public_declaration() {
        let policy = policy(vec![deny_insecure(), public_read_statement(BUCKET)]);
        check_bucket_policy(&policy, BUCKET, false).expect_err("not declared public");
    }

    #[test]
    fn public_read_of_another_bucket_or_wider_actions_is_refused() {
        let mut other_bucket = public_read_statement("arn:aws:s3:::someone-else");
        check_bucket_policy(&policy(vec![other_bucket.clone()]), BUCKET, true)
            .expect_err("another bucket");
        other_bucket = public_read_statement(BUCKET);
        other_bucket["Action"] = json!("s3:*");
        check_bucket_policy(&policy(vec![other_bucket]), BUCKET, true).expect_err("wider action");
    }

    #[test]
    fn an_unknown_shape_is_refused() {
        check_bucket_policy(&json!("x"), BUCKET, false).expect_err("not an object");
        check_bucket_policy(&json!({"Version": "2012-10-17"}), BUCKET, false).expect_err("none");
        check_bucket_policy(&json!({"Statement": [], "Extra": 1}), BUCKET, false)
            .expect_err("unknown field");
        check_bucket_policy(&policy(vec![json!({"Effect": "Maybe"})]), BUCKET, false)
            .expect_err("bad effect");
    }
}
