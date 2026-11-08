//! Key policy for setup-owned AWS KMS keys.
//!
//! The account root keeps full administration, as KMS requires. The management
//! role gets explicit denies, so it can't read data protected by the key even if
//! an IAM policy later grants it more. Explicit denies in the key policy also
//! override grants.
//!
//! Denying only the management role is not enough on its own: management may
//! create and retrust roles named after the stack, grant such a role
//! `kms:Decrypt`, assume it and decrypt. A key that no AWS service uses
//! therefore also gets an allowlist: use of the key is denied to every principal
//! except the runtime roles that setup created for it.
//!
//! The same path works through key administration: a role created under the
//! stack's name prefix can be granted `kms:PutKeyPolicy`, assumed, and used to
//! rewrite this policy. The administration actions are therefore denied to every
//! role under the stack's name prefix, with no exemption, because runtime roles
//! never administer a key. Recovery goes through the account root and IAM
//! principals outside that prefix, which the key policy still trusts.

use crate::{registry::permission_set_uses_aws_key_data, ErrorData, Result};
use alien_core::{Container, PermissionSetReference, Postgres, ServiceAccount, Stack, Storage};
use alien_error::AlienError;
use serde_json::{json, Map, Value};

/// Administration actions the management role never needs on a setup-owned key.
/// The account root can still change the key policy, so recovery stays possible.
const MANAGEMENT_DENIED_ADMIN_ACTIONS: [&str; 7] = [
    "kms:PutKeyPolicy",
    "kms:ScheduleKeyDeletion",
    "kms:DisableKey",
    "kms:DisableKeyRotation",
    "kms:CreateAlias",
    "kms:UpdateAlias",
    "kms:CreateGrant",
];

/// Administration actions denied to every role under the stack's name prefix.
/// Grants are left out: services such as EBS create them for the principal that
/// attaches an encrypted volume, and a key with no native service use already
/// denies `kms:CreateGrant` to everyone outside its runtime roles.
const STACK_ROLE_DENIED_ADMIN_ACTIONS: [&str; 6] = [
    "kms:PutKeyPolicy",
    "kms:ScheduleKeyDeletion",
    "kms:DisableKey",
    "kms:DisableKeyRotation",
    "kms:CreateAlias",
    "kms:UpdateAlias",
];

/// Actions that return plaintext, move ciphertext to another key, or hand key
/// use to someone else.
const KEY_USE_DENIED_ACTIONS: [&str; 4] = [
    "kms:Decrypt",
    "kms:GenerateDataKey*",
    "kms:ReEncrypt*",
    "kms:CreateGrant",
];

/// AWS services that call KMS for the management role while it provisions
/// volumes, databases and buckets that encrypt with a data-layer key.
const NATIVE_ENCRYPTION_SERVICES: [&str; 3] = [
    "ec2.*.amazonaws.com",
    "rds.*.amazonaws.com",
    "s3.*.amazonaws.com",
];

/// Encryption context the `key/availability` probe sends. Encrypt is allowed
/// only with this context, so the management role can't create ciphertext
/// that the application would accept.
const AVAILABILITY_PROBE_CONTEXT_KEY: &str = "kms:EncryptionContext:deployment-purpose";
const AVAILABILITY_PROBE_CONTEXT_VALUE: &str = "key-health";

