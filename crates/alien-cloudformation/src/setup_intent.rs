//! Non-secret setup intent read back from the registration resource.

use alien_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use indexmap::IndexMap;

use crate::{
    dependencies::collect_references,
    json_text::lower_json_strings,
    template::{CfExpression, CfParameter, CfResource, CfTemplate},
};

/// Maximum UTF-8 size accepted by a setup intent reader after cloud resolution.
pub const MAX_SETUP_INTENT_BYTES: usize = 32 * 1024;

/// Add an authoritative, non-secret intent to an existing registration template.
///
/// The audience is supplied as a parameter at installation time. The identical
/// property is necessary: a metadata-only update does not invoke the resource.
/// This does not authorize callbacks; the reader must independently check the
/// exact stack, registration resource, audience and operation lifecycle.
pub fn add_verified_setup_intent(template: &mut CfTemplate) -> Result<()> {
    let mut candidate = template.clone();
    add_intent(&mut candidate)?;
    *template = candidate;
    Ok(())
}

fn add_intent(template: &mut CfTemplate) -> Result<()> {
    if template.resources.contains_key("SetupIntentReadPolicy")
        || template.parameters.contains_key("SetupManagerAudience")
        || template
            .resources
            .get("DeploymentRegistration")
            .is_some_and(|registration| {
                registration.metadata.contains_key("SetupIntent")
                    || registration.properties.contains_key("SetupIntent")
            })
    {
        return Err(refusal("setup intent identifiers are already in use"));
    }
    let management = template
        .resources
        .iter()
        .find(|(_, resource)| {
            resource.resource_type == "AWS::IAM::Role"
                && resource.properties.get("RoleName")
                    == Some(&CfExpression::sub("${AWS::StackName}-management"))
        })
        .map(|(id, resource)| (id.clone(), resource.condition.clone()))
        .ok_or_else(|| refusal("a setup-owned management role is required"))?;
    let registration = template
        .resources
        .get("DeploymentRegistration")
        .ok_or_else(|| refusal("a DeploymentRegistration resource is required"))?;
    // Only the fields the manager imports. Everything here is already in the
    // template; metadata exposes it to the stack's own management role only.
    let mut fields = IndexMap::new();
    for name in [
        "DeploymentName",
        "ResourcePrefix",
        "Platform",
        "BasePlatform",
        "Region",
        "SetupTarget",
        "SetupImportFormatVersion",
        "SetupFingerprint",
        "SetupFingerprintVersion",
    ] {
        if let Some(value) = registration.properties.get(name) {
            fields.insert(name.to_string(), value.clone());
        }
    }
    for name in ["ManagementConfig", "StackSettings", "Resources"] {
        let value = registration
            .properties
            .get(name)
            .ok_or_else(|| refusal("registration is missing an intent field"))?;
        // CloudFormation turns typed leaves into strings, so these travel as JSON text.
        fields.insert(
            name.to_string(),
            if name == "Resources" {
                resources_json(value)?
            } else {
                CfExpression::to_json_string(value.clone())
            },
        );
    }
    if let Some(inputs) = registration.properties.get("InputValues") {
        fields.insert("InputValues".to_string(), inputs.clone());
    }
    let source = CfExpression::Object(fields);
    let mut references = indexmap::IndexSet::new();
    collect_references(&source, &mut references);
    if references.iter().any(|name| {
        template
            .parameters
            .get(name)
            .is_some_and(|parameter| parameter.no_echo == Some(true))
    }) {
        return Err(refusal(
            "setup intent cannot reference a sensitive parameter",
        ));
    }
    template.parameters.insert(
        "SetupManagerAudience".into(),
        CfParameter {
            parameter_type: "String".into(),
            description: Some("Management endpoint authorized to register this install.".into()),
            default: None,
            allowed_values: None,
            allowed_pattern: Some("^https://[A-Za-z0-9][A-Za-z0-9.:-]*(/[A-Za-z0-9/_-]*)?$".into()),
            constraint_description: Some("must be an HTTPS management endpoint".into()),
            min_length: Some(1),
            max_length: Some(2048),
            min_value: None,
            max_value: None,
            no_echo: None,
        },
    );
    let intent = CfExpression::object([
        ("version", CfExpression::from("1")),
        ("installId", CfExpression::ref_("AWS::StackId")),
        ("audience", CfExpression::ref_("SetupManagerAudience")),
        ("source", source),
    ]);
    // The generator's JSON lowering checks every parameter interpolated into
    // JSON text. InputValues stays native so plain text retains quotes and slashes.
    template
        .resources
        .get_mut("DeploymentRegistration")
        .ok_or_else(|| refusal("registration disappeared"))?
        .properties
        .insert("SetupIntent".into(), intent);
    lower_json_strings(template)?;
    let registration = template
        .resources
        .get_mut("DeploymentRegistration")
        .ok_or_else(|| refusal("registration disappeared"))?;
    let intent = serde_json::to_value(&registration.properties["SetupIntent"])
        .into_alien_error()
        .context(ErrorData::GenericError {
            message: "could not serialize setup intent".into(),
        })?;
    registration.metadata.insert("SetupIntent".into(), intent);
    if serde_json::to_vec(&registration.metadata)
        .into_alien_error()
        .context(ErrorData::GenericError {
            message: "could not measure setup intent".into(),
        })?
        .len()
        > MAX_SETUP_INTENT_BYTES
    {
        return Err(refusal("setup intent exceeds the 32768-byte limit"));
    }
    let mut policy = CfResource::new("SetupIntentReadPolicy".into(), "AWS::IAM::Policy".into());
    policy.condition = management.1.clone();
    policy.properties.insert(
        "PolicyName".into(),
        CfExpression::sub("${AWS::StackName}-setup-read"),
    );
    policy.properties.insert(
        "Roles".into(),
        CfExpression::list([CfExpression::ref_(&management.0)]),
    );
    policy.properties.insert(
        "PolicyDocument".into(),
        CfExpression::object([
            ("Version", CfExpression::from("2012-10-17")),
            (
                "Statement",
                CfExpression::list([CfExpression::object([
                    ("Sid", CfExpression::from("ReadOwnSetupIntent")),
                    ("Effect", CfExpression::from("Allow")),
                    (
                        "Action",
                        CfExpression::from("cloudformation:DescribeStackResource"),
                    ),
                    ("Resource", CfExpression::ref_("AWS::StackId")),
                ])]),
            ),
        ]),
    );
    if let Some(condition) = management.1 {
        registration.properties.insert(
            "SetupReaderReady".into(),
            CfExpression::if_(
                condition,
                CfExpression::ref_("SetupIntentReadPolicy"),
                CfExpression::no_value(),
            ),
        );
    } else {
        registration.depends_on.push("SetupIntentReadPolicy".into());
    }
    if template
        .resources
        .insert("SetupIntentReadPolicy".into(), policy)
        .is_some()
    {
        return Err(refusal("setup reader policy logical ID is already in use"));
    }

    Ok(())
}

