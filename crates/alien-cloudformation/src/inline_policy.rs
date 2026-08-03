use crate::{
    emitters::aws::helpers::{chunk_managed_policy_statements, uniquify_iam_statement_sids},
    template::{CfExpression, CfResource, CfTemplate},
};
use alien_core::{ErrorData, Result};
use alien_error::AlienError;
use indexmap::IndexMap;

const IAM_POLICY_RESOURCE_TYPE: &str = "AWS::IAM::Policy";
const IAM_MANAGED_POLICY_RESOURCE_TYPE: &str = "AWS::IAM::ManagedPolicy";
const IAM_MANAGED_POLICIES_PER_ROLE: usize = 10;

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
                        "IAM role '{role_id}' requires {} managed policies, exceeding AWS's attachment limit of {IAM_MANAGED_POLICIES_PER_ROLE}",
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

fn expression_references(expression: &CfExpression, logical_id: &str) -> bool {
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
    use super::{compact_statements, consolidate_role_inline_policies};
    use crate::template::{CfExpression, CfResource, CfTemplate};

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
        let mut template = template_with_inline_grants();
        let role = template
            .resources
            .get_mut("ExecutionRole")
            .expect("role should exist");
        role.properties.insert(
            "ManagedPolicyArns".to_string(),
            CfExpression::list((0..10).map(|index| {
                CfExpression::from(format!("arn:aws:iam::aws:policy/Existing{index}"))
            })),
        );

        let error = consolidate_role_inline_policies(&mut template)
            .expect_err("the eleventh managed-policy attachment must be rejected");
        assert!(error.to_string().contains("requires 11 managed policies"));
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