/// Builds the key policy document.
///
/// `account_root_arn`, `management_role_arn`, `stack_role_arn_pattern` and
/// `runtime_role_arns` are template values (a string or an intrinsic function
/// object). `stack_role_arn_pattern` is an ARN pattern matching every IAM role
/// whose name starts with the stack's name prefix, such as
/// `arn:aws:iam::123456789012:role/my-stack-*`.
///
/// `native_service_use` is true when a stack resource encrypts with the key
/// through an AWS service (EBS, RDS, S3). Those services call KMS for the
/// management role while it provisions them, so the key-use denies don't apply
/// to calls made through EC2, RDS or S3. The service keeps the data key; the
/// caller never receives it. The management role can still make those services
/// read or write data they encrypt with the key. Such a key has no allowlist,
/// because the customer's own principals read the data it protects.
///
/// Otherwise only the runtime uses the key, and `runtime_role_arns` names the
/// roles that may. An empty list gives no allowlist, so only the
/// management-role denies apply.
pub fn aws_key_policy(
    account_root_arn: Value,
    management_role_arn: Option<Value>,
    stack_role_arn_pattern: Value,
    native_service_use: bool,
    runtime_role_arns: Vec<Value>,
) -> Value {
    let mut statements = vec![json!({
        "Sid": "EnableAccountAdministration",
        "Effect": "Allow",
        "Principal": { "AWS": account_root_arn },
        "Action": "kms:*",
        "Resource": "*",
    })];

    let allowlist = !native_service_use && !runtime_role_arns.is_empty();

    if let Some(role_arn) = management_role_arn {
        let is_management = || {
            Map::from_iter([(
                "ArnEquals".to_string(),
                json!({ "aws:PrincipalArn": role_arn.clone() }),
            )])
        };
        statements.push(json!({
            "Sid": "DenyManagementKeyAdministration",
            "Effect": "Deny",
            "Principal": { "AWS": "*" },
            "Action": MANAGEMENT_DENIED_ADMIN_ACTIONS,
            "Resource": "*",
            "Condition": is_management(),
        }));
        statements.push(json!({
            "Sid": "DenyStackRoleKeyAdministration",
            "Effect": "Deny",
            "Principal": { "AWS": "*" },
            "Action": STACK_ROLE_DENIED_ADMIN_ACTIONS,
            "Resource": "*",
            "Condition": { "ArnLike": { "aws:PrincipalArn": stack_role_arn_pattern } },
        }));
        if !allowlist {
            let mut use_condition = is_management();
            if native_service_use {
                use_condition.insert(
                    "StringNotLike".to_string(),
                    json!({ "kms:ViaService": NATIVE_ENCRYPTION_SERVICES }),
                );
            }
            let mut encrypt_condition = use_condition.clone();
            encrypt_condition.insert(
                "StringNotEquals".to_string(),
                json!({ AVAILABILITY_PROBE_CONTEXT_KEY: AVAILABILITY_PROBE_CONTEXT_VALUE }),
            );
            statements.push(json!({
                "Sid": "DenyManagementKeyUse",
                "Effect": "Deny",
                "Principal": { "AWS": "*" },
                "Action": KEY_USE_DENIED_ACTIONS,
                "Resource": "*",
                "Condition": use_condition,
            }));
            statements.push(json!({
                "Sid": "DenyManagementEncryptOutsideAvailabilityProbe",
                "Effect": "Deny",
                "Principal": { "AWS": "*" },
                "Action": "kms:Encrypt",
                "Resource": "*",
                "Condition": encrypt_condition,
            }));
        }
    }

    if allowlist {
        let outside_runtime = json!({ "aws:PrincipalArn": runtime_role_arns });
        statements.push(json!({
            "Sid": "DenyKeyUseOutsideRuntime",
            "Effect": "Deny",
            "Principal": { "AWS": "*" },
            "Action": KEY_USE_DENIED_ACTIONS,
            "Resource": "*",
            "Condition": { "ArnNotEquals": outside_runtime.clone() },
        }));
        statements.push(json!({
            "Sid": "DenyEncryptOutsideRuntimeAndAvailabilityProbe",
            "Effect": "Deny",
            "Principal": { "AWS": "*" },
            "Action": "kms:Encrypt",
            "Resource": "*",
            "Condition": {
                "ArnNotEquals": outside_runtime,
                "StringNotEquals": { AVAILABILITY_PROBE_CONTEXT_KEY: AVAILABILITY_PROBE_CONTEXT_VALUE },
            },
        }));
    }

    json!({ "Version": "2012-10-17", "Statement": statements })
}

