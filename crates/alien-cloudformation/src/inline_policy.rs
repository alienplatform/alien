use crate::{
    emitters::aws::helpers::{chunk_managed_policy_statements, uniquify_iam_statement_sids},
    template::{CfExpression, CfResource, CfTemplate},
};
use alien_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use indexmap::IndexMap;

const IAM_POLICY_RESOURCE_TYPE: &str = "AWS::IAM::Policy";
const IAM_MANAGED_POLICY_RESOURCE_TYPE: &str = "AWS::IAM::ManagedPolicy";
// AWS raised the default from 10 to 20 in August 2026. Do not assume an
// account has requested the optional increase beyond the default.
// https://aws.amazon.com/about-aws/whats-new/2026/08/aws-iam-quota-increase/
const IAM_MANAGED_POLICIES_PER_ROLE: usize = 20;

#[derive(PartialEq)]
struct PolicyGroupKey {
    roles: CfExpression,
    condition: Option<String>,
    document_properties: IndexMap<String, CfExpression>,
}

struct PolicyGroup {
    key: PolicyGroupKey,
    policy_ids: Vec<String>,
}

/// Combines compatible external inline policies into managed policies.
///
/// IAM applies one aggregate size quota to every inline policy on a role. The
/// resource emitters intentionally produce independent permission grants, so a
/// role with many grants otherwise repeats policy-document and statement
/// overhead until it reaches that quota. The consolidated grants use managed
/// policies so CloudFormation can attach them before deleting legacy inline
/// policies without temporarily exceeding the inline-policy quota. Equal
/// action/resource axes are still combined without broadening access.
pub(crate) fn consolidate_role_inline_policies(template: &mut CfTemplate) -> Result<()> {
    let mut groups: Vec<PolicyGroup> = Vec::new();
    let mut managed_policy_attachment_counts = managed_policy_attachment_counts(template);

    for (logical_id, resource) in &template.resources {
        let Some(key) = policy_group_key(resource) else {
            continue;
        };

        if policy_is_referenced(template, logical_id) {
            continue;
        }

        if let Some(group) = groups.iter_mut().find(|group| group.key == key) {
            group.policy_ids.push(logical_id.clone());
        } else {
            groups.push(PolicyGroup {
                key,
                policy_ids: vec![logical_id.clone()],
            });
        }
    }

    let mut replacements = IndexMap::new();
    let mut consolidated_resources = Vec::new();
    let mut consolidated_ids = Vec::new();
    for (group_index, group) in groups.into_iter().enumerate() {
        if group.policy_ids.len() < 2 {
            continue;
        }

        let mut statements = Vec::new();
        let mut dependencies = Vec::new();
        for policy_id in &group.policy_ids {
            let policy = template
                .resources
                .get(policy_id)
                .expect("grouped IAM policy should exist");
            let CfExpression::Object(policy_document) = &policy.properties["PolicyDocument"] else {
                unreachable!("grouped IAM policy document should be an object");
            };
            let Some(CfExpression::List(policy_statements)) = policy_document.get("Statement")
            else {
                unreachable!("grouped IAM policy statements should be a list");
            };

            statements.extend(policy_statements.iter().cloned());
            dependencies.extend(policy.depends_on.iter().cloned());
        }

        let policy_documents = chunk_managed_policy_statements(compact_statements(statements))?;
        for role_id in referenced_role_ids(&group.key.roles) {
            let attachment_count = managed_policy_attachment_counts
                .entry(role_id.clone())
                .or_default();
            if *attachment_count + policy_documents.len() > IAM_MANAGED_POLICIES_PER_ROLE {
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate_cloudformation_template".to_string(),
                    reason: format!(
                        "IAM role '{role_id}' requires {} managed policies, exceeding AWS's default attachment quota of {IAM_MANAGED_POLICIES_PER_ROLE}",
                        *attachment_count + policy_documents.len()
                    ),
                }));
            }
            *attachment_count += policy_documents.len();
        }

        let mut replacement_ids = Vec::new();
        for (statement_index, policy_document) in policy_documents.into_iter().enumerate() {
            let logical_id = consolidated_policy_logical_id(
                template,
                &consolidated_ids,
                &group.key,
                group_index,
                statement_index,
            );
            consolidated_ids.push(logical_id.clone());
            replacement_ids.push(logical_id.clone());

            let mut consolidated = template
                .resources
                .get(&group.policy_ids[0])
                .expect("grouped IAM policy should exist")
                .clone();
            consolidated.logical_id = logical_id;
            consolidated.resource_type = IAM_MANAGED_POLICY_RESOURCE_TYPE.to_string();
            consolidated.properties.shift_remove("PolicyName");
            consolidated
                .properties
                .insert("PolicyDocument".to_string(), policy_document);
            consolidated.depends_on = deduplicate(dependencies.clone());
            consolidated_resources.push(consolidated);
        }

        for removed_id in &group.policy_ids {
            replacements.insert(removed_id.clone(), replacement_ids.clone());
        }
    }

    for removed_id in replacements.keys() {
        template.resources.shift_remove(removed_id);
    }
    for resource in consolidated_resources {
        template
            .resources
            .insert(resource.logical_id.clone(), resource);
    }
    for resource in template.resources.values_mut() {
        resource.depends_on = deduplicate(
            std::mem::take(&mut resource.depends_on)
                .into_iter()
                .flat_map(|dependency| {
                    replacements
                        .get(&dependency)
                        .cloned()
                        .unwrap_or_else(|| vec![dependency])
                })
                .collect(),
        );
    }

    Ok(())
}

