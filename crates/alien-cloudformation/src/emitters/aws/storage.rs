//! AWS Storage — S3 bucket + bucket policy.
//!
//! Renders an `AWS::S3::Bucket` with encryption, ownership controls, and
//! public access block plus an `AWS::S3::BucketPolicy` enforcing TLS in
//! transit. Supports optional versioning, lifecycle rules, and an
//! optional public-read policy when `Storage::public_read` is set.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        required_logical_id, resource_config, stack_name, storage_notification_configuration,
        storage_notification_dependencies, tags,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, Result, Storage};
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
            .insert("BucketName".to_string(), stack_name(storage.id()));
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
            bucket_policy_document(bucket_id, storage.public_read),
        );

        Ok(vec![bucket, policy])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let bucket_id = required_logical_id(ctx)?;
        Ok(CfExpression::object([
            ("bucketName", CfExpression::ref_(bucket_id)),
            ("bucketArn", CfExpression::get_att(bucket_id, "Arn")),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let storage = resource_config::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("s3")),
            ("bucketName", stack_name(storage.id())),
        ])))
    }
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

fn bucket_policy_document(bucket_id: &str, public_read: bool) -> CfExpression {
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
            ("Resource", bucket_objects_arn),
        ]));
    }

    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        ("Statement", CfExpression::list(statements)),
    ])
}