/// Ids of the service accounts whose runtime roles may use the key: those with
/// any grant scoped to the key, or a stack-wide grant that allows KMS key use.
/// Stack-wide grants are judged by the actions they allow, not by their names.
///
/// A stack-wide grant that can't be resolved is an error. Treating it as "not a
/// key grant" could drop the only runtime role from the allowlist and leave a
/// policy that silently protects against the management role alone.
pub fn key_runtime_service_accounts(stack: &Stack, key_id: &str) -> Result<Vec<String>> {
    let reference_uses_keys = |reference: &PermissionSetReference| -> Result<bool> {
        let set = reference
            .resolve(|name| crate::get_permission_set(name).cloned())
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidPermissionSet {
                    message: format!(
                        "Permission set '{}' is not defined, so it can't be checked for KMS key use",
                        reference.id()
                    ),
                })
            })?;
        Ok(permission_set_uses_aws_key_data(&set))
    };
    let mut accounts = Vec::new();
    for (_, entry) in stack.resources() {
        let Some(account) = entry.config.downcast_ref::<ServiceAccount>() else {
            continue;
        };
        let profile = stack
            .permission_profiles()
            .get(account.id.strip_suffix("-sa").unwrap_or(&account.id));
        let mut uses_key = account.resource_permission_sets.contains_key(key_id)
            || account
                .stack_permission_sets
                .iter()
                .any(permission_set_uses_aws_key_data);
        if let Some(profile) = profile {
            uses_key |= profile.0.get(key_id).is_some_and(|refs| !refs.is_empty());
            if let Some(refs) = profile.0.get("*") {
                for reference in refs {
                    uses_key |= reference_uses_keys(reference)?;
                }
            }
        }
        if uses_key {
            accounts.push(account.id.clone());
        }
    }
    Ok(accounts)
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Key, PermissionProfile, PermissionSetReference, ResourceLifecycle};

    fn statement<'a>(policy: &'a Value, sid: &str) -> Option<&'a Value> {
        policy["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .find(|statement| statement["Sid"] == sid)
    }

    fn policy(native_service_use: bool, runtime: &[&str]) -> Value {
        aws_key_policy(
            json!("root"),
            Some(json!("management")),
            json!("stack-roles-*"),
            native_service_use,
            runtime.iter().map(|arn| json!(arn)).collect(),
        )
    }

    #[test]
    fn a_runtime_only_key_denies_everyone_but_its_runtime_roles() {
        let policy = policy(false, &["runtime-a", "runtime-b"]);

        let outside = statement(&policy, "DenyKeyUseOutsideRuntime").unwrap();
        assert_eq!(outside["Principal"], json!({ "AWS": "*" }));
        assert_eq!(
            outside["Condition"],
            json!({ "ArnNotEquals": { "aws:PrincipalArn": ["runtime-a", "runtime-b"] } })
        );
        let actions = outside["Action"].as_array().unwrap();
        for action in [
            "kms:Decrypt",
            "kms:GenerateDataKey*",
            "kms:ReEncrypt*",
            "kms:CreateGrant",
        ] {
            assert!(actions.contains(&json!(action)), "{action}");
        }
        // Encrypt stays open only for runtime roles and the availability probe.
        let encrypt = statement(&policy, "DenyEncryptOutsideRuntimeAndAvailabilityProbe").unwrap();
        assert_eq!(encrypt["Action"], json!("kms:Encrypt"));
        assert_eq!(
            encrypt["Condition"]["StringNotEquals"],
            json!({ "kms:EncryptionContext:deployment-purpose": "key-health" })
        );
        // The root account may still administer the key, so recovery needs no edit.
        let admin = statement(&policy, "DenyManagementKeyAdministration").unwrap();
        assert!(!outside["Action"]
            .as_array()
            .unwrap()
            .contains(&json!("kms:PutKeyPolicy")));
        assert_eq!(
            admin["Condition"],
            json!({ "ArnEquals": { "aws:PrincipalArn": "management" } })
        );
        for action in [
            "kms:PutKeyPolicy",
            "kms:ScheduleKeyDeletion",
            "kms:DisableKey",
            "kms:DisableKeyRotation",
            "kms:CreateAlias",
            "kms:UpdateAlias",
        ] {
            assert!(
                admin["Action"].as_array().unwrap().contains(&json!(action)),
                "{action}"
            );
        }
        assert!(statement(&policy, "DenyManagementKeyUse").is_none());
    }

    #[test]
    fn roles_under_the_stack_prefix_cannot_administer_the_key() {
        // The management role can create `<stack>-x`, grant it kms:PutKeyPolicy
        // and assume it, so the deny has to match the prefix and not one ARN.
        for (native, runtime) in [
            (false, vec!["runtime"]),
            (false, vec![]),
            (true, vec!["runtime"]),
        ] {
            let policy = policy(native, &runtime);
            let deny = statement(&policy, "DenyStackRoleKeyAdministration").unwrap();
            assert_eq!(deny["Effect"], json!("Deny"));
            assert_eq!(deny["Principal"], json!({ "AWS": "*" }));
            assert_eq!(
                deny["Condition"],
                json!({ "ArnLike": { "aws:PrincipalArn": "stack-roles-*" } }),
                "no StringNotEquals/ArnNotEquals exemption may sit next to the prefix match"
            );
            let actions = deny["Action"].as_array().unwrap();
            for action in [
                "kms:PutKeyPolicy",
                "kms:ScheduleKeyDeletion",
                "kms:DisableKey",
                "kms:DisableKeyRotation",
                "kms:CreateAlias",
                "kms:UpdateAlias",
            ] {
                assert!(actions.contains(&json!(action)), "{action}");
            }
            // Runtime roles use the key; they never administer it.
            for action in ["kms:Decrypt", "kms:Encrypt", "kms:GenerateDataKey*"] {
                assert!(!actions.contains(&json!(action)), "{action}");
            }
        }
    }

    #[test]
    fn the_account_root_is_never_denied_administration() {
        let policy = policy(false, &["runtime"]);
        let root_allow = statement(&policy, "EnableAccountAdministration").unwrap();
        assert_eq!(root_allow["Effect"], json!("Allow"));
        assert_eq!(root_allow["Principal"], json!({ "AWS": "root" }));
        assert_eq!(root_allow["Action"], json!("kms:*"));
        // Every administration deny is conditioned on a role ARN match, so the
        // root principal (which has no role ARN) can't be caught by it.
        for sid in [
            "DenyManagementKeyAdministration",
            "DenyStackRoleKeyAdministration",
        ] {
            let deny = statement(&policy, sid).unwrap();
            let condition = deny["Condition"].as_object().unwrap();
            assert!(
                condition.contains_key("ArnEquals") || condition.contains_key("ArnLike"),
                "{sid}"
            );
        }
    }

    #[test]
    fn a_key_without_runtime_roles_denies_only_the_management_role() {
        let policy = policy(false, &[]);
        assert!(statement(&policy, "DenyKeyUseOutsideRuntime").is_none());
        let direct = statement(&policy, "DenyManagementKeyUse").unwrap();
        assert_eq!(
            direct["Condition"],
            json!({ "ArnEquals": { "aws:PrincipalArn": "management" } })
        );
        assert!(statement(&policy, "DenyManagementEncryptOutsideAvailabilityProbe").is_some());
    }

    #[test]
    fn a_data_key_exempts_only_the_native_encryption_services() {
        let policy = policy(true, &["runtime"]);
        assert!(statement(&policy, "DenyKeyUseOutsideRuntime").is_none());
        let direct = statement(&policy, "DenyManagementKeyUse").unwrap();
        assert_eq!(
            direct["Condition"]["StringNotLike"],
            json!({ "kms:ViaService": ["ec2.*.amazonaws.com", "rds.*.amazonaws.com", "s3.*.amazonaws.com"] })
        );
        let admin = statement(&policy, "DenyManagementKeyAdministration").unwrap();
        assert!(admin["Condition"].get("StringNotLike").is_none());
    }

    #[test]
    fn runtime_roles_are_the_service_accounts_granted_the_key() {
        let stack = Stack::new("stack".to_string())
            .add(
                Key::new("secrets".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceAccount::new("manager-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceAccount::new("other-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceAccount::new("wild-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .permissions(
                alien_core::PermissionsConfig::new()
                    .with_profile(
                        "manager",
                        PermissionProfile::new().resource(
                            "secrets",
                            [PermissionSetReference::from_name("key/remote-cryptography")],
                        ),
                    )
                    .with_profile(
                        "other",
                        PermissionProfile::new().resource(
                            "elsewhere",
                            [PermissionSetReference::from_name("key/remote-cryptography")],
                        ),
                    )
                    .with_profile(
                        "wild",
                        PermissionProfile::new()
                            .global([PermissionSetReference::from_name("key/remote-cryptography")]),
                    ),
            )
            .build();

        assert_eq!(
            key_runtime_service_accounts(&stack, "secrets").unwrap(),
            vec!["manager-sa".to_string(), "wild-sa".to_string()]
        );
        assert!(!key_has_native_service_use(&stack, "secrets"));
    }

    #[test]
    fn a_custom_stack_wide_grant_that_allows_decrypt_is_a_runtime_role() {
        let custom = |id: &str, action: &str| {
            PermissionSetReference::from_inline(
                serde_json::from_value(json!({
                    "id": id, "description": "test",
                    "platforms": { "aws": [{
                        "grant": { "actions": [action] },
                        "binding": { "stack": { "resources": ["*"] } }
                    }] }
                }))
                .unwrap(),
            )
        };
        let stack = Stack::new("stack".to_string())
            .add(
                Key::new("secrets".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceAccount::new("named-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceAccount::new("custom-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceAccount::new("unrelated-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .permissions(
                alien_core::PermissionsConfig::new()
                    .with_profile(
                        "named",
                        PermissionProfile::new()
                            .global([PermissionSetReference::from_name("key/remote-cryptography")]),
                    )
                    .with_profile(
                        "custom",
                        PermissionProfile::new().global([custom("custom/crypto", "kms:Decrypt")]),
                    )
                    .with_profile(
                        "unrelated",
                        PermissionProfile::new()
                            .global([custom("key/looks-like-a-key", "s3:GetObject")]),
                    ),
            )
            .build();

        assert_eq!(
            key_runtime_service_accounts(&stack, "secrets").unwrap(),
            vec!["named-sa".to_string(), "custom-sa".to_string()]
        );
    }

    #[test]
    fn a_stack_wide_grant_that_cannot_be_resolved_is_an_error() {
        let stack = Stack::new("stack".to_string())
            .add(
                Key::new("secrets".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceAccount::new("manager-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .permissions(
                alien_core::PermissionsConfig::new().with_profile(
                    "manager",
                    PermissionProfile::new()
                        .global([PermissionSetReference::from_name("key/not-a-real-set")]),
                ),
            )
            .build();

        let error = key_runtime_service_accounts(&stack, "secrets")
            .expect_err("an unknown grant must not silently drop the runtime role");
        assert_eq!(error.code, "INVALID_PERMISSION_SET");
        assert!(
            error.message.contains("key/not-a-real-set"),
            "{}",
            error.message
        );
    }
}