fn refusal(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationNotSupported {
        operation: "generate verified setup intent".into(),
        reason: reason.into(),
    })
}

fn resources_json(value: &CfExpression) -> Result<CfExpression> {
    let CfExpression::List(entries) = value else {
        return Err(refusal("registration resources must be a list"));
    };
    let mut encoded = Vec::with_capacity(entries.len());
    for entry in entries {
        // A conditional resource stays conditional; only its body becomes JSON text.
        if let CfExpression::Object(fields) = entry {
            if let Some(CfExpression::List(parts)) = fields.get("Fn::If") {
                if fields.len() != 1 || parts.len() != 3 || parts[2] != CfExpression::no_value() {
                    return Err(refusal("unsupported conditional registration resource"));
                }
                encoded.push(CfExpression::object([(
                    "Fn::If",
                    CfExpression::list([
                        parts[0].clone(),
                        CfExpression::to_json_string(without_build_environment(&parts[1])),
                        parts[2].clone(),
                    ]),
                )]));
                continue;
            }
        }
        encoded.push(CfExpression::to_json_string(without_build_environment(
            entry,
        )));
    }
    Ok(CfExpression::join(
        "",
        CfExpression::list([
            CfExpression::from("["),
            CfExpression::join(",", CfExpression::list(encoded)),
            CfExpression::from("]"),
        ]),
    ))
}

/// Build environment values come from the pinned release on import, never
/// from readable cloud metadata.
fn without_build_environment(resource: &CfExpression) -> CfExpression {
    let mut resource = resource.clone();
    if let CfExpression::Object(fields) = &mut resource {
        if fields.get("type") == Some(&CfExpression::from("build")) {
            if let Some(CfExpression::Object(data)) = fields.get_mut("importData") {
                data.shift_remove("buildEnvVars");
            }
        }
    }
    resource
}
