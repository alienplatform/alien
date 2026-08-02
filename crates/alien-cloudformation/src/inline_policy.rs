use crate::{
    emitters::aws::helpers::uniquify_iam_statement_sids,
    template::{CfExpression, CfResource, CfTemplate},
};
use indexmap::IndexMap;

const IAM_POLICY_RESOURCE_TYPE: &str = "AWS::IAM::Policy";

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

/// Combines compatible external inline policies attached to the same roles.
///
/// IAM applies one aggregate size quota to every inline policy on a role. The
/// resource emitters intentionally produce independent permission grants, so a
/// role with many grants otherwise repeats policy-document and statement
/// overhead until it reaches that quota. Combining the documents also lets us
/// safely combine equal actions across resources and equal resources across
/// actions without broadening access.
pub(crate) fn consolidate_role_inline_policies(template: &mut CfTemplate) {
    let mut groups: Vec<PolicyGroup> = Vec::new();

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

        let logical_id =
            consolidated_policy_logical_id(template, &consolidated_ids, &group.key, group_index);
        consolidated_ids.push(logical_id.clone());
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

        let mut consolidated = template
            .resources
            .get(&group.policy_ids[0])
            .expect("grouped IAM policy should exist")
            .clone();
        consolidated.logical_id = logical_id.clone();
        consolidated.properties.insert(
            "PolicyName".to_string(),
            CfExpression::from(format!("resource-permissions-{}", group_index + 1)),
        );
        let CfExpression::Object(policy_document) = consolidated
            .properties
            .get_mut("PolicyDocument")
            .expect("consolidated IAM policy document should exist")
        else {
            unreachable!("consolidated IAM policy document should be an object");
        };
        policy_document.insert(
            "Statement".to_string(),
            CfExpression::list(compact_statements(statements)),
        );
        consolidated.depends_on = deduplicate(dependencies);
        consolidated_resources.push(consolidated);

        for removed_id in &group.policy_ids {
            replacements.insert(removed_id.clone(), logical_id.clone());
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
        for dependency in &mut resource.depends_on {
            if let Some(retained_id) = replacements.get(dependency) {
                *dependency = retained_id.clone();
            }
        }
        resource.depends_on = deduplicate(std::mem::take(&mut resource.depends_on));
    }
}

fn consolidated_policy_logical_id(
    template: &CfTemplate,
    consolidated_ids: &[String],
    key: &PolicyGroupKey,
    group_index: usize,
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
        .map(|role_id| format!("{role_id}InlinePermissions"))
        .unwrap_or_else(|| format!("ConsolidatedRoleInlinePermissions{}", group_index + 1));

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
    use super::compact_statements;
    use crate::template::CfExpression;

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
