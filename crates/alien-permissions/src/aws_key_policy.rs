//! Key policy for setup-owned AWS KMS keys.
//!
//! The account root keeps full administration, as KMS requires. The management
//! role gets explicit denies, so it can't read data protected by the key even if
//! an IAM policy later grants it more. Explicit denies in the key policy also
//! override grants.

use alien_core::{Container, Postgres, Stack, Storage};
use serde_json::{json, Value};

/// Actions that return plaintext, move ciphertext to another key, or hand key use
/// to someone else.
const MANAGEMENT_DENIED_ACTIONS: [&str; 5] = [
    "kms:Decrypt",
    "kms:GenerateDataKey*",
    "kms:ReEncrypt*",
    "kms:CreateGrant",
    "kms:PutKeyPolicy",
];

/// Encryption context the `key/availability` probe sends. Encrypt is allowed
/// only with this context, so the management role can't create ciphertext
/// that the application would accept.
const AVAILABILITY_PROBE_CONTEXT_KEY: &str = "kms:EncryptionContext:deployment-purpose";
const AVAILABILITY_PROBE_CONTEXT_VALUE: &str = "key-health";

/// Builds the key policy document.
///
/// `account_root_arn` and `management_role_arn` are template values (a string or
/// an intrinsic function object). `native_service_use` is true when a stack
/// resource encrypts with the key through an AWS service (EBS, RDS, S3). Those
/// services call KMS on the management role's behalf while it provisions them,
/// so calls made through an AWS service are exempt from the deny. They never
/// return plaintext to the caller.
pub fn aws_key_policy(
    account_root_arn: Value,
    management_role_arn: Option<Value>,
    native_service_use: bool,
) -> Value {
    let mut statements = vec![json!({
        "Sid": "EnableAccountAdministration",
        "Effect": "Allow",
        "Principal": { "AWS": account_root_arn },
        "Action": "kms:*",
        "Resource": "*",
    })];

    if let Some(role_arn) = management_role_arn {
        let direct_call_only = |mut condition: serde_json::Map<String, Value>| {
            if native_service_use {
                condition.insert("Null".to_string(), json!({ "kms:ViaService": "true" }));
            }
            condition
        };
        let principal = |mut condition: serde_json::Map<String, Value>| {
            condition.insert(
                "ArnEquals".to_string(),
                json!({ "aws:PrincipalArn": role_arn.clone() }),
            );
            Value::Object(condition)
        };

        statements.push(json!({
            "Sid": "DenyManagementKeyUse",
            "Effect": "Deny",
            "Principal": { "AWS": "*" },
            "Action": MANAGEMENT_DENIED_ACTIONS,
            "Resource": "*",
            "Condition": principal(direct_call_only(serde_json::Map::new())),
        }));
        statements.push(json!({
            "Sid": "DenyManagementEncryptOutsideAvailabilityProbe",
            "Effect": "Deny",
            "Principal": { "AWS": "*" },
            "Action": "kms:Encrypt",
            "Resource": "*",
            "Condition": principal(direct_call_only(serde_json::Map::from_iter([(
                "StringNotEquals".to_string(),
                json!({ AVAILABILITY_PROBE_CONTEXT_KEY: AVAILABILITY_PROBE_CONTEXT_VALUE }),
            )]))),
        }));
    }

    json!({ "Version": "2012-10-17", "Statement": statements })
}

/// Whether a stack resource uses the key for native AWS service encryption.
pub fn key_has_native_service_use(stack: &Stack, key_id: &str) -> bool {
    stack.resources().any(|(_, entry)| {
        let key = if let Some(storage) = entry.config.downcast_ref::<Storage>() {
            storage.encryption_key.as_ref()
        } else if let Some(postgres) = entry.config.downcast_ref::<Postgres>() {
            postgres.encryption_key.as_ref()
        } else if let Some(container) = entry.config.downcast_ref::<Container>() {
            container
                .persistent_storage
                .as_ref()
                .and_then(|storage| storage.encryption_key.as_ref())
        } else {
            None
        };
        key.is_some_and(|key| key.id() == key_id)
    })
}
