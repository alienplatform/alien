//! AWS Storage — S3 bucket + bucket policy.
//!
//! Renders an `AWS::S3::Bucket` with encryption, ownership controls, and
//! public access block plus an `AWS::S3::BucketPolicy` enforcing TLS in
//! transit. Supports optional versioning, lifecycle rules, and an
//! optional public-read policy when `Storage::public_read` is set.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, required_logical_id, resource_config, service_account_role_id,
            stack_id_short_suffix, storage_notification_configuration,
            storage_notification_dependencies, tags, uniquify_iam_statement_sids,
        },
        service_account::permission_context,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, ownership_policy_for_resource_type, Email, ErrorData, PermissionProfile,
    PermissionSetReference, RemoteStackManagement, Result, ServiceAccount, Storage,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};
use indexmap::IndexMap;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsStorageEmitter;

impl CfEmitter for AwsStorageEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let storage = resource_config::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let bucket_id = required_logical_id(ctx)?;

        let mut bucket = CfResource::new(bucket_id.to_string(), "AWS::S3::Bucket".to_string());
        bucket
            .properties
            .insert("BucketName".to_string(), bucket_name(storage.id()));
        bucket.properties.insert(
            "BucketEncryption".to_string(),
            CfExpression::object([(
                "ServerSideEncryptionConfiguration",
                CfExpression::list([CfExpression::object([(
                    "ServerSideEncryptionByDefault",
                    CfExpression::object([("SSEAlgorithm", CfExpression::from("AES256"))]),
                )])]),
            )]),
        );
        bucket.properties.insert(
            "OwnershipControls".to_string(),
            CfExpression::object([(
                "Rules",
                CfExpression::list([CfExpression::object([(
                    "ObjectOwnership",
                    CfExpression::from("BucketOwnerEnforced"),
                )])]),
            )]),
        );
        bucket.properties.insert(
            "PublicAccessBlockConfiguration".to_string(),
            public_access_block(!storage.public_read),
        );
        if let Some(notification) = storage_notification_configuration(ctx)? {
            bucket
                .properties
                .insert("NotificationConfiguration".to_string(), notification);
            bucket
                .depends_on
                .extend(storage_notification_dependencies(ctx));
        }

        if storage.versioning {
            bucket.properties.insert(
                "VersioningConfiguration".to_string(),
                CfExpression::object([("Status", CfExpression::from("Enabled"))]),
            );
        }

        if !storage.lifecycle_rules.is_empty() {
            bucket.properties.insert(
                "LifecycleConfiguration".to_string(),
                CfExpression::object([(
                    "Rules",
                    CfExpression::list(storage.lifecycle_rules.iter().enumerate().map(
                        |(index, rule)| {
                            let mut lifecycle_rule = IndexMap::from([
                                (
                                    "Id".to_string(),
                                    CfExpression::from(format!("Rule{}", index + 1)),
                                ),
                                ("Status".to_string(), CfExpression::from("Enabled")),
                                (
                                    "ExpirationInDays".to_string(),
                                    CfExpression::Integer(i64::from(rule.days)),
                                ),
                            ]);
                            lifecycle_rule.insert(
                                "Prefix".to_string(),
                                CfExpression::from(rule.prefix.clone().unwrap_or_default()),
                            );
                            CfExpression::Object(lifecycle_rule)
                        },
                    )),
                )]),
            );
        }

        bucket.properties.insert("Tags".to_string(), tags(ctx));
        bucket.deletion_policy = Some("Retain".to_string());
        bucket.update_replace_policy = Some("Retain".to_string());

        let mut policy = CfResource::new(
            format!("{bucket_id}BucketPolicy"),
            "AWS::S3::BucketPolicy".to_string(),
        );
        policy
            .properties
            .insert("Bucket".to_string(), CfExpression::ref_(bucket_id));
        policy.properties.insert(
            "PolicyDocument".to_string(),
            bucket_policy_document(
                bucket_id,
                storage.public_read,
                email_inbound_targets_bucket(ctx),
            ),
        );

        let mut resources = vec![bucket, policy];
        resources.extend(storage_iam_policies(ctx, storage, bucket_id)?);

        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let bucket_id = required_logical_id(ctx)?;
        Ok(CfExpression::object([
            ("bucketName", CfExpression::ref_(bucket_id)),
            ("bucketArn", CfExpression::get_att(bucket_id, "Arn")),
        ]))
    }

    /// Every resource this emitter returns — bucket, policy, and the IAM
    /// policies that name the bucket — is stamped with the gate's condition by
    /// the generator, so they appear and disappear together.
    fn supports_enabled_when(&self) -> bool {
        true
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        resource_config::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let bucket_id = required_logical_id(ctx)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("s3")),
            ("bucketName", CfExpression::ref_(bucket_id)),
        ])))
    }
}

