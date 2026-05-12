//! AWS ArtifactRegistry — ECR repository plus pull and push IAM roles.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        required_logical_id, resource_config, stack_name, tags, PARAM_MANAGING_ROLE_ARN,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, ArtifactRegistry, Result, ServiceAccount};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsArtifactRegistryEmitter;

impl CfEmitter for AwsArtifactRegistryEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let registry = resource_config::<ArtifactRegistry>(ctx, ArtifactRegistry::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let repository_id = format!("{logical_id}Repository");
        let pull_role_id = format!("{logical_id}PullRole");
        let push_role_id = format!("{logical_id}PushRole");

        let mut repository =
            CfResource::new(repository_id.clone(), "AWS::ECR::Repository".to_string());
        repository
            .properties
            .insert("RepositoryName".to_string(), stack_name(registry.id()));
        repository.properties.insert(
            "ImageScanningConfiguration".to_string(),
            CfExpression::object([("ScanOnPush", CfExpression::from(true))]),
        );
        repository.properties.insert(
            "ImageTagMutability".to_string(),
            CfExpression::from("IMMUTABLE"),
        );
        repository.properties.insert(
            "EncryptionConfiguration".to_string(),
            CfExpression::object([("EncryptionType", CfExpression::from("AES256"))]),
        );
        repository.properties.insert(
            "LifecyclePolicy".to_string(),
            CfExpression::object([(
                "LifecyclePolicyText",
                CfExpression::to_json_string(ecr_lifecycle_policy()),
            )]),
        );
        repository.properties.insert("Tags".to_string(), tags(ctx));
        repository.deletion_policy = Some("Retain".to_string());
        repository.update_replace_policy = Some("Retain".to_string());

        let mut pull_role = ecr_access_role(ctx, &pull_role_id, registry, "pull", false)?;
        let mut push_role = ecr_access_role(ctx, &push_role_id, registry, "push", true)?;
        pull_role.depends_on.push(repository_id.clone());
        push_role.depends_on.push(repository_id);

        Ok(vec![repository, pull_role, push_role])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let registry = resource_config::<ArtifactRegistry>(ctx, ArtifactRegistry::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let pull_role_id = format!("{logical_id}PullRole");
        let push_role_id = format!("{logical_id}PushRole");
        Ok(CfExpression::object([
            ("accountId", CfExpression::ref_("AWS::AccountId")),
            ("region", CfExpression::ref_("AWS::Region")),
            (
                "registryId",
                CfExpression::sub("${AWS::AccountId}:${AWS::Region}"),
            ),
            (
                "registryEndpoint",
                CfExpression::sub("${AWS::AccountId}.dkr.ecr.${AWS::Region}.${AWS::URLSuffix}"),
            ),
            ("repositoryPrefix", stack_name(registry.id())),
            ("pullRoleArn", CfExpression::get_att(pull_role_id, "Arn")),
            ("pushRoleArn", CfExpression::get_att(push_role_id, "Arn")),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let registry = resource_config::<ArtifactRegistry>(ctx, ArtifactRegistry::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let pull_role_id = format!("{logical_id}PullRole");
        let push_role_id = format!("{logical_id}PushRole");
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("ecr")),
            ("repositoryPrefix", stack_name(registry.id())),
            ("pullRoleArn", CfExpression::get_att(pull_role_id, "Arn")),
            ("pushRoleArn", CfExpression::get_att(push_role_id, "Arn")),
        ])))
    }
}

fn ecr_access_role(
    ctx: &EmitContext<'_>,
    role_id: &str,
    registry: &ArtifactRegistry,
    suffix: &str,
    push: bool,
) -> Result<CfResource> {
    let mut role = CfResource::new(role_id.to_string(), "AWS::IAM::Role".to_string());
    role.properties.insert(
        "RoleName".to_string(),
        stack_name(&format!("{}-{suffix}", registry.id())),
    );
    role.properties.insert(
        "AssumeRolePolicyDocument".to_string(),
        ecr_role_trust_policy(ctx),
    );
    role.properties.insert(
        "Policies".to_string(),
        CfExpression::list([CfExpression::object([
            (
                "PolicyName",
                CfExpression::from(if push { "ecr-push-pull" } else { "ecr-pull" }),
            ),
            ("PolicyDocument", ecr_policy_document(ctx, push)?),
        ])]),
    );
    role.properties.insert("Tags".to_string(), tags(ctx));
    Ok(role)
}

