//! What an SQS queue's policy may allow, derived from the stack: the one rule the controller
//! that writes a queue policy, the verifier that checks it and setup share.
//!
//! A policy may deny anything. It may allow only S3 to send the queue the notifications of the
//! stack's own storages that notify it, from the install's own account. Every other principal's
//! access comes from IAM grants the stack's permission profiles render.

use serde_json::{json, Value};

/// The statement that lets S3 deliver the notifications of `bucket_arns` to `queue_arn`.
pub fn storage_ingress_statement(
    sid: &str,
    queue_arn: &str,
    account: &str,
    bucket_arns: &[String],
) -> Value {
    json!({
        "Sid": sid,
        "Effect": "Allow",
        "Principal": {"Service": "s3.amazonaws.com"},
        "Action": "sqs:SendMessage",
        "Resource": queue_arn,
        "Condition": {
            "StringEquals": {"aws:SourceAccount": account},
            "ArnLike": {"aws:SourceArn": bucket_arns}
        }
    })
}

/// Why `policy` is not one the stack allows. `allowed_bucket_arns` are the buckets of the stack's
/// storages that notify this queue; an Allow must be [`storage_ingress_statement`] for some of
/// them (none, one or all), so a foreign account, the management role, another service, a
/// bucket the stack does not name, `NotPrincipal` and `NotAction` are all refused.
pub fn check_queue_policy(
    policy: &Value,
    sid: &str,
    queue_arn: &str,
    account: &str,
    allowed_bucket_arns: &[String],
) -> Result<(), String> {
    let policy = policy
        .as_object()
        .ok_or("the queue policy is not a JSON object")?;
    if let Some(key) = policy
        .keys()
        .find(|key| !matches!(key.as_str(), "Version" | "Id" | "Statement"))
    {
        return Err(format!("the queue policy has an unknown field '{key}'"));
    }
    let statements = match policy.get("Statement") {
        Some(Value::Array(statements)) => statements.iter().collect::<Vec<_>>(),
        Some(statement @ Value::Object(_)) => vec![statement],
        _ => return Err("the queue policy has no statements".to_string()),
    };
    for statement in statements {
        match statement.get("Effect").and_then(Value::as_str) {
            Some("Deny") => {}
            Some("Allow") => {
                let sources: Vec<String> = statement
                    .pointer("/Condition/ArnLike/aws:SourceArn")
                    .and_then(Value::as_array)
                    .map(|sources| {
                        sources
                            .iter()
                            .filter_map(|source| source.as_str().map(str::to_string))
                            .collect()
                    })
                    .ok_or_else(|| {
                        format!("the queue policy allows access without a source: {statement}")
                    })?;
                if let Some(foreign) = sources
                    .iter()
                    .find(|source| !allowed_bucket_arns.contains(source))
                {
                    return Err(format!(
                        "the queue policy lets {foreign} send to the queue, and the signed stack \
                         has no storage notifying this queue from there"
                    ));
                }
                if *statement != storage_ingress_statement(sid, queue_arn, account, &sources) {
                    return Err(format!(
                        "the queue policy allows something other than S3 notifications from the \
                         stack's storages: {statement}"
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "a queue policy statement has no valid Effect: {statement}"
                ))
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SID: &str = "AllowStorageIngress";
    const QUEUE: &str = "arn:aws:sqs:us-east-1:111122223333:acme-jobs";
    const ACCOUNT: &str = "111122223333";

    fn allowed() -> Vec<String> {
        vec!["arn:aws:s3:::acme-uploads".to_string()]
    }

    fn policy(statements: Vec<Value>) -> Value {
        json!({"Version": "2012-10-17", "Statement": statements})
    }

    #[test]
    fn the_ingress_the_controller_builds_is_accepted_unchanged() {
        let statement = storage_ingress_statement(SID, QUEUE, ACCOUNT, &allowed());
        check_queue_policy(&policy(vec![statement]), SID, QUEUE, ACCOUNT, &allowed())
            .expect("accepted");
        check_queue_policy(&policy(vec![]), SID, QUEUE, ACCOUNT, &allowed()).expect("empty");
    }

    #[test]
    fn anything_else_that_allows_is_refused() {
        let own = storage_ingress_statement(SID, QUEUE, ACCOUNT, &allowed());
        let mut foreign_account = own.clone();
        foreign_account["Condition"]["StringEquals"]["aws:SourceAccount"] = json!("999988887777");
        let foreign_bucket =
            storage_ingress_statement(SID, QUEUE, ACCOUNT, &["arn:aws:s3:::theirs".to_string()]);
        let mut wildcard_source = own.clone();
        wildcard_source["Condition"]["ArnLike"]["aws:SourceArn"] = json!(["arn:aws:s3:::*"]);
        let management = json!({
            "Effect": "Allow",
            "Principal": {"AWS": "arn:aws:iam::111122223333:role/acme-management"},
            "Action": "sqs:*", "Resource": QUEUE
        });
        let service_without_source = json!({
            "Effect": "Allow", "Principal": {"Service": "sns.amazonaws.com"},
            "Action": "sqs:SendMessage", "Resource": QUEUE
        });
        let anyone = json!({
            "Effect": "Allow", "Principal": "*", "Action": "sqs:SendMessage", "Resource": QUEUE
        });
        let mut not_principal = own.clone();
        not_principal.as_object_mut().unwrap().remove("Principal");
        not_principal["NotPrincipal"] = json!({"AWS": "arn:aws:iam::111122223333:root"});
        let mut not_action = own.clone();
        not_action.as_object_mut().unwrap().remove("Action");
        not_action["NotAction"] = json!("sqs:DeleteQueue");
        let mut wider_action = own;
        wider_action["Action"] = json!("sqs:*");
        for statement in [
            foreign_account,
            foreign_bucket,
            wildcard_source,
            management,
            service_without_source,
            anyone,
            not_principal,
            not_action,
            wider_action,
        ] {
            check_queue_policy(
                &policy(vec![statement.clone()]),
                SID,
                QUEUE,
                ACCOUNT,
                &allowed(),
            )
            .expect_err(&statement.to_string());
        }
    }

    #[test]
    fn a_deny_is_free_and_an_unknown_shape_is_refused() {
        let deny =
            json!({"Effect": "Deny", "NotPrincipal": "*", "Action": "sqs:*", "Resource": QUEUE});
        check_queue_policy(&policy(vec![deny]), SID, QUEUE, ACCOUNT, &allowed()).expect("deny");
        check_queue_policy(&json!([]), SID, QUEUE, ACCOUNT, &allowed()).expect_err("shape");
        check_queue_policy(
            &json!({"Statement": [], "X": 1}),
            SID,
            QUEUE,
            ACCOUNT,
            &allowed(),
        )
        .expect_err("field");
    }
}