fn bucket_name(storage_id: &str) -> CfExpression {
    CfExpression::object([(
        "Fn::Join",
        CfExpression::list([
            CfExpression::from("-"),
            CfExpression::list([
                CfExpression::ref_("AWS::StackName"),
                CfExpression::from(storage_id),
                stack_id_short_suffix(),
            ]),
        ]),
    )])
}

fn public_access_block(block_public_access: bool) -> CfExpression {
    CfExpression::object([
        ("BlockPublicAcls", CfExpression::from(block_public_access)),
        ("BlockPublicPolicy", CfExpression::from(block_public_access)),
        ("IgnorePublicAcls", CfExpression::from(block_public_access)),
        (
            "RestrictPublicBuckets",
            CfExpression::from(block_public_access),
        ),
    ])
}

/// True when a setup-emitted Email resource delivers inbound mail into this
/// bucket. The SES write grant must live in this bucket policy — S3 supports
/// only one bucket policy resource per bucket, so the email emitter cannot
/// attach its own.
fn email_inbound_targets_bucket(ctx: &EmitContext<'_>) -> bool {
    ctx.stack.resources().any(|(_id, entry)| {
        let ownership = ownership_policy_for_resource_type(entry.config.resource_type().as_ref());
        if !ownership.should_emit_in_setup(entry.lifecycle) {
            return false;
        }
        let Some(email) = entry.config.downcast_ref::<Email>() else {
            return false;
        };
        email
            .inbound
            .as_ref()
            .is_some_and(|inbound| inbound.storage.id == ctx.resource_id)
    })
}

fn bucket_policy_document(
    bucket_id: &str,
    public_read: bool,
    allow_ses_inbound: bool,
) -> CfExpression {
    let bucket_arn = CfExpression::get_att(bucket_id, "Arn");
    let bucket_objects_arn =
        CfExpression::sub(format!("arn:${{AWS::Partition}}:s3:::${{{bucket_id}}}/*"));

    let mut statements = vec![CfExpression::object([
        ("Sid", CfExpression::from("DenyInsecureTransport")),
        ("Effect", CfExpression::from("Deny")),
        ("Principal", CfExpression::from("*")),
        ("Action", CfExpression::from("s3:*")),
        (
            "Resource",
            CfExpression::list([bucket_arn.clone(), bucket_objects_arn.clone()]),
        ),
        (
            "Condition",
            CfExpression::object([(
                "Bool",
                CfExpression::object([("aws:SecureTransport", CfExpression::from("false"))]),
            )]),
        ),
    ])];

    if public_read {
        statements.push(CfExpression::object([
            ("Sid", CfExpression::from("AllowPublicRead")),
            ("Effect", CfExpression::from("Allow")),
            ("Principal", CfExpression::from("*")),
            ("Action", CfExpression::from("s3:GetObject")),
            ("Resource", bucket_objects_arn.clone()),
        ]));
    }

    if allow_ses_inbound {
        // SES delivers received mail via its service principal; scope the
        // grant to this account per the SES receiving documentation.
        statements.push(CfExpression::object([
            ("Sid", CfExpression::from("AllowSesInboundDelivery")),
            ("Effect", CfExpression::from("Allow")),
            (
                "Principal",
                CfExpression::object([("Service", CfExpression::from("ses.amazonaws.com"))]),
            ),
            ("Action", CfExpression::from("s3:PutObject")),
            ("Resource", bucket_objects_arn),
            (
                "Condition",
                CfExpression::object([(
                    "StringEquals",
                    CfExpression::object([(
                        "aws:SourceAccount",
                        CfExpression::ref_("AWS::AccountId"),
                    )]),
                )]),
            ),
        ]));
    }

    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        ("Statement", CfExpression::list(statements)),
    ])
}

