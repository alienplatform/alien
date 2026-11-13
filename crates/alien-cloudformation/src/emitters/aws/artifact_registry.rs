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
        let replication = replication_resources(ctx, registry)?;
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
        repository.retain_data();

        let mut pull_role = ecr_access_role(ctx, &pull_role_id, "registry-pull", false)?;
        let mut push_role = ecr_access_role(ctx, &push_role_id, "registry-push", true)?;
        pull_role.depends_on.push(repository_id.clone());
        push_role.depends_on.push(repository_id);

        let mut resources = vec![repository, pull_role, push_role];
        resources.extend(replication);
        Ok(resources)
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
    role_name_suffix: &str,
    push: bool,
) -> Result<CfResource> {
    let mut role = CfResource::new(role_id.to_string(), "AWS::IAM::Role".to_string());
    role.properties
        .insert("RoleName".to_string(), stack_name(role_name_suffix));
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

    let mut statements = vec![
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
    ];
    if push {
        // Replication copies images, but each regional repository needs its own pull policy.
        statements.push(CfExpression::object([
            ("Sid", CfExpression::from("RegionalRepositoryPolicies")),
            ("Effect", CfExpression::from("Allow")),
            ("Action", CfExpression::list([
                "ecr:GetRepositoryPolicy", "ecr:DeleteRepositoryPolicy", "ecr:SetRepositoryPolicy", "ecr:DescribeRepositories",
            ].into_iter().map(CfExpression::from))),
            ("Resource", CfExpression::list([
                CfExpression::sub(format!("arn:${{AWS::Partition}}:ecr:*:${{AWS::AccountId}}:repository/${{AWS::StackName}}-{}", registry.id())),
                CfExpression::sub(format!("arn:${{AWS::Partition}}:ecr:*:${{AWS::AccountId}}:repository/${{AWS::StackName}}-{}-*", registry.id())),
            ])),
        ]));
    }
    Ok(CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        ("Statement", CfExpression::list(statements)),
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

/// ECR replication for the regions the registry's owner declares. Lambda only
/// pulls from ECR in its own region, so the images must exist there. The rule
/// covers this registry's repository prefix only. No repository creation
/// template is written: replicas use the ECR defaults for tag immutability and
/// lifecycle (an accepted limitation), and cross-account pull grants are
/// written on each replica by the registry binding. Replication configuration
/// is one per account and region: only a stack that declares regions writes it.
// CloudFormation cannot read the account's existing replication rules, so the check happens
// before this template is used: the runtime preflight `ReplicationRulesCheck` refuses foreign
// rules, and a caller that generates the template outside the preflights must read the
// registry's reported replication state first.
fn replication_resources(
    ctx: &EmitContext<'_>,
    registry: &ArtifactRegistry,
) -> Result<Vec<CfResource>> {
    let from_input = registry.replication_regions_input.as_deref();
    if registry.replication_regions.is_empty() && from_input.is_none() {
        return Ok(Vec::new());
    }
    if from_input.is_some() && !registry.replication_regions.is_empty() {
        return Err(alien_error::AlienError::new(
            alien_core::ErrorData::GenericError {
                message: format!(
                    "Artifact registry '{}': set either replicationRegions or a regions input, not both",
                    registry.id()
                ),
            },
        ));
    }
    for region in &registry.replication_regions {
        let valid = !region.is_empty()
            && region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid {
            return Err(alien_error::AlienError::new(
                alien_core::ErrorData::GenericError {
                    message: format!(
                        "Artifact registry '{}': '{region}' is not an AWS region name",
                        registry.id()
                    ),
                },
            ));
        }
    }
    let destinations = match from_input {
        Some(input_id) => CfExpression::list((0..=MAX_REPLICATION_REGION_SLOTS).map(|slot| {
            let region = if slot == MAX_REPLICATION_REGION_SLOTS {
                CfExpression::from(format!(
                    "too-many-regions-at-most-{MAX_REPLICATION_REGION_SLOTS}-are-supported"
                ))
            } else {
                replication_slot_region(input_id, slot)
            };
            let destination = CfExpression::object([
                ("Region", region),
                ("RegistryId", CfExpression::ref_("AWS::AccountId")),
            ]);
            if slot == 0 {
                // The whole resource is conditional on slot 0, so a nested
                // condition on it could never be false.
                destination
            } else {
                CfExpression::if_(
                    replication_slot_condition(input_id, slot),
                    destination,
                    CfExpression::no_value(),
                )
            }
        })),
        None => CfExpression::list(registry.replication_regions.iter().map(|region| {
            CfExpression::object([
                ("Region", CfExpression::from(region.as_str())),
                ("RegistryId", CfExpression::ref_("AWS::AccountId")),
            ])
        })),
    };
    let logical_id = required_logical_id(ctx)?;
    let mut configuration = CfResource::new(
        format!("{logical_id}Replication"),
        "AWS::ECR::ReplicationConfiguration".to_string(),
    );
    configuration.properties.insert(
        "ReplicationConfiguration".to_string(),
        CfExpression::object([(
            "Rules",
            CfExpression::list([CfExpression::object([
                ("Destinations", destinations),
                (
                    "RepositoryFilters",
                    CfExpression::list([CfExpression::object([
                        ("Filter", stack_name(registry.id())),
                        ("FilterType", CfExpression::from("PREFIX_MATCH")),
                    ])]),
                ),
            ])]),
        )]),
    );

    if let Some(input_id) = from_input {
        // No region answered: write nothing, so an install that leaves the
        // input empty keeps whatever replication its account already has.
        configuration.condition = Some(replication_slot_condition(input_id, 0));
    }
    configuration
        .depends_on
        .push(format!("{logical_id}Repository"));
    Ok(vec![configuration])
}

/// Region slots a deployer-provided list can fill. CloudFormation cannot loop
/// over a parameter, so the template reserves this many destinations. An
/// eleventh region makes one more destination whose region name is an error
/// message, so ECR rejects the stack instead of dropping a region silently
/// (template rules cannot count list members).
pub(crate) const MAX_REPLICATION_REGION_SLOTS: usize = 10;

pub(crate) fn replication_slot_condition(input_id: &str, slot: usize) -> String {
    format!(
        "{}ReplicaRegion{slot}Set",
        crate::generator::stack_input_parameter_name_for_id(input_id)
    )
}

/// The slot's region from the input list. The list is padded with empty
/// entries first, because `Fn::Select` past the end fails the whole operation
/// and conditions are evaluated on every create and update.
pub(crate) fn replication_slot_region(input_id: &str, slot: usize) -> CfExpression {
    let parameter = crate::generator::stack_input_parameter_name_for_id(input_id);
    let padding = (0..=MAX_REPLICATION_REGION_SLOTS + 1).map(|_| CfExpression::from(""));
    let padded = CfExpression::object([(
        "Fn::Split",
        CfExpression::list([
            CfExpression::from(","),
            CfExpression::join(
                ",",
                CfExpression::list(
                    std::iter::once(CfExpression::join(",", CfExpression::ref_(parameter)))
                        .chain(padding),
                ),
            ),
        ]),
    )]);
    CfExpression::object([(
        "Fn::Select",
        CfExpression::list([CfExpression::Integer(slot as i64), padded]),
    )])
}
