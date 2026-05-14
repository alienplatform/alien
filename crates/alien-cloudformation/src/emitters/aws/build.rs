//! AWS Build — CodeBuild project plus log group and (optional fallback) role.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        required_logical_id, resource_config, role_for_profile_or_fallback, stack_name, tags,
        INLINE_POLICY_NAME,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, Build, ComputeType, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsBuildEmitter;

impl CfEmitter for AwsBuildEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let build = resource_config::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role = role_for_profile_or_fallback(
            ctx,
            &build.permissions,
            &format!("{logical_id}Role"),
            "codebuild.amazonaws.com",
            codebuild_fallback_policy(),
        )?;
        let log_group_id = format!("{logical_id}LogGroup");

        let mut log_group =
            CfResource::new(log_group_id.clone(), "AWS::Logs::LogGroup".to_string());
        log_group.properties.insert(
            "LogGroupName".to_string(),
            CfExpression::sub(format!("/aws/codebuild/${{AWS::StackName}}-{}", build.id)),
        );
        log_group
            .properties
            .insert("RetentionInDays".to_string(), CfExpression::Integer(30));
        log_group.deletion_policy = Some("Retain".to_string());
        log_group.update_replace_policy = Some("Retain".to_string());

        let mut project = CfResource::new(
            logical_id.to_string(),
            "AWS::CodeBuild::Project".to_string(),
        );
        project
            .properties
            .insert("Name".to_string(), stack_name(&build.id));
        project
            .properties
            .insert("ServiceRole".to_string(), role.arn.clone());
        project.properties.insert(
            "Artifacts".to_string(),
            CfExpression::object([("Type", CfExpression::from("NO_ARTIFACTS"))]),
        );
        project.properties.insert(
            "Source".to_string(),
            CfExpression::object([
                ("Type", CfExpression::from("NO_SOURCE")),
                ("BuildSpec", CfExpression::from(default_buildspec())),
            ]),
        );
        project.properties.insert(
            "Environment".to_string(),
            CfExpression::object([
                ("Type", CfExpression::from("LINUX_CONTAINER")),
                ("Image", CfExpression::from("aws/codebuild/standard:7.0")),
                (
                    "ComputeType",
                    CfExpression::from(codebuild_compute_type(&build.compute_type)),
                ),
                ("PrivilegedMode", CfExpression::from(true)),
                (
                    "EnvironmentVariables",
                    CfExpression::list(build.environment.iter().map(|(name, value)| {
                        CfExpression::object([
                            ("Name", CfExpression::from(name.clone())),
                            ("Value", CfExpression::from(value.clone())),
                            ("Type", CfExpression::from("PLAINTEXT")),
                        ])
                    })),
                ),
            ]),
        );
        project.properties.insert(
            "LogsConfig".to_string(),
            CfExpression::object([(
                "CloudWatchLogs",
                CfExpression::object([
                    ("Status", CfExpression::from("ENABLED")),
                    ("GroupName", CfExpression::ref_(&log_group_id)),
                ]),
            )]),
        );
        project.properties.insert("Tags".to_string(), tags(ctx));
        project.depends_on.push(log_group_id);
        if let Some(role_id) = role.resource_id {
            project.depends_on.push(role_id);
        }

        let mut resources = role.resources;
        resources.push(log_group);
        resources.push(project);
        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let build = resource_config::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        Ok(CfExpression::object([
            ("projectName", CfExpression::ref_(logical_id)),
            ("projectArn", CfExpression::get_att(logical_id, "Arn")),
            (
                "buildEnvVars",
                CfExpression::Object(
                    build
                        .environment
                        .iter()
                        .map(|(key, value)| (key.clone(), CfExpression::from(value.clone())))
                        .collect(),
                ),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let build = resource_config::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("codebuild")),
            ("projectName", CfExpression::ref_(logical_id)),
            (
                "buildEnvVars",
                CfExpression::Object(
                    build
                        .environment
                        .iter()
                        .map(|(key, value)| (key.clone(), CfExpression::from(value.clone())))
                        .collect(),
                ),
            ),
            ("monitoring", CfExpression::Null),
        ])))
    }
}

fn codebuild_fallback_policy() -> CfExpression {
    let _ = INLINE_POLICY_NAME;
    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list([CfExpression::object([
                ("Sid", CfExpression::from("WriteLogs")),
                ("Effect", CfExpression::from("Allow")),
                (
                    "Action",
                    CfExpression::list([
                        CfExpression::from("logs:CreateLogGroup"),
                        CfExpression::from("logs:CreateLogStream"),
                        CfExpression::from("logs:PutLogEvents"),
                    ]),
                ),
                ("Resource", CfExpression::from("*")),
            ])]),
        ),
    ])
}

fn codebuild_compute_type(compute_type: &ComputeType) -> &'static str {
    match compute_type {
        ComputeType::Small => "BUILD_GENERAL1_SMALL",
        ComputeType::Medium => "BUILD_GENERAL1_MEDIUM",
        ComputeType::Large | ComputeType::XLarge => "BUILD_GENERAL1_LARGE",
    }
}

fn default_buildspec() -> &'static str {
    r#"version: 0.2
phases:
  build:
    commands:
      - echo "Build project is ready for runtime-provided commands"
"#
}