fn storage_iam_policies(
    ctx: &EmitContext<'_>,
    storage: &Storage,
    bucket_id: &str,
) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context =
        permission_context().with_resource_name(format!("${{AWS::StackName}}-{}", storage.id()));

    for (owner_index, (role_id, permission_refs)) in storage_permission_owners(ctx) {
        for (permission_index, permission_ref) in permission_refs.iter().enumerate() {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with("storage/") {
                continue;
            }

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, &context)
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate AWS CloudFormation storage IAM policy for '{}'",
                        storage.id()
                    ),
                })?;
            let policy_value = serde_json::to_value(policy).into_alien_error().context(
                ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "Failed to serialize IAM policy".to_string(),
                },
            )?;
            let CfExpression::Object(mut policy_object) = cf_from_json(policy_value)? else {
                return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "policy did not serialize to a JSON object".to_string(),
                }));
            };
            let Some(CfExpression::List(policy_statements)) =
                policy_object.shift_remove("Statement")
            else {
                continue;
            };
            let policy_statements = policy_statements
                .into_iter()
                .map(|statement| storage_policy_statement_for_bucket(statement, bucket_id))
                .collect::<Vec<_>>();

            let policy_id =
                format!("{bucket_id}{role_id}StoragePermission{owner_index}{permission_index}");
            let mut policy_resource = CfResource::new(policy_id, "AWS::IAM::Policy".to_string());
            policy_resource.properties.insert(
                "PolicyName".to_string(),
                CfExpression::sub(format!(
                    "${{AWS::StackName}}-{}-storage-{owner_index}-{permission_index}",
                    storage.id()
                )),
            );
            policy_resource.properties.insert(
                "PolicyDocument".to_string(),
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list(uniquify_iam_statement_sids(policy_statements)),
                    ),
                ]),
            );
            policy_resource.properties.insert(
                "Roles".to_string(),
                CfExpression::list([CfExpression::ref_(&role_id)]),
            );
            policy_resource.depends_on.push(bucket_id.to_string());
            policy_resource.depends_on.push(role_id.clone());
            resources.push(policy_resource);
        }
    }

    Ok(resources)
}

fn storage_policy_statement_for_bucket(statement: CfExpression, bucket_id: &str) -> CfExpression {
    let CfExpression::Object(mut statement_object) = statement else {
        return statement;
    };
    if let Some(resource) = statement_object.get_mut("Resource") {
        let original = std::mem::replace(resource, CfExpression::list(Vec::<CfExpression>::new()));
        *resource = storage_resource_refs(original, bucket_id);
    }
    CfExpression::Object(statement_object)
}

fn storage_resource_refs(resource: CfExpression, bucket_id: &str) -> CfExpression {
    match resource {
        CfExpression::List(resources) => CfExpression::list(
            resources
                .into_iter()
                .map(|resource| storage_resource_ref(resource, bucket_id)),
        ),
        resource => storage_resource_ref(resource, bucket_id),
    }
}

fn storage_resource_ref(resource: CfExpression, bucket_id: &str) -> CfExpression {
    if storage_resource_is_object_arn(&resource) {
        CfExpression::sub(format!("arn:${{AWS::Partition}}:s3:::${{{bucket_id}}}/*"))
    } else {
        CfExpression::get_att(bucket_id, "Arn")
    }
}

fn storage_resource_is_object_arn(resource: &CfExpression) -> bool {
    match resource {
        CfExpression::String(value) => value.ends_with("/*"),
        CfExpression::Object(object) => object
            .get("Fn::Sub")
            .is_some_and(storage_resource_is_object_arn),
        CfExpression::List(values) => values.iter().any(storage_resource_is_object_arn),
        _ => false,
    }
}

fn storage_permission_owners(
    ctx: &EmitContext<'_>,
) -> Vec<(usize, (String, Vec<PermissionSetReference>))> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = storage_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if service_account_for_id(ctx, &service_account_id).is_some() {
            if let Some(role_id) = service_account_role_id(ctx, profile_name) {
                owners.push((role_id, refs));
            }
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = storage_permission_refs(profile, ctx.resource_id);
        if !refs.is_empty() {
            if let Some(role_id) = remote_stack_management_role_id(ctx) {
                owners.push((role_id, refs));
            }
        }
    }

    owners.into_iter().enumerate().collect()
}

fn storage_permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
) -> Vec<PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with("storage/"))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    refs
}

fn service_account_for_id<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<&'a ServiceAccount> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()
}

fn remote_stack_management_role_id(ctx: &EmitContext<'_>) -> Option<String> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() != RemoteStackManagement::RESOURCE_TYPE {
            return None;
        }
        let logical_id = ctx.name_for(id)?;
        if logical_id == "Management" {
            Some("ManagementRole".to_string())
        } else {
            Some(format!("{logical_id}Role"))
        }
    })
}
