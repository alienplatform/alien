use std::collections::BTreeMap;

use alien_permission_types::PermissionSet;

use super::{catalog, invalid, sorted, types::*, Result};

fn statement_key(statement: &AwsStatement) -> String {
    serde_json::json!({"effect": statement.effect, "actions": statement.actions,
        "resources": statement.resources, "condition": statement.condition})
    .to_string()
}

pub fn for_catalog(sets: &[PermissionSet]) -> Vec<AwsDeclaration> {
    let mut result = Vec::new();
    for set in sets {
        for permission in set.platforms.aws.iter().flatten() {
            let actions = permission.grant.actions.clone().unwrap_or_default();
            if actions.is_empty() {
                continue;
            }
            for binding in [
                permission.binding.stack.as_ref(),
                permission.binding.resource.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                result.push(AwsDeclaration {
                    effect: permission.effect.clone(),
                    actions: actions.clone(),
                    resources: binding.resources.clone(),
                    condition: binding.condition.as_ref().map(|condition| {
                        condition
                            .iter()
                            .map(|(key, values)| {
                                (
                                    key.clone(),
                                    values
                                        .iter()
                                        .map(|(key, value)| (key.clone(), value.clone()))
                                        .collect(),
                                )
                            })
                            .collect()
                    }),
                    reason: permission
                        .description
                        .clone()
                        .unwrap_or_else(|| set.description.clone()),
                });
            }
        }
    }
    result
}

pub fn collect(plugins: &[CatalogPlugin]) -> Vec<AwsStatement> {
    let mut statements = BTreeMap::<String, AwsStatement>::new();
    for plugin in plugins.iter().filter(|plugin| plugin.enabled) {
        for operation in &plugin.operations {
            for permission in &operation.permissions.aws {
                let source = Source {
                    plugin: plugin.name.clone(),
                    operation: operation.name.clone(),
                    reason: permission.reason.clone(),
                };
                let reason = format!("{}/{}: {}", source.plugin, source.operation, source.reason)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                for action in sorted(&permission.actions) {
                    for resource in sorted(&permission.resources) {
                        let mut statement = AwsStatement {
                            effect: permission.effect.clone(),
                            actions: vec![action.clone()],
                            resources: vec![resource],
                            condition: permission
                                .condition
                                .clone()
                                .filter(|condition| !condition.is_empty()),
                            reasons: vec![],
                            sources: vec![],
                        };
                        let key = statement_key(&statement);
                        let existing = statements.entry(key).or_insert_with(|| {
                            statement.reasons.push(reason.clone());
                            statement.sources.push(source.clone());
                            statement
                        });
                        if !existing.reasons.contains(&reason) {
                            existing.reasons.push(reason.clone());
                            existing.reasons.sort();
                        }
                        if !existing.sources.contains(&source) {
                            existing.sources.push(source.clone());
                            existing.sources.sort();
                        }
                    }
                }
            }
        }
    }
    statements.into_values().collect()
}

pub fn compact(statements: &[AwsStatement]) -> Vec<AwsStatement> {
    let mut result = statements.to_vec();
    for actions in [true, false] {
        let mut groups = BTreeMap::<String, AwsStatement>::new();
        for statement in result {
            let mut key = statement.clone();
            if actions {
                key.actions.clear();
            } else {
                key.resources.clear();
            }
            let key = serde_json::to_string(&key).expect("permission statement serializes");
            if let Some(existing) = groups.get_mut(&key) {
                let (target, source) = if actions {
                    (&mut existing.actions, &statement.actions)
                } else {
                    (&mut existing.resources, &statement.resources)
                };
                target.extend(source.iter().cloned());
                *target = sorted(target);
            } else {
                groups.insert(key, statement);
            }
        }
        result = groups.into_values().collect();
    }
    result.sort_by_key(statement_key);
    result
}

const GLOBAL_METADATA_ACTIONS: &[&str] = &[
    "cloudwatch:DescribeAlarms",
    "cloudwatch:GetMetricStatistics",
    "ec2:DescribeInstances",
    "ec2:DescribeInstanceStatus",
    "ec2:DescribeRouteTables",
    "ec2:DescribeSecurityGroupRules",
    "ec2:DescribeSubnets",
    "ec2:DescribeVolumes",
    "ec2:DescribeVpcs",
    "elasticloadbalancing:DescribeLoadBalancers",
    "elasticloadbalancing:DescribeTargetGroups",
    "elasticloadbalancing:DescribeTargetHealth",
    "logs:DescribeLogGroups",
    "rds:DescribeDBClusters",
    "rds:DescribeDBInstances",
    "rds:DescribeEvents",
    "sqs:ListQueues",
];

