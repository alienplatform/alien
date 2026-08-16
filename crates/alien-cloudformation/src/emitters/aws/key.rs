use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{required_logical_id, resource_config, tags},
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, ErrorData, Key, RemoteBindings, RemoteStackManagement, Result,
};
use alien_error::AlienError;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsKeyEmitter;

impl CfEmitter for AwsKeyEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let _ = resource_config::<Key>(ctx, Key::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let mut key = CfResource::new(logical_id.to_string(), "AWS::KMS::Key".to_string());
        key.properties.insert(
            "Description".to_string(),
            CfExpression::from(format!("Alien encryption key '{}'", ctx.resource_id)),
        );
        key.properties
            .insert("EnableKeyRotation".to_string(), CfExpression::from(true));
        key.properties
            .insert("PendingWindowInDays".to_string(), CfExpression::from(30u32));
        key.properties
            .insert("KeyPolicy".to_string(), root_key_policy());
        key.properties.insert("Tags".to_string(), tags(ctx));
        key.deletion_policy = Some("Retain".to_string());
        key.update_replace_policy = Some("Retain".to_string());

        let mut resources = vec![key];
        if management_has_key_permission(ctx) {
            resources.push(management_metadata_policy(ctx, logical_id)?);
        }
        if alien_core::remote_bindings::remote_binding_for_entry(ctx.resource).is_some() {
            resources.push(remote_access_policy(ctx, logical_id)?);
        }
        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let logical_id = required_logical_id(ctx)?;
        Ok(CfExpression::object([(
            "keyArn",
            CfExpression::get_att(logical_id, "Arn"),
        )]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let logical_id = required_logical_id(ctx)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("kms")),
            ("keyArn", CfExpression::get_att(logical_id, "Arn")),
            ("region", CfExpression::ref_("AWS::Region")),
        ])))
    }
}

fn management_has_key_permission(ctx: &EmitContext<'_>) -> bool {
    ctx.stack
        .management()
        .profile()
        .and_then(|profile| profile.0.get(ctx.resource_id))
        .is_some_and(|refs| {
            refs.iter()
                .any(|reference| reference.id() == "key/management")
        })
}

fn management_role_id(ctx: &EmitContext<'_>) -> Option<String> {
    ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
            .then(|| {
                ctx.name_for(id).map(|logical_id| {
                    if logical_id == "Management" {
                        "ManagementRole".to_string()
                    } else {
                        format!("{logical_id}Role")
                    }
                })
            })
            .flatten()
    })
}

fn management_metadata_policy(ctx: &EmitContext<'_>, key_id: &str) -> Result<CfResource> {
    let role_id = management_role_id(ctx).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: "managed Key has no Management identity".to_string(),
        })
    })?;
    let mut policy = CfResource::new(
        format!("{key_id}ManagementMetadataPolicy"),
        "AWS::IAM::Policy".to_string(),
    );
    policy.properties.insert(
        "PolicyName".to_string(),
        CfExpression::sub(format!(
            "${{AWS::StackName}}-{}-key-metadata",
            ctx.resource_id
        )),
    );
    policy.properties.insert(
        "Roles".to_string(),
        CfExpression::list([CfExpression::ref_(&role_id)]),
    );
    policy.properties.insert(
        "PolicyDocument".to_string(),
        CfExpression::object([
            ("Version", CfExpression::from("2012-10-17")),
            (
                "Statement",
                CfExpression::list([CfExpression::object([
                    ("Effect", CfExpression::from("Allow")),
                    ("Action", CfExpression::from("kms:DescribeKey")),
                    ("Resource", CfExpression::get_att(key_id, "Arn")),
                ])]),
            ),
        ]),
    );
    policy.depends_on.push(role_id);
    policy.depends_on.push(key_id.to_string());
    Ok(policy)
}

fn root_key_policy() -> CfExpression {
    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list([CfExpression::object([
                ("Sid", CfExpression::from("EnableAccountAdministration")),
                ("Effect", CfExpression::from("Allow")),
                (
                    "Principal",
                    CfExpression::object([(
                        "AWS",
                        CfExpression::sub("arn:${AWS::Partition}:iam::${AWS::AccountId}:root"),
                    )]),
                ),
                ("Action", CfExpression::from("kms:*")),
                ("Resource", CfExpression::from("*")),
            ])]),
        ),
    ])
}

fn remote_access_policy(ctx: &EmitContext<'_>, key_id: &str) -> Result<CfResource> {
    let access_role_id = ctx
        .stack
        .resources()
        .find_map(|(id, entry)| {
            (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
                .then(|| ctx.name_for(id))
                .flatten()
        })
        .map(|id| format!("{id}Role"))
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "remote Key has no Remote Bindings identity".to_string(),
            })
        })?;
    let actions = alien_permissions::get_permission_set("key/remote-cryptography")
        .and_then(|set| set.platforms.aws.as_ref())
        .into_iter()
        .flatten()
        .flat_map(|entry| entry.grant.actions.iter().flatten())
        .cloned()
        .collect::<Vec<_>>();
    if actions.is_empty() {
        return Err(AlienError::new(ErrorData::GenericError {
            message: "key/remote-cryptography has no AWS actions".to_string(),
        }));
    }

    let mut policy = CfResource::new(
        format!("{key_id}RemoteCryptographyPolicy"),
        "AWS::IAM::Policy".to_string(),
    );
    policy.properties.insert(
        "PolicyName".to_string(),
        CfExpression::sub(format!(
            "${{AWS::StackName}}-{}-key-access",
            ctx.resource_id
        )),
    );
    policy.properties.insert(
        "Roles".to_string(),
        CfExpression::list([CfExpression::ref_(access_role_id)]),
    );
    policy.properties.insert(
        "PolicyDocument".to_string(),
        CfExpression::object([
            ("Version", CfExpression::from("2012-10-17")),
            (
                "Statement",
                CfExpression::list([CfExpression::object([
                    ("Effect", CfExpression::from("Allow")),
                    (
                        "Action",
                        CfExpression::list(actions.into_iter().map(CfExpression::from)),
                    ),
                    ("Resource", CfExpression::get_att(key_id, "Arn")),
                ])]),
            ),
        ]),
    );
    Ok(policy)
}