/// Rejects definite IAM role quota overflows in the final emitted template.
///
/// Unresolved values contribute no characters to the lower bound: their
/// serialized CloudFormation syntax is not the JSON that IAM receives. Literal
/// fragments of Fn::Sub strings still contribute. Likewise,
/// conditional resources are excluded rather than summing grants which might
/// never coexist. A passing check is therefore not proof of runtime headroom.
pub(crate) fn validate_role_policy_limits(template: &CfTemplate) -> Result<()> {
    let mut inline_sizes: IndexMap<String, usize> = IndexMap::new();
    let mut attachments: IndexMap<String, usize> = IndexMap::new();
    for resource in template
        .resources
        .values()
        .filter(|resource| resource.condition.is_none())
    {
        if resource.resource_type == "AWS::IAM::Role" {
            if let Some(CfExpression::List(policies)) = resource.properties.get("Policies") {
                for policy in policies {
                    if let CfExpression::Object(policy) = policy {
                        if let Some(document) = policy.get("PolicyDocument") {
                            *inline_sizes.entry(resource.logical_id.clone()).or_default() +=
                                policy_size_lower_bound(document)?;
                        }
                    }
                }
            }
            if let Some(CfExpression::List(arns)) = resource.properties.get("ManagedPolicyArns") {
                // An intrinsic element can resolve to AWS::NoValue.
                *attachments.entry(resource.logical_id.clone()).or_default() += arns
                    .iter()
                    .filter(|arn| matches!(arn, CfExpression::String(_)))
                    .count();
            }
        }
        if resource.resource_type == IAM_POLICY_RESOURCE_TYPE
            || resource.resource_type == IAM_MANAGED_POLICY_RESOURCE_TYPE
        {
            let Some(roles) = resource.properties.get("Roles") else {
                continue;
            };
            for role_id in referenced_role_ids(roles) {
                if template
                    .resources
                    .get(&role_id)
                    .is_some_and(|role| role.condition.is_some())
                {
                    continue;
                }
                if resource.resource_type == IAM_POLICY_RESOURCE_TYPE {
                    if let Some(document) = resource.properties.get("PolicyDocument") {
                        *inline_sizes.entry(role_id).or_default() +=
                            policy_size_lower_bound(document)?;
                    }
                } else {
                    *attachments.entry(role_id).or_default() += 1;
                }
            }
        }
    }

    let mut shortfalls = Vec::new();
    for (role_id, size) in inline_sizes {
        if size > 10_240 {
            shortfalls.push(format!("IAM role '{role_id}' requires at least {size} non-whitespace inline-policy characters; AWS allows 10240 across all inline policies on one role. Reduce permissions or move grants to managed policies."));
        }
    }
    for (role_id, count) in attachments {
        if count > IAM_MANAGED_POLICIES_PER_ROLE {
            shortfalls.push(format!("IAM role '{role_id}' requires at least {count} managed-policy attachments; AWS's default quota is {IAM_MANAGED_POLICIES_PER_ROLE} per role. Reduce the number of attached policies."));
        }
    }
    if shortfalls.is_empty() {
        Ok(())
    } else {
        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "generate_cloudformation_template".to_string(),
            reason: shortfalls.join("\n"),
        }))
    }
}