fn partition(value: &str) -> bool {
    matches!(value, "aws" | "aws-cn" | "aws-us-gov")
}

fn valid_bucket_arn(value: &str) -> bool {
    let parts: Vec<_> = value.split(':').collect();
    matches!(parts.as_slice(), ["arn", p, "s3", "", "", name]
        if partition(p) && (3..=63).contains(&name.len())
        && name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-'))
        && name.bytes().next().is_some_and(|byte| byte.is_ascii_alphanumeric())
        && name.bytes().last().is_some_and(|byte| byte.is_ascii_alphanumeric()))
}

fn valid_queue_arn(value: &str) -> bool {
    let parts: Vec<_> = value.split(':').collect();
    let ["arn", p, "sqs", region, account, name] = parts.as_slice() else {
        return false;
    };
    let region_parts: Vec<_> = region.split('-').collect();
    let valid_region = region_parts.len() >= 2
        && region_parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        });
    let queue_name = name.strip_suffix(".fifo").unwrap_or(name);
    partition(p)
        && valid_region
        && account.len() == 12
        && account.bytes().all(|byte| byte.is_ascii_digit())
        && !queue_name.is_empty()
        && name.len() <= 80
        && queue_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn compile(
    statements: &[AwsStatement],
    ceilings: &AwsResourceCeilings,
) -> Result<Vec<AwsStatement>> {
    for (name, entries, valid) in [
        (
            "s3BucketArns",
            &ceilings.s3_bucket_arns,
            valid_bucket_arn as fn(&str) -> bool,
        ),
        (
            "sqsQueueArns",
            &ceilings.sqs_queue_arns,
            valid_queue_arn as fn(&str) -> bool,
        ),
    ] {
        if let Some(entry) = entries.iter().find(|entry| !valid(entry)) {
            return invalid(format!("Installer-provided {name} entry '{entry}' must be an exact resource ARN without wildcards"), "installer-resource-ceiling", entry);
        }
    }
    let mut compiled = Vec::new();
    for statement in statements {
        for action in &statement.actions {
            if action.contains(['*', '?']) {
                return invalid(
                    format!("AWS action '{action}' is not an exact operation permission"),
                    action,
                    &statement.resources.join(","),
                );
            }
            if !catalog()
                .aws
                .values()
                .any(|capability| capability.actions.contains(action))
            {
                return invalid(
                    format!("AWS action '{action}' has no reviewed operation capability"),
                    action,
                    &statement.resources.join(","),
                );
            }
            for resource in &statement.resources {
                let replaced = match (action.as_str(), resource.as_str()) {
                    ("s3:ListBucket", "arn:aws:s3:::*") => {
                        Some(("s3BucketArns", sorted(&ceilings.s3_bucket_arns)))
                    }
                    (
                        "s3:GetObject" | "s3:GetObjectAttributes" | "s3:GetObjectTagging",
                        "arn:aws:s3:::*/*",
                    ) => Some((
                        "s3BucketArns",
                        sorted(&ceilings.s3_bucket_arns)
                            .into_iter()
                            .map(|arn| format!("{arn}/*"))
                            .collect(),
                    )),
                    ("sqs:GetQueueAttributes", "arn:aws:sqs:*:*:*") => {
                        Some(("sqsQueueArns", sorted(&ceilings.sqs_queue_arns)))
                    }
                    _ => None,
                };
                let resources = if let Some((name, resources)) = replaced {
                    if resources.is_empty() {
                        return invalid(format!("AWS operation '{action}' requires at least one installer-provided {name} entry"), action, resource);
                    }
                    resources
                } else if resource == "*" && GLOBAL_METADATA_ACTIONS.contains(&action.as_str()) {
                    vec![resource.clone()]
                } else if resource.contains(['*', '?']) {
                    return invalid(format!("AWS operation '{action}' declares unsupported wildcard resource '{resource}'"), action, resource);
                } else {
                    vec![resource.clone()]
                };
                compiled.push(AwsStatement {
                    actions: vec![action.clone()],
                    resources,
                    ..statement.clone()
                });
            }
        }
    }
    Ok(compact(&compiled))
}