fn ecr_role_trust_policy(ctx: &EmitContext<'_>) -> CfExpression {
    let mut principals = Vec::new();
    for (id, entry) in ctx.stack.resources() {
        if entry.config.downcast_ref::<ServiceAccount>().is_some() {
            if let Some(logical_id) = ctx.name_for(id) {
                principals.push(CfExpression::get_att(format!("{logical_id}Role"), "Arn"));
            }
        }
    }

    if principals.is_empty() {
        principals.push(CfExpression::ref_(PARAM_MANAGING_ROLE_ARN));
    }

    let principal_value = if principals.len() == 1 {
        principals.remove(0)
    } else {
        CfExpression::list(principals)
    };

    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list([CfExpression::object([
                ("Effect", CfExpression::from("Allow")),
                (
                    "Principal",
                    CfExpression::object([("AWS", principal_value)]),
                ),
                ("Action", CfExpression::from("sts:AssumeRole")),
            ])]),
        ),
    ])
}

fn ecr_policy_document(ctx: &EmitContext<'_>, push: bool) -> Result<CfExpression> {
    let logical_id = required_logical_id(ctx)?;
    let repository_id = format!("{logical_id}Repository");
    let registry = resource_config::<ArtifactRegistry>(ctx, ArtifactRegistry::RESOURCE_TYPE)?;
    let mut repository_actions = vec![
        "ecr:BatchCheckLayerAvailability",
        "ecr:BatchGetImage",
        "ecr:DescribeImages",
        "ecr:DescribeRepositories",
        "ecr:GetDownloadUrlForLayer",
        "ecr:ListImages",
    ];
    if push {
        repository_actions.extend([
            "ecr:CompleteLayerUpload",
            "ecr:CreateRepository",
            "ecr:DeleteRepository",
            "ecr:InitiateLayerUpload",
            "ecr:PutImage",
            "ecr:UploadLayerPart",
        ]);
    }

    Ok(CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list([
                CfExpression::object([
                    ("Sid", CfExpression::from("GetAuthorizationToken")),
                    ("Effect", CfExpression::from("Allow")),
                    ("Action", CfExpression::from("ecr:GetAuthorizationToken")),
                    ("Resource", CfExpression::from("*")),
                ]),
                CfExpression::object([
                    ("Sid", CfExpression::from("RepositoryAccess")),
                    ("Effect", CfExpression::from("Allow")),
                    (
                        "Action",
                        CfExpression::list(repository_actions.into_iter().map(CfExpression::from)),
                    ),
                    (
                        "Resource",
                        CfExpression::list([
                            CfExpression::get_att(repository_id, "Arn"),
                            CfExpression::sub(format!(
                                "arn:${{AWS::Partition}}:ecr:${{AWS::Region}}:${{AWS::AccountId}}:repository/${{AWS::StackName}}-{}-*",
                                registry.id()
                            )),
                        ]),
                    ),
                ]),
            ]),
        ),
    ]))
}

fn ecr_lifecycle_policy() -> CfExpression {
    CfExpression::object([(
        "rules",
        CfExpression::list([CfExpression::object([
            ("rulePriority", CfExpression::Integer(1)),
            (
                "description",
                CfExpression::from("Retain the 100 most recent images"),
            ),
            (
                "selection",
                CfExpression::object([
                    ("tagStatus", CfExpression::from("any")),
                    ("countType", CfExpression::from("imageCountMoreThan")),
                    ("countNumber", CfExpression::Integer(100)),
                ]),
            ),
            (
                "action",
                CfExpression::object([("type", CfExpression::from("expire"))]),
            ),
        ])]),
    )])
}