fn contains_intrinsic(expression: &CfExpression) -> bool {
    match expression {
        CfExpression::Object(values) => {
            values
                .keys()
                .any(|key| key == "Ref" || key.starts_with("Fn::"))
                || values.values().any(contains_intrinsic)
        }
        CfExpression::List(values) => values.iter().any(contains_intrinsic),
        _ => false,
    }
}

fn policy_size_lower_bound(expression: &CfExpression) -> Result<usize> {
    if !contains_intrinsic(expression) {
        return literal_size(expression);
    }
    match expression {
        CfExpression::Object(values) if values.contains_key("Fn::Sub") => {
            let template = match &values["Fn::Sub"] {
                CfExpression::String(template) => Some(template),
                CfExpression::List(values) => values.first().and_then(|value| match value {
                    CfExpression::String(template) => Some(template),
                    _ => None,
                }),
                _ => None,
            };
            Ok(template.map_or(0, |template| substitution_literal_size(template)))
        }
        CfExpression::Object(values)
            if values
                .keys()
                .any(|key| key == "Ref" || key.starts_with("Fn::")) =>
        {
            Ok(0)
        }
        CfExpression::Object(values) => {
            // Omit punctuation and keys of unresolved members so even a value
            // removed by AWS::NoValue cannot inflate the lower bound.
            let mut size = 0;
            for (key, value) in values {
                let value_size = policy_size_lower_bound(value)?;
                if value_size > 0 {
                    size += literal_size(key)? + value_size;
                }
            }
            Ok(size)
        }
        CfExpression::List(values) => values
            .iter()
            .try_fold(0, |size, value| Ok(size + policy_size_lower_bound(value)?)),
        value => literal_size(value),
    }
}

// Count only text CloudFormation must retain. Dynamic substitutions, JSON
// quotes and escape overhead contribute zero to this conservative bound.
fn substitution_literal_size(mut template: &str) -> usize {
    let mut size = 0;
    while let Some(start) = template.find("${") {
        size += template[..start]
            .chars()
            .filter(|character| !character.is_whitespace())
            .count();
        let variable = &template[start + 2..];
        let Some(end) = variable.find('}') else {
            return 0;
        };
        if let Some(literal) = variable[..end].strip_prefix('!') {
            size += 3 + literal
                .chars()
                .filter(|character| !character.is_whitespace())
                .count();
        }
        template = &variable[end + 1..];
    }
    size + template
        .chars()
        .filter(|character| !character.is_whitespace())
        .count()
}

fn literal_size(value: &impl serde::Serialize) -> Result<usize> {
    serde_json::to_string(value)
        .into_alien_error()
        .context(ErrorData::TemplateSerializationFailed {
            format: "CloudFormation IAM policy".to_string(),
            reason: "Failed to serialize literal policy content for quota validation".to_string(),
        })
        .map(|json| {
            json.chars()
                .filter(|character| !character.is_whitespace())
                .count()
        })
}

fn managed_policy_attachment_counts(template: &CfTemplate) -> IndexMap<String, usize> {
    let mut counts = IndexMap::new();

    for resource in template.resources.values() {
        if resource.resource_type == IAM_MANAGED_POLICY_RESOURCE_TYPE {
            if let Some(roles) = resource.properties.get("Roles") {
                for role_id in referenced_role_ids(roles) {
                    *counts.entry(role_id).or_default() += 1;
                }
            }
        }

        if resource.resource_type == "AWS::IAM::Role" {
            let Some(CfExpression::List(policy_arns)) =
                resource.properties.get("ManagedPolicyArns")
            else {
                continue;
            };
            *counts.entry(resource.logical_id.clone()).or_default() += policy_arns.len();
        }
    }

    counts
}

fn referenced_role_ids(roles: &CfExpression) -> Vec<String> {
    let CfExpression::List(roles) = roles else {
        return Vec::new();
    };

    roles
        .iter()
        .filter_map(|role| {
            let CfExpression::Object(role) = role else {
                return None;
            };
            match role.get("Ref") {
                Some(CfExpression::String(role_id)) => Some(role_id.clone()),
                _ => None,
            }
        })
        .collect()
}

fn consolidated_policy_logical_id(
    template: &CfTemplate,
    consolidated_ids: &[String],
    key: &PolicyGroupKey,
    group_index: usize,
    statement_index: usize,
) -> String {
    let role_id = match &key.roles {
        CfExpression::List(roles) if roles.len() == 1 => match &roles[0] {
            CfExpression::Object(role_ref) => match role_ref.get("Ref") {
                Some(CfExpression::String(role_id)) => Some(role_id.as_str()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    };
    let base = role_id
        .map(|role_id| format!("{role_id}ManagedPermissions{}", statement_index + 1))
        .unwrap_or_else(|| {
            format!(
                "ConsolidatedRoleManagedPermissions{}{}",
                group_index + 1,
                statement_index + 1
            )
        });

    if !template.resources.contains_key(&base) && !consolidated_ids.contains(&base) {
        return base;
    }

    (2..)
        .map(|suffix| format!("{base}{suffix}"))
        .find(|candidate| {
            !template.resources.contains_key(candidate) && !consolidated_ids.contains(candidate)
        })
        .expect("a unique CloudFormation logical id should be available")
}

fn policy_group_key(resource: &CfResource) -> Option<PolicyGroupKey> {
    if resource.resource_type != IAM_POLICY_RESOURCE_TYPE
        || !resource.metadata.is_empty()
        || resource.deletion_policy.is_some()
        || resource.update_replace_policy.is_some()
        || resource.properties.contains_key("Groups")
        || resource.properties.contains_key("Users")
    {
        return None;
    }

    let roles = resource.properties.get("Roles")?.clone();
    let CfExpression::Object(policy_document) = resource.properties.get("PolicyDocument")? else {
        return None;
    };
    if !matches!(
        policy_document.get("Statement"),
        Some(CfExpression::List(_))
    ) {
        return None;
    }

    let mut document_properties = policy_document.clone();
    document_properties.shift_remove("Statement");

    Some(PolicyGroupKey {
        roles,
        condition: resource.condition.clone(),
        document_properties,
    })
}

fn policy_is_referenced(template: &CfTemplate, policy_id: &str) -> bool {
    template
        .resources
        .values()
        .flat_map(|resource| resource.properties.values())
        .chain(template.conditions.values())
        .chain(template.outputs.values().flat_map(|output| {
            std::iter::once(&output.value)
                .chain(output.export.iter().flat_map(|export| export.values()))
        }))
        .any(|expression| expression_references(expression, policy_id))
}

pub(crate) fn expression_references(expression: &CfExpression, logical_id: &str) -> bool {
    match expression {
        CfExpression::String(value) => {
            value == logical_id
                || value.starts_with(&format!("{logical_id}."))
                || value.contains(&format!("${{{logical_id}}}"))
                || value.contains(&format!("${{{logical_id}."))
        }
        CfExpression::List(values) => values
            .iter()
            .any(|value| expression_references(value, logical_id)),
        CfExpression::Object(values) => values
            .values()
            .any(|value| expression_references(value, logical_id)),
        CfExpression::Null
        | CfExpression::Bool(_)
        | CfExpression::Integer(_)
        | CfExpression::Number(_) => false,
    }
}

fn compact_statements(statements: Vec<CfExpression>) -> Vec<CfExpression> {
    let statements = merge_statement_values(statements, "Resource");
    uniquify_iam_statement_sids(merge_statement_values(statements, "Action"))
}

struct StatementGroup {
    statement: IndexMap<String, CfExpression>,
    sid: Option<CfExpression>,
    values: Vec<CfExpression>,
}

fn merge_statement_values(statements: Vec<CfExpression>, property: &str) -> Vec<CfExpression> {
    let mut groups: Vec<StatementGroup> = Vec::new();
    let mut unchanged = Vec::new();

    for statement in statements {
        let CfExpression::Object(mut statement) = statement else {
            unchanged.push(statement);
            continue;
        };
        let Some(value) = statement.shift_remove(property) else {
            unchanged.push(CfExpression::Object(statement));
            continue;
        };
        let sid = statement.shift_remove("Sid");
        let values = match value {
            CfExpression::List(values) => values,
            value => vec![value],
        };

        if let Some(group) = groups.iter_mut().find(|group| group.statement == statement) {
            group.values.extend(values);
        } else {
            groups.push(StatementGroup {
                statement,
                sid,
                values,
            });
        }
    }

    unchanged.extend(groups.into_iter().map(|mut group| {
        if let Some(sid) = group.sid {
            group.statement.insert("Sid".to_string(), sid);
        }
        let values = deduplicate(group.values);
        let value = match values.as_slice() {
            [value] => value.clone(),
            _ => CfExpression::list(values),
        };
        group.statement.insert(property.to_string(), value);
        CfExpression::Object(group.statement)
    }));
    unchanged
}

fn deduplicate<T: PartialEq>(values: Vec<T>) -> Vec<T> {
    values.into_iter().fold(Vec::new(), |mut unique, value| {
        if !unique.contains(&value) {
            unique.push(value);
        }
        unique
    })
}

#[cfg(test)]
mod tests {
    use super::{
        compact_statements, consolidate_role_inline_policies, policy_size_lower_bound,
        validate_role_policy_limits,
    };
    use crate::{
        generate_cloudformation_template,
        template::{CfExpression, CfResource, CfTemplate},
        CfRegistry, CloudFormationOptions, CloudFormationTarget, RegistrationMode,
    };
    use alien_core::{PermissionSet, ResourceLifecycle, ServiceAccount, Stack, StackSettings};

    #[test]
    fn compaction_does_not_create_action_resource_cross_products() {
        let statements = vec![
            statement("ReadOne", "s3:GetObject", "bucket-one"),
            statement("ReadTwo", "s3:GetObject", "bucket-two"),
            statement("WriteOne", "s3:PutObject", "bucket-one"),
        ];

        let compacted = compact_statements(statements);

        assert_eq!(compacted.len(), 2);
        assert!(compacted.contains(&statement_with_values(
            "ReadOne",
            vec!["s3:GetObject"],
            vec!["bucket-one", "bucket-two"],
        )));
        assert!(compacted.contains(&statement_with_values(
            "WriteOne",
            vec!["s3:PutObject"],
            vec!["bucket-one"],
        )));
    }

    #[test]
    fn consolidation_counts_managed_policies_already_attached_to_the_role() {
        for existing in [19, 20] {
            let mut template = template_with_inline_grants();
            let role = template
                .resources
                .get_mut("ExecutionRole")
                .expect("role should exist");
            role.properties.insert(
                "ManagedPolicyArns".to_string(),
                CfExpression::list((0..existing).map(|index| {
                    CfExpression::from(format!("arn:aws:iam::aws:policy/Existing{index}"))
                })),
            );

            let result = consolidate_role_inline_policies(&mut template);
            if existing == 19 {
                result.expect("the twentieth attachment fits the default quota");
                assert_eq!(super::managed_policy_attachment_counts(&template)["ExecutionRole"], 20);
                validate_role_policy_limits(&template).unwrap();
            } else {
                let error = result.expect_err("the twenty-first attachment requires a quota increase");
                assert!(error.to_string().contains("requires 21 managed policies"));
            }
        }
    }

    #[test]
    fn consolidation_rejects_a_managed_policy_document_over_the_size_limit() {
        let mut template = template_with_inline_grants();
        for policy_id in ["GrantOne", "GrantTwo"] {
            let policy = template
                .resources
                .get_mut(policy_id)
                .expect("policy should exist");
            let CfExpression::Object(document) = policy
                .properties
                .get_mut("PolicyDocument")
                .expect("policy document should exist")
            else {
                panic!("policy document should be an object");
            };
            document.insert(
                "Statement".to_string(),
                CfExpression::list([statement("ReadData", "s3:GetObject", &"x".repeat(6_000))]),
            );
        }

        let error = consolidate_role_inline_policies(&mut template)
            .expect_err("an oversized managed policy must be rejected during generation");
        assert!(error.to_string().contains("too large for a managed policy"));
    }

    #[test]
    fn final_role_guard_aggregates_embedded_and_external_inline_policies() {
        let mut template = template_with_inline_grants();
        template.resources.shift_remove("GrantTwo");
        let document = CfExpression::object([(
            "Statement",
            CfExpression::list([statement("Read", "s3:GetObject", &"x".repeat(6_000))]),
        )]);
        template
            .resources
            .get_mut("GrantOne")
            .unwrap()
            .properties
            .insert("PolicyDocument".into(), document.clone());
        template
            .resources
            .get_mut("ExecutionRole")
            .unwrap()
            .properties
            .insert(
                "Policies".into(),
                CfExpression::list([CfExpression::object([("PolicyDocument", document)])]),
            );
        // One external policy remains inline after consolidation.
        consolidate_role_inline_policies(&mut template).unwrap();
        let error =
            validate_role_policy_limits(&template).expect_err("aggregate must exceed role quota");
        assert!(error.to_string().contains("ExecutionRole"));
        assert!(error.to_string().contains("10240"));
    }

    #[test]
    fn literal_policy_size_accepts_boundary_and_rejects_one_more_character() {
        let mut template = template_with_inline_grants();
        template.resources.shift_remove("GrantTwo");
        let base = CfExpression::object([("Resource", CfExpression::from(""))]);
        let overhead = policy_size_lower_bound(&base).unwrap();
        let policy = template.resources.get_mut("GrantOne").unwrap();
        policy.properties.insert(
            "PolicyDocument".into(),
            CfExpression::object([(
                "Resource",
                CfExpression::from("x".repeat(10_240 - overhead)),
            )]),
        );
        validate_role_policy_limits(&template).expect("exact boundary must pass");
        template
            .resources
            .get_mut("GrantOne")
            .unwrap()
            .properties
            .insert(
                "PolicyDocument".into(),
                CfExpression::object([(
                    "Resource",
                    CfExpression::from("x".repeat(10_241 - overhead)),
                )]),
            );
        validate_role_policy_limits(&template).expect_err("one character over must fail");
    }

    #[test]
    fn unresolved_and_conditional_policy_content_cannot_cause_false_shortfalls() {
        let mut template = template_with_inline_grants();
        for policy_id in ["GrantOne", "GrantTwo"] {
            template
                .resources
                .get_mut(policy_id)
                .unwrap()
                .properties
                .insert(
                    "PolicyDocument".into(),
                    CfExpression::object([(
                        "Resource",
                        CfExpression::object([(
                            "Fn::Sub",
                            CfExpression::from("${Unknown}".repeat(2_000)),
                        )]),
                    )]),
                );
        }
        validate_role_policy_limits(&template).expect("intrinsic syntax is not IAM policy content");
        let policy = template.resources.get_mut("GrantOne").unwrap();
        policy.condition = Some("Optional".into());
        policy.properties.insert(
            "PolicyDocument".into(),
            CfExpression::object([("Resource", CfExpression::from("x".repeat(20_000)))]),
        );
        validate_role_policy_limits(&template).expect("conditional grant cannot be assumed active");
    }

    #[test]
    fn final_guard_checks_default_attachment_quota() {
        let mut template = template_with_inline_grants();
        template.resources.shift_remove("GrantOne");
        template.resources.shift_remove("GrantTwo");
        template
            .resources
            .get_mut("ExecutionRole")
            .unwrap()
            .properties
            .insert(
                "ManagedPolicyArns".into(),
                CfExpression::list(
                    (0..20).map(|index| CfExpression::from(format!("policy-{index}"))),
                ),
            );
        validate_role_policy_limits(&template).expect("20 is the default quota");
        let mut extra = CfResource::new("Extra".into(), "AWS::IAM::ManagedPolicy".into());
        extra.properties.insert(
            "Roles".into(),
            CfExpression::list([CfExpression::ref_("ExecutionRole")]),
        );
        template.resources.insert("Extra".into(), extra);
        let error =
            validate_role_policy_limits(&template).expect_err("21 attachments cannot be granted");
        assert!(error.to_string().contains("21 managed-policy attachments"));
    }

    #[test]
    fn built_in_generation_blocks_large_service_account_and_renders_small_control() {
        for (count, should_pass) in [(10, true), (400, false)] {
            let resources: Vec<String> = (0..count)
                .map(|index| format!("arn:aws:s3:::q5-policy-control/objects/object-{index:04}"))
                .collect();
            let set: PermissionSet = serde_json::from_value(serde_json::json!({
                "id": "q5-policy-read",
                "description": "Read explicit test objects",
                "platforms": {"aws": [{
                    "grant": {"actions": ["s3:GetObject"]},
                    "binding": {"stack": {"resources": resources}}
                }]}
            }))
            .unwrap();
            let mut account = ServiceAccount::new("execution-sa".into()).build();
            account.stack_permission_sets.push(set);
            let stack = Stack::new("q5-policy-control".into())
                .add(account, ResourceLifecycle::Frozen)
                .build();
            let registry = CfRegistry::built_in();
            let result = generate_cloudformation_template(
                &stack,
                CloudFormationOptions {
                    registry: &registry,
                    target: CloudFormationTarget::Aws,
                    stack_settings: StackSettings::default(),
                    setup_target: "aws".into(),
                    setup_fingerprint: "test".into(),
                    setup_fingerprint_version: 1,
                    registration: RegistrationMode::OutputsFallback,
                    description: None,
                },
            );
            if should_pass {
                let template = result.expect("small policy must render");
                assert!(template.resources.contains_key("ExecutionSaRole"));
            } else {
                let error = result
                    .expect_err("oversized inline policy must fail before artifact generation");
                assert!(error.to_string().contains("ExecutionSaRole"));
                assert!(error.to_string().contains("10240"));
            }
        }
    }

    #[test]
    fn substitution_bound_preserves_literals_and_excludes_dynamic_values() {
        let dynamic = CfExpression::object([(
            "Resource",
            CfExpression::sub("arn:${AWS::Partition}:s3:::q5-data/${ObjectKey}"),
        )]);
        let resolved =
            CfExpression::object([("Resource", CfExpression::from("arn:aws:s3:::q5-data/"))]);
        let bound = policy_size_lower_bound(&dynamic).unwrap();
        assert!(bound > "arn::s3:::q5-data/".len());
        assert!(bound <= policy_size_lower_bound(&resolved).unwrap());
        assert_eq!(
            super::substitution_literal_size("${!Literal}"),
            "${Literal}".len()
        );
        assert_eq!(super::substitution_literal_size("${Unknown}"), 0);
        assert_eq!(super::substitution_literal_size("invalid ${"), 0);
    }

    #[test]
    fn final_guard_reports_multiple_role_shortfalls_together() {
        let mut template = CfTemplate::default();
        for id in ["FirstRole", "SecondRole"] {
            let mut role = CfResource::new(id.into(), "AWS::IAM::Role".into());
            role.properties.insert(
                "Policies".into(),
                CfExpression::list([CfExpression::object([(
                    "PolicyDocument",
                    CfExpression::object([("Resource", CfExpression::from("x".repeat(11_000)))]),
                )])]),
            );
            template.resources.insert(id.into(), role);
        }
        let error = validate_role_policy_limits(&template)
            .unwrap_err()
            .to_string();
        assert!(error.contains("FirstRole"));
        assert!(error.contains("SecondRole"));
    }

    fn template_with_inline_grants() -> CfTemplate {
        let mut template = CfTemplate::default();
        let role = CfResource::new("ExecutionRole".to_string(), "AWS::IAM::Role".to_string());
        template.resources.insert(role.logical_id.clone(), role);

        for (policy_id, resource) in [("GrantOne", "bucket-one"), ("GrantTwo", "bucket-two")] {
            let mut policy = CfResource::new(policy_id.to_string(), "AWS::IAM::Policy".to_string());
            policy.properties.insert(
                "PolicyName".to_string(),
                CfExpression::from(format!("{}-permissions", policy_id.to_lowercase())),
            );
            policy.properties.insert(
                "Roles".to_string(),
                CfExpression::list([CfExpression::ref_("ExecutionRole")]),
            );
            policy.properties.insert(
                "PolicyDocument".to_string(),
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list([statement("ReadData", "s3:GetObject", resource)]),
                    ),
                ]),
            );
            template.resources.insert(policy.logical_id.clone(), policy);
        }

        template
    }

    fn statement(sid: &str, action: &str, resource: &str) -> CfExpression {
        CfExpression::object([
            ("Sid", CfExpression::from(sid)),
            ("Effect", CfExpression::from("Allow")),
            ("Action", CfExpression::from(action)),
            ("Resource", CfExpression::from(resource)),
        ])
    }

    fn statement_with_values(sid: &str, actions: Vec<&str>, resources: Vec<&str>) -> CfExpression {
        let action = match actions.as_slice() {
            [action] => CfExpression::from(*action),
            _ => CfExpression::list(actions.into_iter().map(CfExpression::from)),
        };
        let resource = match resources.as_slice() {
            [resource] => CfExpression::from(*resource),
            _ => CfExpression::list(resources.into_iter().map(CfExpression::from)),
        };
        CfExpression::object([
            ("Effect", CfExpression::from("Allow")),
            ("Sid", CfExpression::from(sid)),
            ("Resource", resource),
            ("Action", action),
        ])
    }
}
