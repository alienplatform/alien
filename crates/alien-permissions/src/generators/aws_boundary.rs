//! The setup-owned ceiling for roles written by management.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

use alien_core::{
    AwsBindingSpec, PermissionSet, PermissionSetReference, Stack, ALIEN_STACK_TAG_KEY,
};

use alien_error::AlienError;
use serde_json::{json, Value};

use crate::{
    aws_resource_policies::without_refs_the_stack_policy_grants, get_permission_set,
    list_permission_set_ids, BindingTarget, ErrorData, PermissionContext, Result,
};

use super::{
    aws_boundary_coverage::action_patterns_overlap, AwsIamStatement, AwsRuntimePermissionsGenerator,
};

const MANAGED_POLICY_LIMIT: usize = 6_144;

/// Resource-policy and sharing writes that could expose stack resources outside the
/// account or to the public, including creates that accept a resource policy. Runtime
/// catalog grants never need them. An inline application grant that names one of these
/// exactly lifts the Deny only on that grant's own resources; entries with wildcards, which
/// keep the boundary within IAM's size limit, are never lifted.
const SHARING_ACTIONS: &[&str] = &[
    "aoss:*AccessPolicy",
    "aoss:*SecurityPolicy",
    "backup:PutBackupVaultAccessPolicy",
    "codebuild:PutResourcePolicy",
    "dlm:*LifecyclePolicy",
    "dynamodb:PutResourcePolicy",
    "ec2:CopySnapshot",
    "ec2:CreateVpcPeeringConnection",
    "ec2:Modify*ImageAttribute",
    "ec2:ModifySnapshotAttribute",
    "ec2:ModifyVpcEndpointServicePermissions",
    "ecr:*RepositoryCreationTemplate",
    "ecr:PutRegistryPolicy",
    "ecr:PutReplicationConfiguration",
    "ecr:SetRepositoryPolicy",
    "elasticfilesystem:PutFileSystemPolicy",
    "es:Create*Domain",
    "es:Update*DomainConfig",
    "events:PutPermission",
    "glacier:SetVaultAccessPolicy",
    "glue:PutResourcePolicy",
    "kinesis:PutResourcePolicy",
    "kms:PutKeyPolicy",
    "lambda:Add*Permission",
    "lambda:CreateFunctionUrlConfig",
    "lambda:UpdateFunctionUrlConfig",
    "logs:PutDe*Policy",
    "logs:PutResourcePolicy",
    "logs:PutSubscriptionFilter",
    "ram:*",
    "rds:ModifyDB*SnapshotAttribute",
    "s3:*BucketPolicy",
    "s3:Put*PublicAccessBlock",
    "s3:PutAccessPointPolicy*",
    "s3:PutBucketAcl",
    "s3:PutBucketOwnershipControls",
    "s3:PutMultiRegionAccessPointPolicy",
    "s3:PutReplicationConfiguration",
    "secretsmanager:PutResourcePolicy",
    "ses:*IdentityPolicy",
    "sns:AddPermission",
    "sns:CreateTopic",
    "sns:SetTopicAttributes",
    "sqs:AddPermission",
    "sqs:CreateQueue",
    "sqs:SetQueueAttributes",
];

/// Render the boundary including inline and ARN-selected application grants.
///
/// The resolver receives the profile name and scope (`"*"` or a resource id). It must
/// return the same context used to compile that identity's grants, including actual
/// resource names for resource scopes. It is called only for scopes with inline AWS
/// permissions, catalog bindings that select complete resource ARNs, or container
/// volume encryption keys (using the container's permission profile name). Management
/// permissions are deliberately not part of this ceiling. Other named catalog grants
/// are already included by [`aws_permissions_boundary`].
/// Global selected sets also contribute their resource binding to matching resource
/// types, just as workload policy writers do. Those calls resolve the physical
/// resource context, even when the profile does not explicitly name the resource.
pub fn aws_stack_permissions_boundary(
    stack: &Stack,
    context: &PermissionContext,
    resolve_context: impl FnMut(&str, &str) -> Result<PermissionContext>,
) -> Result<Value> {
    stack_boundary(stack, context, resolve_context, Rendering::Current)
}

/// The ceiling the stack's runtime needs, to check that an installed boundary still covers it.
///
/// Unlike [`aws_stack_permissions_boundary`], it lists each scoped pattern without merging any
/// for size ([`WIDENED_SCOPED_PATTERNS`]), so boundaries installed before or after a pattern
/// was merged both cover it. It is never installed, so it has no size limit. The resolver is
/// called as for [`aws_stack_permissions_boundary`].
pub fn aws_stack_boundary_requirement(
    stack: &Stack,
    context: &PermissionContext,
    resolve_context: impl FnMut(&str, &str) -> Result<PermissionContext>,
) -> Result<Value> {
    stack_boundary(stack, context, resolve_context, Rendering::Requirement)
}

fn stack_boundary(
    stack: &Stack,
    context: &PermissionContext,
    mut resolve_context: impl FnMut(&str, &str) -> Result<PermissionContext>,
    rendering: Rendering,
) -> Result<Value> {
    let generator = AwsRuntimePermissionsGenerator::new();
    let mut statements = Vec::new();
    let mut exact_resources = BTreeMap::new();
    let mut inline_grants = Vec::new();
    for (profile_name, profile) in stack.permission_profiles() {
        for (scope, references) in &profile.0 {
            if !references
                .iter()
                .any(|reference| resolved_boundary_set(reference).is_some())
            {
                continue;
            }
            let grant_context = resolve_context(profile_name, scope)?;
            let target = if scope == "*" {
                BindingTarget::Stack
            } else {
                BindingTarget::Resource
            };
            for reference in references {
                if let Some(set) = resolved_boundary_set(reference) {
                    collect_resolved_grants(
                        reference,
                        &grant_source(profile_name, scope, reference),
                        generator
                            .generate_policy(set, target, &grant_context)?
                            .statement,
                        grant_context.resource_name.as_deref(),
                        &mut exact_resources,
                        &mut statements,
                        &mut inline_grants,
                    );
                }
            }
        }
        for (resource_id, entry) in stack.resources() {
            let type_prefix = format!("{}/", entry.config.resource_type());
            let references: Vec<_> = profile
                .0
                .get("*")
                .into_iter()
                .flatten()
                .filter(|reference| {
                    reference.id().starts_with(&type_prefix)
                        && resolved_boundary_set(reference).is_some()
                })
                .cloned()
                .collect();
            if references.is_empty() {
                continue;
            }
            let grant_context = resolve_context(profile_name, resource_id)?;
            let references = without_refs_the_stack_policy_grants(
                stack,
                profile_name,
                profile,
                resource_id,
                references,
                &generator,
                &grant_context,
            )?;
            for reference in &references {
                if let Some(set) = resolved_boundary_set(reference) {
                    collect_resolved_grants(
                        reference,
                        &grant_source(profile_name, resource_id, reference),
                        generator
                            .generate_policy(set, BindingTarget::Resource, &grant_context)?
                            .statement,
                        grant_context.resource_name.as_deref(),
                        &mut exact_resources,
                        &mut statements,
                        &mut inline_grants,
                    );
                }
            }
        }
    }
    let mut volume_keys = BTreeSet::new();
    for (_, entry) in stack.resources() {
        let Some(container) = entry.config.downcast_ref::<alien_core::Container>() else {
            continue;
        };
        let Some(key) = container
            .persistent_storage
            .as_ref()
            .and_then(|storage| storage.encryption_key.as_ref())
        else {
            continue;
        };
        if volume_keys.insert(&key.id) {
            let grant_context = resolve_context(&container.permissions, &key.id)?;
            let reference = PermissionSetReference::from_name("key/native-volume");
            let set = get_permission_set(reference.id()).ok_or_else(|| {
                AlienError::new(ErrorData::InvalidPermissionSet {
                    message: format!("Catalog permission set '{}' is missing", reference.id()),
                })
            })?;
            collect_resolved_grants(
                &reference,
                &format!(
                    "container '{}' volume key '{}' ({})",
                    container.id,
                    key.id,
                    reference.id()
                ),
                generator
                    .generate_policy(set, BindingTarget::Resource, &grant_context)?
                    .statement,
                grant_context.resource_name.as_deref(),
                &mut exact_resources,
                &mut statements,
                &mut inline_grants,
            );
        }
    }
    render_boundary(
        context,
        &statements,
        exact_resources,
        &inline_grants,
        rendering,
    )
}

/// A compiled grant and the grants it came from, which name the largest statements when the
/// boundary does not fit.
#[derive(Debug, Clone)]
struct SourcedStatement {
    statement: AwsIamStatement,
    sources: BTreeSet<String>,
    /// The grants each action came from, including the runtime catalog's.
    action_sources: BTreeMap<String, BTreeSet<String>>,
}

impl SourcedStatement {
    fn new(statement: AwsIamStatement, source: String) -> Self {
        let sources = BTreeSet::from([source]);
        let action_sources = statement
            .action
            .iter()
            .map(|action| (action.clone(), sources.clone()))
            .collect();
        Self {
            statement,
            sources,
            action_sources,
        }
    }
}

fn grant_source(profile_name: &str, scope: &str, reference: &PermissionSetReference) -> String {
    let scope = if scope == "*" {
        "stack-wide".to_string()
    } else {
        format!("on '{scope}'")
    };
    format!("profile '{profile_name}' {scope} ({})", reference.id())
}

/// How `render_boundary` renders the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rendering {
    /// The document setup installs: copies only grants the scoped `NotAction: iam:*` Allow does
    /// not already admit, merges scoped patterns within a service
    /// ([`WIDENED_SCOPED_PATTERNS`]), and must fit IAM's managed-policy limit.
    Current,
    /// What an installed boundary must cover: as `Current`, without merging scoped patterns or
    /// limiting the size.
    Requirement,
    /// Copies every grant and lists each scoped pattern, as the renderer did before admitted
    /// grants were skipped and patterns merged. Tests compare the two renderings.
    #[cfg(test)]
    Earlier,
}

fn collect_resolved_grants(
    reference: &PermissionSetReference,
    source: &str,
    grants: Vec<AwsIamStatement>,
    resource_name: Option<&str>,
    exact_resources: &mut BTreeMap<String, BTreeSet<String>>,
    statements: &mut Vec<SourcedStatement>,
    inline_grants: &mut Vec<AwsIamStatement>,
) {
    for mut grant in grants {
        if matches!(reference, PermissionSetReference::Inline(_)) {
            inline_grants.push(grant.clone());
        }
        // Named ARN selectors are setup-approved resources. Fine-grained service
        // conditions remain in identity policies; inline definitions remain intact.
        if matches!(reference, PermissionSetReference::Name(_))
            && grant.effect == "Allow"
            && grant.not_resource.is_empty()
            && grant.action.iter().all(|action| {
                action.split_once(':').is_some_and(|(service, _)| {
                    !service.eq_ignore_ascii_case("iam") && !service.contains(['*', '?'])
                })
            })
        {
            grant.resource.retain(|resource| {
                if Some(resource.as_str()) == resource_name {
                    exact_resources
                        .entry(resource.clone())
                        .or_default()
                        .insert(source.to_string());
                    false
                } else {
                    true
                }
            });
        }
        if !grant.resource.is_empty() || !grant.not_resource.is_empty() {
            statements.push(SourcedStatement::new(grant, source.to_string()));
        }
    }
}

/// Render the stack permissions boundary from the runtime catalog and compiled inline grants.
///
/// `custom_statements` contains runtime inline grants and catalog grants selecting complete
/// resource ARNs, compiled with their actual resource contexts. Use [`aws_stack_permissions_boundary`]
/// to collect these from a stack. Setup must approve changes to these grants. Only name-based
/// catalog bindings are widened to the stack prefix; complete ARN selectors cannot be widened.
/// Existing stack ownership tags provide a non-IAM ceiling for unprefixed resources.
/// Other unprefixed grants retain their actions and conditions.
/// Returns an error if interpolation fails or the document exceeds IAM's managed-policy limit.
pub fn aws_permissions_boundary(
    context: &PermissionContext,
    custom_statements: &[AwsIamStatement],
) -> Result<Value> {
    let sourced: Vec<_> = custom_statements
        .iter()
        .enumerate()
        .map(|(index, statement)| {
            SourcedStatement::new(
                statement.clone(),
                if statement.sid.is_empty() {
                    format!("custom statement {index}")
                } else {
                    format!("custom statement '{}'", statement.sid)
                },
            )
        })
        .collect();
    render_boundary(
        context,
        &sourced,
        BTreeMap::new(),
        custom_statements,
        Rendering::Current,
    )
}

fn render_boundary(
    context: &PermissionContext,
    custom_statements: &[SourcedStatement],
    exact_resources: BTreeMap<String, BTreeSet<String>>,
    exception_grants: &[AwsIamStatement],
    rendering: Rendering,
) -> Result<Value> {
    let prefix = context.stack_prefix.as_deref().ok_or_else(|| {
        AlienError::new(ErrorData::VariableNotFound {
            variable: "stackPrefix".to_string(),
        })
    })?;
    let ownership_key = format!("aws:ResourceTag/{ALIEN_STACK_TAG_KEY}");
    let mut scoped_resources: BTreeSet<String> = exact_resources.keys().cloned().collect();
    let mut catalog_context = context.clone();
    catalog_context.resource_name = Some(format!("{prefix}-*"));
    let mut statements = Vec::new();
    let generator = AwsRuntimePermissionsGenerator::new();
    let mut ids = list_permission_set_ids();
    ids.sort();
    for id in ids.into_iter().filter(|id| is_runtime_set(id)) {
        let set = get_permission_set(id).ok_or_else(|| {
            AlienError::new(ErrorData::InvalidPermissionSet {
                message: format!("Catalog permission set '{id}' is missing"),
            })
        })?;
        for entry in set
            .platforms
            .aws
            .iter()
            .flatten()
            .filter(|e| e.effect.is_allow())
        {
            for binding in [&entry.binding.stack, &entry.binding.resource]
                .into_iter()
                .flatten()
            {
                let mut entry = entry.clone();
                let mut binding = binding.clone();
                // Complete ARN selectors (such as KMS keys) require the stack resolver.
                binding
                    .resources
                    .retain(|resource| resource != "${resourceName}");
                if binding.resources.is_empty() && binding.not_resources.is_empty() {
                    continue;
                }
                // Some resource bindings already include the prefix before resourceName.
                for resource in &mut binding.resources {
                    *resource =
                        resource.replace("${stackPrefix}-${resourceName}", "${resourceName}");
                }
                entry.binding.stack = Some(binding.clone());
                let mut single = set.clone();
                single.platforms.aws = Some(vec![entry]);
                let policy =
                    generator.generate_policy(&single, BindingTarget::Stack, &catalog_context)?;
                for mut statement in policy.statement {
                    // Existing ownership covers non-IAM runtime operations even when
                    // identity policies impose additional service-specific conditions.
                    if statement
                        .condition
                        .as_ref()
                        .and_then(|conditions| conditions.get("StringEquals"))
                        .and_then(|values| values.get(&ownership_key))
                        .is_some_and(|value| value.as_string() == Some(prefix))
                        && statement.action.iter().all(|action| {
                            action.split_once(':').is_some_and(|(service, _)| {
                                !service.eq_ignore_ascii_case("iam")
                                    && !service.contains(['*', '?'])
                            })
                        })
                    {
                        continue;
                    }
                    let mut unprefixed = Vec::new();
                    for (template, resource) in binding.resources.iter().zip(statement.resource) {
                        if template.contains("${stackPrefix}")
                            || template.contains("${resourceName}")
                        {
                            scoped_resources.insert(resource);
                        } else {
                            unprefixed.push(resource);
                        }
                    }
                    statement.resource = unprefixed;
                    if !statement.resource.is_empty() || !statement.not_resource.is_empty() {
                        merge_statement(
                            &mut statements,
                            SourcedStatement::new(statement, format!("{CATALOG_SOURCE} ({id})")),
                        );
                    }
                }
            }
        }
    }
    // A trailing wildcard also covers longer catalog patterns such as log streams and objects.
    let scoped_resources: Vec<_> = scoped_resources
        .iter()
        .filter(|resource| {
            !scoped_resources.iter().any(|other| {
                other != *resource
                    && other
                        .strip_suffix('*')
                        .is_some_and(|prefix| resource.starts_with(prefix))
            })
        })
        .collect();
    // Copying a grant the scoped Allow already admits would only add size: least-privilege
    // grants on prefixed resources (such as one per secret input and consumer) would otherwise
    // grow the boundary with the stack. Denies stay, and still lift no exemptions below.
    for custom in custom_statements {
        let admitted = match rendering {
            Rendering::Current | Rendering::Requirement => {
                admitted_by_scoped_allow(&custom.statement, &scoped_resources)
            }
            #[cfg(test)]
            Rendering::Earlier => false,
        };
        if !admitted {
            merge_statement(&mut statements, custom.clone());
        }
    }
    let scoped_resources = match rendering {
        Rendering::Current => widen_scoped_patterns(context, prefix, &scoped_resources)?,
        Rendering::Requirement => scoped_resources.into_iter().cloned().collect(),
        #[cfg(test)]
        Rendering::Earlier => scoped_resources.into_iter().cloned().collect(),
    };
    // Rendered statements and resources that come from the stack's grants rather than the
    // stack-independent ceiling are attributed to their sources, to name them on overflow.
    let mut contributions = Contributions::default();
    for resource in &scoped_resources {
        if let Some(sources) = exact_resources.get(resource.as_str()) {
            // The quoted resource and its separator.
            contributions.add(
                sources,
                Added::ExactResource,
                resource.chars().count() + 3,
                resource,
            );
        }
    }
    let mut rendered = vec![
        json!({
            "Effect": "Allow", "NotAction": "iam:*", "Resource": scoped_resources,
        }),
        json!({
            "Effect": "Allow", "NotAction": "iam:*", "Resource": "*",
            "Condition": {"StringEquals": {ownership_key: prefix}},
        }),
    ];
    for sourced in &statements {
        let statement = render_statement(&sourced.statement);
        let example = sourced
            .statement
            .resource
            .first()
            .or(sourced.statement.not_resource.first())
            .map_or("", String::as_str);
        let from_catalog = |sources: &BTreeSet<String>| {
            sources
                .iter()
                .any(|source| source.starts_with(CATALOG_SOURCE))
        };
        if !sourced.action_sources.values().any(from_catalog) {
            // The statement and its separator.
            contributions.add(
                &sourced.sources,
                Added::Statement,
                minified_size(&statement) + 1,
                example,
            );
        } else {
            // Grants merged into a catalog statement add only the actions the catalog lacks.
            // Each group of grants is charged what removing its actions saves; the charges add
            // up to the statement's size beyond the catalog's.
            let mut added: BTreeMap<&BTreeSet<String>, BTreeSet<&str>> = BTreeMap::new();
            for (action, sources) in &sourced.action_sources {
                if !from_catalog(sources) {
                    added.entry(sources).or_default().insert(action);
                }
            }
            let mut remaining = sourced.statement.clone();
            for (sources, actions) in added {
                let size = minified_size(&render_statement(&remaining));
                remaining
                    .action
                    .retain(|action| !actions.contains(action.as_str()));
                contributions.add(
                    sources,
                    Added::Actions,
                    size - minified_size(&render_statement(&remaining)),
                    example,
                );
            }
        }
        rendered.push(statement);
    }
    // Denies are never dropped. Each is exempted only on the resources of grants that name its
    // action exactly; a wildcard action, a condition or NotResource never lifts it.
    let exempt = |action: &str| -> BTreeSet<&str> {
        exception_grants
            .iter()
            .filter(|grant| {
                grant.effect == "Allow"
                    && grant.condition.is_none()
                    && grant.not_resource.is_empty()
                    && grant.action.iter().any(|granted| {
                        !granted.contains(['*', '?']) && granted.eq_ignore_ascii_case(action)
                    })
            })
            .flat_map(|grant| grant.resource.iter().map(String::as_str))
            .collect()
    };
    let mut denies: Vec<(BTreeSet<&str>, Vec<&str>)> = Vec::new();
    for action in SHARING_ACTIONS {
        let resources = exempt(action);
        match denies
            .iter_mut()
            .find(|(exempted, _)| *exempted == resources)
        {
            Some((_, actions)) => actions.push(action),
            None => denies.push((resources, vec![action])),
        }
    }
    denies.sort();
    let grant_exemptions = BTreeSet::from([EXEMPTED_DENIES.to_string()]);
    let mut add_deny = |deny: Option<Value>, exempted: &BTreeSet<&str>| {
        if let Some(deny) = deny {
            if let Some(first) = exempted.first() {
                contributions.add(
                    &grant_exemptions,
                    Added::Statement,
                    minified_size(&deny) + 1,
                    first,
                );
            }
            rendered.push(deny);
        }
    };
    for (exempted, actions) in &denies {
        add_deny(deny_statement(json!(actions), exempted, None), exempted);
    }
    // EC2 creates grants on stack keys to attach encrypted volumes.
    let key_grants = exempt("kms:CreateGrant");
    add_deny(
        deny_statement(
            json!(["kms:CreateGrant"]),
            &key_grants,
            Some(json!({"BoolIfExists": {"kms:GrantIsForAWSResource": "false"}})),
        ),
        &key_grants,
    );
    let policy = json!({"Version": "2012-10-17", "Statement": rendered});
    let size = minified_size(&policy);
    if size > MANAGED_POLICY_LIMIT && rendering != Rendering::Requirement {
        return Err(AlienError::new(ErrorData::PermissionsBoundaryTooLarge {
            size,
            limit: MANAGED_POLICY_LIMIT,
            largest_contributors: contributions.describe(size),
        }));
    }
    Ok(policy)
}

/// Source label prefix of the stack-independent runtime catalog statements.
const CATALOG_SOURCE: &str = "runtime catalog";

/// Label for sharing Denies split out because grants name their actions exactly.
const EXEMPTED_DENIES: &str = "sharing Denies lifted for grants naming their actions";

/// Scoped patterns of one service rendered as a single wider pattern, with `{region}`,
/// `{account}` and `{prefix}` standing for the context's values. A wider pattern only admits
/// more names that start with the stack prefix right after a `/` separator, in a service whose
/// catalog grants already reach the stack's resources there. It is used only when every member
/// is scoped, and grants are still proven admitted against the members.
///
/// - CodeBuild `*/<prefix>-*` newly matches other CodeBuild resource types named with the
///   prefix, such as `build/<prefix>-…`, `build-batch/<prefix>-…`, `report/<prefix>-…` and
///   `fleet/<prefix>-…`.
/// - CloudWatch Logs `log-group:/aws/*/<prefix>-*` newly matches log groups whose last path
///   segment, or any segment after `/aws/`, starts with the prefix, such as
///   `/aws/ecs/<prefix>-…` or `/aws/rds/cluster/<prefix>-…`, and the CodeBuild log group
///   itself rather than only its streams.
const WIDENED_SCOPED_PATTERNS: &[(&[&str], &str)] = &[
    (
        &[
            "arn:aws:codebuild:{region}:{account}:project/{prefix}-*",
            "arn:aws:codebuild:{region}:{account}:report-group/{prefix}-*",
        ],
        "arn:aws:codebuild:{region}:{account}:*/{prefix}-*",
    ),
    (
        &[
            "arn:aws:logs:{region}:{account}:log-group:/aws/codebuild/{prefix}-*:*",
            "arn:aws:logs:{region}:{account}:log-group:/aws/lambda/{prefix}-*",
        ],
        "arn:aws:logs:{region}:{account}:log-group:/aws/*/{prefix}-*",
    ),
];

/// The scoped patterns to render, with each complete group of [`WIDENED_SCOPED_PATTERNS`]
/// replaced by its wider pattern.
fn widen_scoped_patterns(
    context: &PermissionContext,
    prefix: &str,
    scoped: &[&String],
) -> Result<Vec<String>> {
    let variable = |name: &str, value: &Option<String>| {
        value.clone().ok_or_else(|| {
            AlienError::new(ErrorData::VariableNotFound {
                variable: name.to_string(),
            })
        })
    };
    let region = variable("awsRegion", &context.aws_region)?;
    let account = variable("awsAccountId", &context.aws_account_id)?;
    let render = |pattern: &str| {
        pattern
            .replace("{region}", &region)
            .replace("{account}", &account)
            .replace("{prefix}", prefix)
    };
    let mut rendered: BTreeSet<String> = scoped.iter().map(|pattern| pattern.to_string()).collect();
    for (members, widened) in WIDENED_SCOPED_PATTERNS {
        let members: Vec<_> = members.iter().map(|member| render(member)).collect();
        if members.iter().all(|member| rendered.contains(member)) {
            for member in &members {
                rendered.remove(member);
            }
            rendered.insert(render(widened));
        }
    }
    Ok(rendered.into_iter().collect())
}

/// Characters the stack's own grants add to the boundary, by grant source. Runtime catalog
/// statements, the two base Allows and unexempted Denies are the stack-independent rest.
#[derive(Default)]
struct Contributions(BTreeMap<String, Contribution>);

#[derive(Default)]
struct Contribution {
    statements: usize,
    exact_resources: usize,
    extended_catalog_statements: usize,
    characters: usize,
    example: String,
}

/// What a grant source adds to the rendered document.
#[derive(Debug, Clone, Copy)]
enum Added {
    /// A statement of its own.
    Statement,
    /// An exact resource in the scoped Allow.
    ExactResource,
    /// Actions in a runtime catalog statement with the same resources and condition.
    Actions,
}

impl Contributions {
    fn add(&mut self, sources: &BTreeSet<String>, added: Added, characters: usize, example: &str) {
        let label: Vec<_> = sources
            .iter()
            .filter(|source| !source.starts_with(CATALOG_SOURCE))
            .map(String::as_str)
            .collect();
        if label.is_empty() {
            return;
        }
        let entry = self.0.entry(label.join(", ")).or_default();
        match added {
            Added::Statement => entry.statements += 1,
            Added::ExactResource => entry.exact_resources += 1,
            Added::Actions => entry.extended_catalog_statements += 1,
        }
        entry.characters += characters;
        if entry.example.is_empty() {
            entry.example = example.to_string();
        }
    }

    /// The largest contributors, then the stack-independent remainder of `size`.
    fn describe(&self, size: usize) -> String {
        const LISTED: usize = 5;
        let mut largest: Vec<_> = self.0.iter().collect();
        largest.sort_by(|(_, left), (_, right)| right.characters.cmp(&left.characters));
        let attributed: usize = largest.iter().map(|(_, c)| c.characters).sum();
        let mut described: Vec<_> = largest
            .iter()
            .take(LISTED)
            .map(|(label, contribution)| {
                let counted = |count: usize, one: &str, many: &str| match count {
                    0 => None,
                    1 => Some(format!("1 {one}")),
                    count => Some(format!("{count} {many}")),
                };
                let parts: Vec<_> = [
                    counted(contribution.statements, "statement", "statements"),
                    counted(
                        contribution.exact_resources,
                        "exact resource in the scoped Allow",
                        "exact resources in the scoped Allow",
                    ),
                    counted(
                        contribution.extended_catalog_statements,
                        "runtime catalog statement it adds actions to",
                        "runtime catalog statements it adds actions to",
                    ),
                ]
                .into_iter()
                .flatten()
                .collect();
                format!(
                    "{label}: {} characters in {}, e.g. '{}'",
                    contribution.characters,
                    parts.join(" and "),
                    contribution.example
                )
            })
            .collect();
        if largest.len() > LISTED {
            described.push(format!("{} more grant sources", largest.len() - LISTED));
        }
        described.push(format!(
            "stack-independent statements: {} characters",
            size.saturating_sub(attributed)
        ));
        described.join("; ")
    }
}

/// Characters IAM counts against the managed-policy limit: the document without whitespace.
fn minified_size(value: &Value) -> usize {
    value
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .count()
}

/// Select legacy or bounded role grants using the installed setup boundary.
pub(super) fn include_role_statement(
    permission_set: &PermissionSet,
    label: Option<&str>,
    context: &PermissionContext,
) -> bool {
    if let Some(label) = label {
        if label.starts_with("bounded-role-") {
            return context.aws_permissions_boundary_arn.is_some();
        }
        if label.starts_with("legacy-role-") {
            return context.aws_permissions_boundary_arn.is_none();
        }
    }
    if permission_set.id != "container/provision" {
        return true;
    }
    match label {
        Some("deny-volume-backup-role-changes") => context.aws_permissions_boundary_arn.is_none(),
        _ => true,
    }
}

/// Older setups have no boundary policy or adoption grant. Only omit the catalog's
/// boundary condition; explicit custom conditions must keep their original meaning.
pub(super) fn role_write_binding<'a>(
    binding: &'a AwsBindingSpec,
    actions: &'a [String],
    context: &PermissionContext,
) -> (Cow<'a, AwsBindingSpec>, Cow<'a, [String]>) {
    let uses_setup_boundary = binding
        .condition
        .as_ref()
        .and_then(|conditions| conditions.get("StringEquals"))
        .and_then(|values| values.get("iam:PermissionsBoundary"))
        .is_some_and(|value| value.as_string() == Some("${awsPermissionsBoundaryArn}"));
    if context.aws_permissions_boundary_arn.is_some() || !uses_setup_boundary {
        return (Cow::Borrowed(binding), Cow::Borrowed(actions));
    }

    let mut binding = binding.clone();
    if let Some(conditions) = &mut binding.condition {
        if let Some(values) = conditions.get_mut("StringEquals") {
            values.shift_remove("iam:PermissionsBoundary");
            if values.is_empty() {
                conditions.shift_remove("StringEquals");
            }
        }
        if conditions.is_empty() {
            binding.condition = None;
        }
    }
    let actions = actions
        .iter()
        .filter(|action| action.as_str() != "iam:PutRolePermissionsBoundary")
        .cloned()
        .collect();
    (Cow::Owned(binding), Cow::Owned(actions))
}

fn resolved_boundary_set(reference: &PermissionSetReference) -> Option<&PermissionSet> {
    match reference {
        PermissionSetReference::Inline(set) if set.platforms.aws.is_some() => Some(set),
        PermissionSetReference::Name(id) if is_runtime_set(id) => {
            let set = get_permission_set(id)?;
            set.platforms
                .aws
                .as_ref()?
                .iter()
                .any(|entry| {
                    [&entry.binding.stack, &entry.binding.resource]
                        .into_iter()
                        .flatten()
                        .any(|binding| {
                            binding
                                .resources
                                .iter()
                                .any(|resource| resource == "${resourceName}")
                        })
                })
                .then_some(set)
        }
        _ => None,
    }
}

fn is_runtime_set(id: &str) -> bool {
    // Nested IDs such as `experimental/aws-opensearch/provision` end in the operation.
    let category = id.split('/').next().unwrap_or(id);
    let operation = id.rsplit_once('/').map_or("", |(_, operation)| operation);
    !matches!(category, "guard" | "remote-stack-management" | "observe")
        && !operation.starts_with("protect-")
        && !matches!(
            operation,
            "provision" | "management" | "heartbeat" | "telemetry"
        )
}

/// Whether the scoped `NotAction: iam:*` Allow admits every request `statement` allows, so
/// copying it would leave the boundary's allowed set unchanged. Anything not proven is kept:
/// Denies, `NotResource`, conditions, actions that may match IAM, and resources outside the
/// scoped patterns.
fn admitted_by_scoped_allow(statement: &AwsIamStatement, scoped_resources: &[&String]) -> bool {
    statement.effect == "Allow"
        && statement.condition.is_none()
        && statement.not_resource.is_empty()
        && !statement.action.is_empty()
        && !statement.resource.is_empty()
        && statement
            .action
            .iter()
            .all(|action| !action_patterns_overlap(action, "iam:*"))
        && statement.resource.iter().all(|resource| {
            scoped_resources
                .iter()
                .any(|scoped| scoped_pattern_contains(scoped, resource))
        })
}

/// Whether every ARN `resource` matches also matches the scoped pattern `scoped`.
///
/// Proven only for an identical pattern, or a `scoped` pattern that is a literal prefix followed
/// by `*` and that `resource` starts with. The resource's leading characters are then the same
/// literals, so all of its matches start with the prefix. Both patterns come from one context,
/// so template tokens such as `${AWS::StackName}` substitute identically in both. A prefix
/// ending inside a token could split it, so that case is not proven. ARN matching is
/// case-sensitive, and so is this comparison.
fn scoped_pattern_contains(scoped: &str, resource: &str) -> bool {
    if scoped == resource {
        return true;
    }
    let Some(prefix) = scoped.strip_suffix('*') else {
        return false;
    };
    let ends_inside_token = prefix
        .rfind("${")
        .is_some_and(|start| !prefix[start..].contains('}'));
    !prefix.is_empty()
        && !prefix.contains(['*', '?'])
        && !ends_inside_token
        && resource.starts_with(prefix)
}

fn merge_statement(statements: &mut Vec<SourcedStatement>, sourced: SourcedStatement) {
    let SourcedStatement {
        mut statement,
        sources,
        action_sources,
    } = sourced;
    statement.resource.sort();
    statement.resource.dedup();
    statement.not_resource.sort();
    statement.not_resource.dedup();
    if let Some(existing) = statements.iter_mut().find(|existing| {
        let existing = &existing.statement;
        existing.effect == statement.effect
            && existing.resource == statement.resource
            && existing.not_resource == statement.not_resource
            && existing.condition == statement.condition
    }) {
        existing.statement.action.extend(statement.action);
        existing.statement.action.sort();
        existing.statement.action.dedup();
        existing.sources.extend(sources);
        for (action, sources) in action_sources {
            existing
                .action_sources
                .entry(action)
                .or_default()
                .extend(sources);
        }
    } else {
        statement.action.sort();
        statement.action.dedup();
        statements.push(SourcedStatement {
            statement,
            sources,
            action_sources,
        });
    }
}

/// Case-insensitive IAM action match where `pattern` may use `*` and `?`.
pub(super) fn action_pattern_matches(pattern: &str, action: &str) -> bool {
    fn matches(pattern: &[u8], action: &[u8]) -> bool {
        match (pattern.first(), action.first()) {
            (None, None) => true,
            (Some(b'*'), _) => {
                matches(&pattern[1..], action)
                    || (!action.is_empty() && matches(pattern, &action[1..]))
            }
            (Some(b'?'), Some(_)) => matches(&pattern[1..], &action[1..]),
            (Some(left), Some(right)) => {
                left.eq_ignore_ascii_case(right) && matches(&pattern[1..], &action[1..])
            }
            _ => false,
        }
    }
    matches(pattern.as_bytes(), action.as_bytes())
}

/// A Deny on every resource except `exempted`; none when a grant exempts every resource.
fn deny_statement(
    actions: Value,
    exempted: &BTreeSet<&str>,
    condition: Option<Value>,
) -> Option<Value> {
    if exempted.contains("*") {
        return None;
    }
    let mut statement = json!({"Effect": "Deny", "Action": actions});
    if exempted.is_empty() {
        statement["Resource"] = json!("*");
    } else {
        statement["NotResource"] = json!(exempted);
    }
    if let Some(condition) = condition {
        statement["Condition"] = condition;
    }
    Some(statement)
}

fn render_statement(statement: &AwsIamStatement) -> Value {
    let mut value = json!({"Effect": statement.effect, "Action": statement.action});
    if !statement.resource.is_empty() {
        value["Resource"] = json!(statement.resource);
    }
    if !statement.not_resource.is_empty() {
        value["NotResource"] = json!(statement.not_resource);
    }
    if let Some(condition) = &statement.condition {
        value["Condition"] = json!(condition);
    }
    for key in ["Action", "Resource", "NotResource"] {
        if let Some(values) = value[key].as_array().filter(|values| values.len() == 1) {
            value[key] = values[0].clone();
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> PermissionContext {
        PermissionContext::new()
            .with_stack_prefix("s".repeat(40))
            .with_aws_region("ap-southeast-7")
            .with_aws_account_id("123456789012")
            .with_managing_account_id("210987654321")
    }

    fn denied_actions(boundary: &Value) -> Vec<String> {
        boundary["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["Effect"] == "Deny" && s.get("Condition").is_none())
            .flat_map(|s| match &s["Action"] {
                Value::Array(actions) => actions.clone(),
                action => vec![action.clone()],
            })
            .map(|action| action.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn runtime_catalog_never_needs_sharing_actions() {
        for id in list_permission_set_ids()
            .into_iter()
            .filter(|id| is_runtime_set(id))
        {
            for entry in get_permission_set(id)
                .unwrap()
                .platforms
                .aws
                .iter()
                .flatten()
            {
                if !entry.effect.is_allow() {
                    continue;
                }
                for action in entry.grant.actions.iter().flatten() {
                    for denied in SHARING_ACTIONS {
                        assert!(
                            !action_patterns_overlap(action, denied),
                            "{id} grants {action}, which the boundary denies"
                        );
                    }
                    if action_pattern_matches(action, "kms:CreateGrant") {
                        for binding in [&entry.binding.stack, &entry.binding.resource]
                            .into_iter()
                            .flatten()
                        {
                            let condition = binding.condition.as_ref().unwrap();
                            assert_eq!(
                                condition["Bool"]["kms:GrantIsForAWSResource"].as_string(),
                                Some("true"),
                                "{id} creates key grants outside AWS resources"
                            );
                        }
                    }
                }
            }
        }
    }

    fn inline(
        actions: &[&str],
        resources: &[&str],
        condition: Option<Value>,
    ) -> PermissionSetReference {
        let mut binding = json!({"resources": resources});
        if let Some(condition) = condition {
            binding["condition"] = condition;
        }
        PermissionSetReference::Inline(
            serde_json::from_value(json!({
                "id": "custom/sharing",
                "description": "Test grant",
                "platforms": {"aws": [{"grant": {"actions": actions}, "binding": {"stack": binding}}]}
            }))
            .unwrap(),
        )
    }

    #[test]
    fn grants_lift_sharing_denies_only_on_their_own_resources() {
        let boundary = aws_permissions_boundary(&context(), &[]).unwrap();
        let mut denied = denied_actions(&boundary);
        denied.sort();
        let mut expected: Vec<_> = SHARING_ACTIONS.iter().map(|a| a.to_string()).collect();
        expected.sort();
        assert_eq!(denied, expected);
        let grant_deny = boundary["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["Action"] == json!(["kms:CreateGrant"]))
            .unwrap();
        assert_eq!(grant_deny["Effect"], "Deny");
        assert_eq!(grant_deny["Resource"], "*");
        assert_eq!(
            grant_deny["Condition"],
            json!({"BoolIfExists": {"kms:GrantIsForAWSResource": "false"}})
        );

        let queue = "arn:aws:sqs:ap-southeast-7:123456789012:jobs";
        let stack = Stack::new("test".to_string())
            .permission(
                "application",
                alien_core::PermissionProfile(indexmap::indexmap! {
                    "*".to_string() => vec![
                        inline(&["sqs:SetQueueAttributes"], &[queue], None),
                        // Wildcards and conditions never lift a Deny.
                        inline(&["s3:*", "*", "kms:*", "sqs:Add*", "ram:*"], &["*"], None),
                        inline(
                            &["sns:SetTopicAttributes"],
                            &["*"],
                            Some(json!({"StringEquals": {"aws:ResourceTag/team": "jobs"}})),
                        ),
                        // Provision and management catalog sets never lift a Deny.
                        PermissionSetReference::from_name("storage/provision"),
                    ]
                }),
            )
            .build();
        let boundary =
            aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context())).unwrap();
        let denies: Vec<_> = boundary["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["Effect"] == "Deny")
            .collect();
        let everywhere: Vec<_> = SHARING_ACTIONS
            .iter()
            .filter(|action| **action != "sqs:SetQueueAttributes")
            .collect();
        assert_eq!(
            denies,
            vec![
                &json!({"Effect": "Deny", "Action": everywhere, "Resource": "*"}),
                &json!({"Effect": "Deny", "Action": ["sqs:SetQueueAttributes"], "NotResource": [queue]}),
                grant_deny,
            ]
        );
    }

    #[test]
    fn action_patterns_match_case_insensitively() {
        assert!(action_pattern_matches("s3:*", "s3:PutBucketPolicy"));
        assert!(action_pattern_matches(
            "S3:putbucket*",
            "s3:PutBucketPolicy"
        ));
        assert!(action_pattern_matches(
            "sqs:?etQueueAttributes",
            "sqs:SetQueueAttributes"
        ));
        assert!(!action_pattern_matches("s3:Get*", "s3:PutBucketPolicy"));
        assert!(!action_pattern_matches(
            "s3:PutBucket",
            "s3:PutBucketPolicy"
        ));
    }

    #[test]
    fn catalog_and_individual_arn_grants_fit_and_cover_runtime_permissions() {
        let context = context();
        let policy = aws_permissions_boundary(&context, &[]).unwrap();
        assert!(policy.to_string().len() <= MANAGED_POLICY_LIMIT);
        let context = context.with_resource_name(format!("{}-example", "s".repeat(40)));
        let generator = AwsRuntimePermissionsGenerator::new();
        for id in list_permission_set_ids()
            .into_iter()
            .filter(|id| is_runtime_set(id))
        {
            let set = get_permission_set(id).unwrap();
            for entry in set
                .platforms
                .aws
                .iter()
                .flatten()
                .filter(|e| e.effect.is_allow())
            {
                for (target, binding) in [
                    (BindingTarget::Stack, &entry.binding.stack),
                    (BindingTarget::Resource, &entry.binding.resource),
                ] {
                    if binding.is_none() {
                        continue;
                    }
                    let mut single = set.clone();
                    single.platforms.aws = Some(vec![entry.clone()]);
                    let key_context = context.clone().with_resource_name("arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab");
                    let arn_selected =
                        resolved_boundary_set(&PermissionSetReference::from_name(id)).is_some();
                    let actual_context = if arn_selected { &key_context } else { &context };
                    let required = generator
                        .generate_policy(&single, target, actual_context)
                        .unwrap();
                    let boundary = if arn_selected {
                        aws_permissions_boundary(&context, &required.statement).unwrap()
                    } else {
                        policy.clone()
                    };
                    assert!(
                        super::super::aws_permissions_boundary_covers(
                            &boundary,
                            &serde_json::to_value(required).unwrap(),
                        )
                        .unwrap(),
                        "Boundary does not cover {id} ({target:?})"
                    );
                }
            }
        }
    }

    #[test]
    fn ownership_ceiling_requires_existing_tags_and_excludes_iam() {
        let context = context();
        let boundary = aws_permissions_boundary(&context, &[]).unwrap();
        let key = format!("aws:ResourceTag/{ALIEN_STACK_TAG_KEY}");
        for (action, condition, covered) in [
            (
                "ec2:ModifyVolume",
                json!({"StringEquals": {&key: context.stack_prefix}}),
                true,
            ),
            (
                "ec2:ModifyVolume",
                json!({"StringEquals": {&key: "another-stack"}}),
                false,
            ),
            (
                "ec2:ModifyVolume",
                json!({"StringEqualsIfExists": {&key: context.stack_prefix}}),
                false,
            ),
            (
                "ec2:ModifyVolume",
                json!({"StringEquals": {"aws:RequestTag/deployment": context.stack_prefix}}),
                false,
            ),
            (
                "iam:PutRolePolicy",
                json!({"StringEquals": {&key: context.stack_prefix}}),
                false,
            ),
        ] {
            let required = json!({"Version": "2012-10-17", "Statement": [{
                "Effect": "Allow", "Action": action, "Resource": "*", "Condition": condition,
            }]});
            assert_eq!(
                super::super::aws_permissions_boundary_covers(&boundary, &required).unwrap(),
                covered,
                "{required}"
            );
        }
    }

    #[test]
    fn arn_selected_catalog_grants_use_exact_resource_ceiling() {
        let key_arn =
            "arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab";
        for id in [
            "key/availability",
            "key/native-storage",
            "key/native-volume",
            "key/remote-cryptography",
        ] {
            for global in [false, true] {
                let profile = alien_core::PermissionProfile(indexmap::indexmap! {
                    (if global { "*" } else { "encryption" }).to_string() => vec![PermissionSetReference::from_name(id)]
                });
                let stack = Stack::new("test".to_string())
                    .permission("application", profile)
                    .add(
                        alien_core::Key::new("encryption".to_string()).build(),
                        alien_core::ResourceLifecycle::Frozen,
                    )
                    .build();
                if global {
                    assert_eq!(
                        aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context()))
                            .unwrap_err()
                            .code,
                        "BINDING_TARGET_NOT_SUPPORTED"
                    );
                    continue;
                }
                let boundary = aws_stack_permissions_boundary(&stack, &context(), |_, scope| {
                    Ok(if scope == "*" {
                        context()
                    } else {
                        context().with_resource_name(key_arn)
                    })
                })
                .unwrap();
                let required = serde_json::to_value(
                    AwsRuntimePermissionsGenerator::new()
                        .generate_policy(
                            get_permission_set(id).unwrap(),
                            BindingTarget::Resource,
                            &context().with_resource_name(key_arn),
                        )
                        .unwrap(),
                )
                .unwrap();
                assert!(
                    super::super::aws_permissions_boundary_covers(&boundary, &required).unwrap(),
                    "{id}, global={global}"
                );
                let sibling = serde_json::from_str(&required.to_string().replace(
                    "1234abcd-12ab-34cd-56ef-1234567890ab",
                    "9876abcd-12ab-34cd-56ef-1234567890ab",
                ))
                .unwrap();
                assert!(
                    !super::super::aws_permissions_boundary_covers(&boundary, &sibling).unwrap()
                );
                if required["Statement"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|statement| statement.get("Condition").is_some())
                {
                    let mut unconditional = required.clone();
                    for statement in unconditional["Statement"].as_array_mut().unwrap() {
                        statement.as_object_mut().unwrap().remove("Condition");
                    }
                    // Only grants for AWS resources may use a stack key outside its conditions.
                    let grants = unconditional.to_string().contains("kms:CreateGrant");
                    assert_eq!(
                        super::super::aws_permissions_boundary_covers(&boundary, &unconditional)
                            .unwrap(),
                        !grants,
                        "{id}"
                    );
                }
                assert_eq!(
                    aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context()))
                        .unwrap_err()
                        .code,
                    "VARIABLE_NOT_FOUND"
                );
            }
        }
    }

    #[test]
    fn inline_key_override_keeps_its_conditions() {
        let key_arn =
            "arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab";
        let set = get_permission_set("key/native-volume").unwrap().clone();
        let stack = Stack::new("test".to_string())
            .permission(
                "application",
                alien_core::PermissionProfile(indexmap::indexmap! {
                    "encryption".to_string() => vec![PermissionSetReference::Inline(set.clone())]
                }),
            )
            .build();
        let context = context().with_resource_name(key_arn);
        let boundary =
            aws_stack_permissions_boundary(&stack, &context, |_, _| Ok(context.clone())).unwrap();
        let mut required = AwsRuntimePermissionsGenerator::new()
            .generate_policy(&set, BindingTarget::Resource, &context)
            .unwrap();
        assert!(super::super::aws_permissions_boundary_covers(
            &boundary,
            &serde_json::to_value(&required).unwrap()
        )
        .unwrap());
        for statement in &mut required.statement {
            statement.condition = None;
        }
        assert!(!super::super::aws_permissions_boundary_covers(
            &boundary,
            &serde_json::to_value(required).unwrap()
        )
        .unwrap());
    }

    #[test]
    fn combined_arn_selected_grants_fit_with_longest_prefix() {
        let stack = Stack::new("test".to_string())
            .permission(
                "application",
                alien_core::PermissionProfile::new().resource(
                    "encryption",
                    [
                        "key/availability",
                        "key/native-storage",
                        "key/native-volume",
                        "key/remote-cryptography",
                    ],
                ),
            )
            .add(
                alien_core::Key::new("encryption".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let render = |context: PermissionContext| {
            aws_stack_permissions_boundary(&stack, &context, |_, _| {
                Ok(context.clone().with_resource_name(
                    "arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab",
                ))
            })
        };
        let boundary = render(context()).unwrap();
        assert!(boundary.to_string().len() <= MANAGED_POLICY_LIMIT);
        for id in [
            "key/availability",
            "key/native-storage",
            "key/native-volume",
            "key/remote-cryptography",
        ] {
            let required = AwsRuntimePermissionsGenerator::new().generate_policy(
                get_permission_set(id).unwrap(), BindingTarget::Resource,
                &context().with_resource_name("arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab"),
            ).unwrap();
            assert!(super::super::aws_permissions_boundary_covers(
                &boundary,
                &serde_json::to_value(required).unwrap()
            )
            .unwrap());
        }
    }

    fn custom_statement(resource: String) -> AwsIamStatement {
        AwsIamStatement {
            sid: "Custom".to_string(),
            effect: "Allow".to_string(),
            action: vec!["s3:GetObject".to_string()],
            resource: vec![resource],
            not_resource: vec![],
            condition: Some(
                serde_json::from_value(
                    json!({"StringEquals": {"s3:ExistingObjectTag/access": "approved"}}),
                )
                .unwrap(),
            ),
        }
    }

    #[test]
    fn custom_permissions_keep_conditions_and_merge_matching_resources() {
        let first = custom_statement("arn:aws:s3:::external-bucket/*".to_string());
        let mut second = first.clone();
        second.action = vec!["s3:GetObjectVersion".to_string()];
        let boundary = aws_permissions_boundary(&context(), &[first.clone(), second]).unwrap();
        let grants: Vec<_> = boundary["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["Resource"] == json!(first.resource[0]))
            .collect();
        assert_eq!(grants.len(), 1);
        assert_eq!(
            grants[0]["Action"],
            json!(["s3:GetObject", "s3:GetObjectVersion"])
        );
        assert_eq!(grants[0]["Condition"], json!(first.condition));
    }

    #[test]
    fn oversized_direct_and_profile_grants_fail_with_actionable_error() {
        let resource = format!("arn:aws:s3:::{}/*", "x".repeat(MANAGED_POLICY_LIMIT));
        let stack = Stack::new("test".to_string())
            .permission("one", inline_profile(&resource))
            .build();
        for result in [
            aws_permissions_boundary(&context(), &[custom_statement(resource)]),
            aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context())),
        ] {
            let error = result.unwrap_err();
            assert_eq!(error.code, "PERMISSIONS_BOUNDARY_TOO_LARGE");
            assert!(!error.retryable);
            assert!(error
                .to_string()
                .contains("Reduce custom runtime permissions"));
        }
    }

    fn inline_profile(resource: &str) -> alien_core::PermissionProfile {
        let set = serde_json::from_value(json!({
            "id": "custom/read",
            "description": "Read approved objects",
            "platforms": {"aws": [{
                "grant": {"actions": ["s3:GetObject"]},
                "binding": {
                    "stack": {"resources": [resource]},
                    "resource": {
                        "resources": ["arn:aws:s3:::${resourceName}/*"],
                        "condition": {"StringEquals": {"s3:ExistingObjectTag/profile": "${resourceId}"}}
                    }
                }
            }]}
        })).unwrap();
        alien_core::PermissionProfile(indexmap::indexmap! {
            "*".to_string() => vec![PermissionSetReference::Inline(set)]
        })
    }

    #[test]
    fn stack_boundary_compiles_inline_grants_with_their_identity_and_resource_contexts() {
        let mut scoped = inline_profile("unused");
        let sets = scoped.0.shift_remove("*").unwrap();
        scoped.0.insert("uploads".to_string(), sets);
        let mut other_cloud = inline_profile("unused");
        let PermissionSetReference::Inline(set) = &mut other_cloud.0["*"][0] else {
            unreachable!()
        };
        set.platforms.aws = None;
        let stack = Stack::new("test".to_string())
            .permission("other-cloud", other_cloud)
            .permission("reader", inline_profile("arn:aws:s3:::external-global/*"))
            .permission("writer", scoped)
            .permission(
                "catalog",
                alien_core::PermissionProfile::new().global(["storage/data-read"]),
            )
            .management(alien_core::ManagementPermissions::Extend(inline_profile(
                "arn:aws:s3:::management-only/*",
            )))
            .build();
        let mut resolved = Vec::new();
        let boundary = aws_stack_permissions_boundary(&stack, &context(), |profile, scope| {
            resolved.push((profile.to_string(), scope.to_string()));
            Ok(context()
                .with_resource_name("actual-existing-bucket")
                .with_resource_id(scope))
        })
        .unwrap();
        assert_eq!(
            resolved,
            [
                ("reader".to_string(), "*".to_string()),
                ("writer".to_string(), "uploads".to_string())
            ]
        );
        let statements = boundary["Statement"].as_array().unwrap();
        assert!(statements
            .iter()
            .any(|s| s["Resource"] == json!("arn:aws:s3:::external-global/*")
                && s["Action"] == json!("s3:GetObject")));
        assert!(statements.iter().any(|s| s["Resource"]
            == json!("arn:aws:s3:::actual-existing-bucket/*")
            && s["Action"] == json!("s3:GetObject")
            && s["Condition"]
                == json!({"StringEquals": {"s3:ExistingObjectTag/profile": "uploads"}})));
        assert!(!boundary.to_string().contains("management-only"));
    }

    #[test]
    fn global_inline_resource_grants_cover_imported_names_and_preserve_conditions() {
        let mut profile = inline_profile("arn:aws:s3:::${stackPrefix}-*/*");
        let PermissionSetReference::Inline(set) = &mut profile.0["*"][0] else {
            unreachable!()
        };
        set.id = "storage/custom-read".to_string();
        let set = set.clone();
        let account = alien_core::ServiceAccount::from_permission_profile(
            "reader-sa".to_string(),
            &profile,
            |_| None,
        )
        .unwrap();
        let stack = Stack::new("test".to_string())
            .permission("reader", profile.clone())
            .add(account, alien_core::ResourceLifecycle::Frozen)
            .add(
                alien_core::Storage::new("uploads".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Storage::new("archive".to_string()).build(),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                alien_core::Queue::new("jobs".to_string()).build(),
                alien_core::ResourceLifecycle::Live,
            )
            .build();
        let mut resolved = Vec::new();
        let boundary = aws_stack_permissions_boundary(&stack, &context(), |name, scope| {
            assert_eq!(name, "reader");
            resolved.push(scope.to_string());
            Ok(context()
                .with_resource_id(scope)
                .with_resource_name(format!("imported-{scope}")))
        })
        .unwrap();
        assert_eq!(resolved, ["*", "uploads", "archive"]);
        let generator = AwsRuntimePermissionsGenerator::new();
        for id in ["uploads", "archive"] {
            let resource_context = context()
                .with_resource_id(id)
                .with_resource_name(format!("imported-{id}"));
            let selected = without_refs_the_stack_policy_grants(
                &stack,
                "reader",
                &profile,
                id,
                vec![PermissionSetReference::Inline(set.clone())],
                &generator,
                &resource_context,
            )
            .unwrap();
            assert_eq!(selected.len(), 1);
            let required = serde_json::to_value(
                generator
                    .generate_policy(&set, BindingTarget::Resource, &resource_context)
                    .unwrap(),
            )
            .unwrap();
            assert!(super::super::aws_permissions_boundary_covers(&boundary, &required).unwrap());
            let mut unconditional = required.clone();
            for statement in unconditional["Statement"].as_array_mut().unwrap() {
                statement.as_object_mut().unwrap().remove("Condition");
            }
            assert!(
                !super::super::aws_permissions_boundary_covers(&boundary, &unconditional).unwrap()
            );
        }
    }

    #[test]
    fn implicit_grants_reuse_runtime_redundancy_selection() {
        let mut profile = inline_profile("arn:aws:s3:::${stackPrefix}-*/*");
        let PermissionSetReference::Inline(set) = &mut profile.0["*"][0] else {
            unreachable!()
        };
        set.id = "storage/custom-read".to_string();
        let set = set.clone();
        let account = alien_core::ServiceAccount::from_permission_profile(
            "reader-sa".to_string(),
            &profile,
            |_| None,
        )
        .unwrap();
        let mut stack = Stack::new("test".to_string())
            .permission("reader", profile)
            .add(account, alien_core::ResourceLifecycle::Frozen)
            .add(
                alien_core::Storage::new("uploads".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let render = |stack: &Stack| {
            aws_stack_permissions_boundary(stack, &context(), |_, scope| {
                Ok(context()
                    .with_resource_id(scope)
                    .with_resource_name(format!("{}-{scope}", context().stack_prefix.unwrap())))
            })
            .unwrap()
        };
        let globals = AwsRuntimePermissionsGenerator::new()
            .generate_policy(&set, BindingTarget::Stack, &context())
            .unwrap();
        let expected = aws_permissions_boundary(&context(), &globals.statement).unwrap();
        assert_eq!(render(&stack), expected);
        // Explicit grants are retained by runtime even when a global grant covers them.
        stack
            .permissions
            .profiles
            .get_mut("reader")
            .unwrap()
            .0
            .insert(
                "uploads".to_string(),
                vec![PermissionSetReference::Inline(set)],
            );
        let explicit = render(&stack);
        assert_ne!(explicit, expected);
        assert!(super::super::aws_permissions_boundary_covers(&explicit, &expected).unwrap());
        // The implicit and explicit copies of the conditional grant merge into one.
        assert_eq!(
            explicit["Statement"].as_array().unwrap().len(),
            expected["Statement"].as_array().unwrap().len() + 1
        );
    }

    #[test]
    fn implicit_resource_grants_count_toward_the_boundary_size_limit() {
        let mut profile = inline_profile("arn:aws:s3:::${stackPrefix}-*/*");
        let PermissionSetReference::Inline(set) = &mut profile.0["*"][0] else {
            unreachable!()
        };
        set.id = "storage/custom-read".to_string();
        let mut stack = Stack::new("test".to_string()).permission("reader", profile);
        for index in 0..24 {
            let id = format!("uploads-{index}");
            stack = stack.add(
                alien_core::Storage::new(id).build(),
                alien_core::ResourceLifecycle::Frozen,
            );
        }
        let stack = stack.build();
        let error = aws_stack_permissions_boundary(&stack, &context(), |_, scope| {
            Ok(context()
                .with_resource_id(scope)
                .with_resource_name(format!("imported-{scope}")))
        })
        .unwrap_err();
        assert_eq!(error.code, "PERMISSIONS_BOUNDARY_TOO_LARGE");
    }

    #[test]
    fn stack_boundary_preserves_inline_overrides_and_deduplicates_shared_grants() {
        let mut profile = inline_profile("arn:aws:s3:::custom-override/*");
        let PermissionSetReference::Inline(set) = &mut profile.0["*"][0] else {
            unreachable!()
        };
        // An inline definition remains custom even when its id matches a catalog set.
        set.id = "storage/data-read".to_string();
        let one = Stack::new("test".to_string())
            .permission("one", profile.clone())
            .build();
        let two = Stack::new("test".to_string())
            .permission("one", profile.clone())
            .permission("two", profile)
            .build();
        let render = |stack: &Stack| {
            aws_stack_permissions_boundary(stack, &context(), |_, _| Ok(context())).unwrap()
        };
        let boundary = render(&one);
        assert_eq!(boundary, render(&two));
        assert!(boundary["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["Resource"] == json!("arn:aws:s3:::custom-override/*")));
    }

    #[test]
    fn stack_boundary_propagates_context_failures_for_each_grant_source() {
        for source in ["global", "explicit-resource", "implicit-resource"] {
            let mut profile = inline_profile(if source == "global" {
                "${missingVariable}"
            } else {
                "arn:aws:s3:::${stackPrefix}-*/*"
            });
            if source == "explicit-resource" {
                let sets = profile.0.shift_remove("*").unwrap();
                profile.0.insert("uploads".to_string(), sets);
            } else if source == "implicit-resource" {
                let PermissionSetReference::Inline(set) = &mut profile.0["*"][0] else {
                    unreachable!()
                };
                set.id = "storage/custom-read".to_string();
            }
            let stack = Stack::new("test".to_string())
                .permission("reader", profile)
                .add(
                    alien_core::Storage::new("uploads".to_string()).build(),
                    alien_core::ResourceLifecycle::Frozen,
                )
                .build();
            for resolver_fails in [true, false] {
                let mut resolved = Vec::new();
                let error = aws_stack_permissions_boundary(&stack, &context(), |_, scope| {
                    resolved.push(scope.to_string());
                    if resolver_fails && (source == "global" || scope == "uploads") {
                        Err(AlienError::new(ErrorData::VariableNotFound {
                            variable: "actualResourceName".to_string(),
                        }))
                    } else {
                        Ok(context())
                    }
                })
                .unwrap_err();
                let missing = if resolver_fails {
                    "actualResourceName"
                } else if source == "global" {
                    "missingVariable"
                } else {
                    "resourceName"
                };
                assert_eq!(error.code, "VARIABLE_NOT_FOUND", "{source}");
                assert!(error.to_string().contains(missing), "{source}: {error}");
                let expected = match source {
                    "global" => vec!["*"],
                    "explicit-resource" => vec!["uploads"],
                    _ => vec!["*", "uploads"],
                };
                assert_eq!(resolved, expected, "{source}");
            }
        }
    }

    #[test]
    fn control_plane_sets_are_excluded() {
        for id in [
            "worker/provision",
            "worker/management",
            "storage/heartbeat",
            "storage/telemetry",
            "guard/example",
            "sandbox/protect-setup-roles",
            "remote-stack-management/execute",
            "observe/logs",
            "experimental/aws-opensearch/provision",
        ] {
            assert!(!is_runtime_set(id), "{id}");
        }
        for id in [
            "worker/execute",
            "storage/data-read",
            "sandbox/remote-execute",
            "experimental/aws-opensearch/data-access",
        ] {
            assert!(is_runtime_set(id), "{id}");
        }
    }

    /// An inline set of one AWS entry, bound stack-wide.
    fn inline_entry(
        id: &str,
        effect: &str,
        actions: &[&str],
        resources: &[String],
        condition: Option<Value>,
    ) -> PermissionSetReference {
        let mut binding = json!({"resources": resources});
        if let Some(condition) = condition {
            binding["condition"] = condition;
        }
        PermissionSetReference::Inline(
            serde_json::from_value(json!({
                "id": id,
                "description": "Test grant",
                "platforms": {"aws": [{
                    "effect": effect,
                    "grant": {"actions": actions},
                    "binding": {"stack": binding}
                }]}
            }))
            .unwrap(),
        )
    }

    /// Least-privilege reads of per-input secret parameters, one profile per consumer. Each
    /// consumer reads a different window of the keys, so no two grants share a resource list.
    fn secret_consumer_profiles(
        keys: usize,
        consumers: usize,
    ) -> Vec<(String, alien_core::PermissionProfile)> {
        (0..consumers)
            .map(|consumer| {
                let resources: Vec<_> = (consumer..keys)
                    .map(|key| {
                        format!(
                            "arn:aws:ssm:${{awsRegion}}:${{awsAccountId}}:parameter/${{stackPrefix}}-secret-{key}"
                        )
                    })
                    .collect();
                (
                    format!("consumer-{consumer}"),
                    alien_core::PermissionProfile(indexmap::indexmap! {
                        "*".to_string() => vec![inline_entry(
                            "custom/secret-inputs-read",
                            "Allow",
                            &["ssm:GetParameter"],
                            &resources,
                            None,
                        )]
                    }),
                )
            })
            .collect()
    }

    /// A Deny on a prefixed parameter and a catalog key grant selecting a provider-assigned ARN.
    fn kept_grants_profile() -> alien_core::PermissionProfile {
        alien_core::PermissionProfile(indexmap::indexmap! {
            "*".to_string() => vec![inline_entry(
                "custom/protect-gateway-config",
                "Deny",
                &["ssm:*"],
                &["arn:aws:ssm:${awsRegion}:${awsAccountId}:parameter/${stackPrefix}-gateway-config".to_string()],
                None,
            )],
            "encryption".to_string() => vec![PermissionSetReference::from_name("key/native-volume")],
        })
    }

    const KEY_ARN: &str =
        "arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab";

    fn key_context(scope: &str) -> Result<PermissionContext> {
        Ok(match scope {
            "encryption" => context().with_resource_name(KEY_ARN),
            _ => context(),
        })
    }

    fn statements_of(boundary: &Value) -> &[Value] {
        boundary["Statement"].as_array().unwrap()
    }

    fn values_of(value: &Value) -> Vec<&str> {
        match value {
            Value::Array(values) => values.iter().map(|v| v.as_str().unwrap()).collect(),
            Value::Null => vec![],
            value => vec![value.as_str().unwrap()],
        }
    }

    /// IAM's case-sensitive ARN wildcard: `*` matches any run of characters, `?` one.
    fn arn_matches(pattern: &str, value: &str) -> bool {
        fn matches(pattern: &[u8], value: &[u8]) -> bool {
            match (pattern.first(), value.first()) {
                (None, None) => true,
                (Some(b'*'), _) => {
                    matches(&pattern[1..], value)
                        || (!value.is_empty() && matches(pattern, &value[1..]))
                }
                (Some(b'?'), Some(_)) => matches(&pattern[1..], &value[1..]),
                (Some(left), Some(right)) => left == right && matches(&pattern[1..], &value[1..]),
                _ => false,
            }
        }
        matches(pattern.as_bytes(), value.as_bytes())
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Decision {
        Allowed,
        ExplicitlyDenied,
        NotAllowed,
    }

    /// Evaluate a boundary document: an explicit Deny wins, then any Allow. Conditions are not
    /// modeled per key; `conditions_hold` decides every conditioned statement at once, and
    /// callers compare documents under both answers.
    fn decide(boundary: &Value, action: &str, resource: &str, conditions_hold: bool) -> Decision {
        let applies = |statement: &Value| {
            let action_listed = |key| {
                values_of(&statement[key])
                    .iter()
                    .any(|pattern| action_pattern_matches(pattern, action))
            };
            let action_matches = match statement.get("NotAction") {
                Some(_) => !action_listed("NotAction"),
                None => action_listed("Action"),
            };
            let resource_matches = match statement.get("NotResource") {
                Some(not_resources) => !values_of(not_resources)
                    .iter()
                    .any(|pattern| arn_matches(pattern, resource)),
                None => values_of(&statement["Resource"])
                    .iter()
                    .any(|pattern| arn_matches(pattern, resource)),
            };
            action_matches
                && resource_matches
                && (statement.get("Condition").is_none() || conditions_hold)
        };
        let matching: Vec<_> = statements_of(boundary)
            .iter()
            .filter(|s| applies(s))
            .collect();
        if matching.iter().any(|s| s["Effect"] == "Deny") {
            Decision::ExplicitlyDenied
        } else if matching.iter().any(|s| s["Effect"] == "Allow") {
            Decision::Allowed
        } else {
            Decision::NotAllowed
        }
    }

    /// A concrete request value matched by `pattern`.
    fn concrete(pattern: &str) -> String {
        pattern.replace('*', "probe").replace('?', "p")
    }

    /// Requests on resources only [`WIDENED_SCOPED_PATTERNS`] admit, all named with the stack
    /// prefix right after a `/`, and requests the widened patterns must still not admit.
    fn widening_probes() -> (Vec<String>, Vec<String>) {
        let prefix = "s".repeat(40);
        let codebuild = "arn:aws:codebuild:ap-southeast-7:123456789012";
        let logs = "arn:aws:logs:ap-southeast-7:123456789012:log-group";
        let widened = vec![
            format!("{codebuild}:build/{prefix}-images:0a1b2c3d"),
            format!("{codebuild}:build-batch/{prefix}-images:0a1b2c3d"),
            format!("{codebuild}:report/{prefix}-tests:0a1b2c3d"),
            format!("{codebuild}:fleet/{prefix}-runners"),
            format!("{logs}:/aws/codebuild/{prefix}-images"),
            format!("{logs}:/aws/ecs/{prefix}-api"),
            format!("{logs}:/aws/rds/cluster/{prefix}-db/postgresql"),
        ];
        let excluded = vec![
            format!("{codebuild}:project/other-{prefix}-images"),
            format!("{codebuild}:project/{prefix}images"),
            format!("{logs}:/aws/ecs/team-{prefix}-api"),
            format!("{logs}:/{prefix}-api"),
            format!("{logs}:/custom/{prefix}-api"),
            format!("arn:aws:logs:ap-southeast-7:999999999999:log-group:/aws/ecs/{prefix}-api"),
            format!("arn:aws:codebuild:us-east-1:123456789012:fleet/{prefix}-runners"),
        ];
        (widened, excluded)
    }

    /// Render `stack` with the earlier renderer and the current one, and require the same
    /// decision for every runtime catalog action and every sharing action, on stack, external
    /// and foreign resources, including each copied grant's own requests. The only exception is
    /// the documented widening: non-IAM actions on [`widening_probes`] resources become
    /// allowed. Returns both documents.
    fn assert_same_decisions(stack: &Stack) -> (Value, Value) {
        let before = stack_boundary(
            stack,
            &context(),
            |_, scope| key_context(scope),
            Rendering::Earlier,
        )
        .unwrap();
        let after =
            aws_stack_permissions_boundary(stack, &context(), |_, scope| key_context(scope))
                .unwrap();
        let denies = |boundary: &Value| -> Vec<Value> {
            statements_of(boundary)
                .iter()
                .filter(|s| s["Effect"] == "Deny")
                .cloned()
                .collect()
        };
        assert_eq!(denies(&after), denies(&before));

        let mut actions: BTreeSet<String> = SHARING_ACTIONS.iter().map(|a| concrete(a)).collect();
        for id in list_permission_set_ids()
            .into_iter()
            .filter(|id| is_runtime_set(id))
        {
            for entry in get_permission_set(id)
                .unwrap()
                .platforms
                .aws
                .iter()
                .flatten()
            {
                actions.extend(entry.grant.actions.iter().flatten().map(|a| concrete(a)));
            }
        }
        let mut resources = BTreeSet::new();
        for statement in statements_of(&before) {
            for key in ["Action", "NotAction"] {
                actions.extend(values_of(&statement[key]).iter().map(|a| concrete(a)));
            }
            for key in ["Resource", "NotResource"] {
                resources.extend(values_of(&statement[key]).iter().map(|r| concrete(r)));
            }
        }
        actions.extend(
            [
                "iam:PassRole",
                "iam:CreateRole",
                "sts:AssumeRole",
                "organizations:LeaveOrganization",
            ]
            .map(String::from),
        );
        // The same names under another deployment's prefix, and in another account.
        let foreign: Vec<_> = resources
            .iter()
            .flat_map(|r| {
                [
                    r.replace(&"s".repeat(40), "other-deployment"),
                    r.replace("123456789012", "999999999999"),
                ]
            })
            .collect();
        resources.extend(foreign);
        let (widened, excluded) = widening_probes();
        resources.extend(widened.iter().cloned());
        resources.extend(excluded);

        let mut decisions = BTreeSet::new();
        let mut newly_allowed = BTreeSet::new();
        for action in &actions {
            for resource in &resources {
                for conditions_hold in [true, false] {
                    let decision = decide(&after, action, resource, conditions_hold);
                    let earlier = decide(&before, action, resource, conditions_hold);
                    if earlier != decision {
                        assert!(
                            earlier == Decision::NotAllowed
                                && decision == Decision::Allowed
                                && widened.contains(resource)
                                && !action_pattern_matches("iam:*", action),
                            "{action} on {resource} (conditions hold: {conditions_hold}): \
                             {earlier:?} before, {decision:?} now"
                        );
                        newly_allowed.insert(resource.clone());
                    }
                    decisions.insert(format!("{decision:?}"));
                }
            }
        }
        assert_eq!(decisions.len(), 3, "probes must reach every decision");
        // Each documented widening is real, so the probes would notice it disappear.
        assert_eq!(newly_allowed, widened.into_iter().collect());
        (before, after)
    }

    /// Grants the scoped Allow admits are skipped, and no decision changes. A skipped grant
    /// that names a sharing action exactly still lifts that Deny on its own resource only.
    #[test]
    fn admitted_grants_are_skipped_without_changing_any_decision() {
        let prefix = "s".repeat(40);
        let mut profile = kept_grants_profile();
        profile.0["*"].push(inline_entry(
            "custom/queue-attributes",
            "Allow",
            &["sqs:SetQueueAttributes", "sqs:SendMessage"],
            &["arn:aws:sqs:${awsRegion}:${awsAccountId}:${stackPrefix}-jobs".to_string()],
            None,
        ));
        let mut stack = Stack::new("test".to_string()).permission("kept", profile);
        for (name, profile) in secret_consumer_profiles(2, 2) {
            stack = stack.permission(name, profile);
        }
        let stack = stack
            .add(
                alien_core::Key::new("encryption".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();

        let (before, after) = assert_same_decisions(&stack);
        let text = after.to_string();
        assert!(text.len() < before.to_string().len());
        for skipped in [
            format!(":parameter/{prefix}-secret-"),
            "sqs:SendMessage".into(),
        ] {
            assert!(
                !text.contains(skipped.as_str()),
                "{skipped} was copied: {text}"
            );
        }
        for kept in ["-gateway-config", KEY_ARN] {
            assert!(text.contains(kept), "{kept} is missing: {text}");
        }
        let parameter =
            format!("arn:aws:ssm:ap-southeast-7:123456789012:parameter/{prefix}-secret-1");
        assert_eq!(
            decide(&after, "ssm:GetParameter", &parameter, false),
            Decision::Allowed
        );
        let gateway =
            format!("arn:aws:ssm:ap-southeast-7:123456789012:parameter/{prefix}-gateway-config");
        assert_eq!(
            decide(&after, "ssm:GetParameter", &gateway, true),
            Decision::ExplicitlyDenied
        );
        let queue = |name: &str| format!("arn:aws:sqs:ap-southeast-7:123456789012:{prefix}-{name}");
        assert_eq!(
            decide(&after, "sqs:SetQueueAttributes", &queue("jobs"), true),
            Decision::Allowed
        );
        assert_eq!(
            decide(&after, "sqs:SetQueueAttributes", &queue("other"), true),
            Decision::ExplicitlyDenied
        );
    }

    /// Grants the scoped Allow can't be proven to admit are copied unchanged: external
    /// resources, conditions, IAM actions, actions that may match IAM, and patterns that are not
    /// a literal prefix of the resource.
    #[test]
    fn unproven_grants_are_kept() {
        let grants = [
            ("s3:GetObject", "arn:aws:s3:::partner-exports/*", None),
            (
                "dynamodb:GetItem",
                "arn:aws:dynamodb:${awsRegion}:${awsAccountId}:table/${stackPrefix}-orders",
                Some(json!({"StringEquals": {"aws:PrincipalTag/team": "orders"}})),
            ),
            (
                "iam:GetRole",
                "arn:aws:iam::${awsAccountId}:role/${stackPrefix}-worker",
                None,
            ),
            ("i*:*", "arn:aws:s3:::${stackPrefix}-uploads", None),
            // A wildcard before the prefix can match names outside it.
            (
                "s3:GetObject",
                "arn:aws:s3:::*${stackPrefix}-archive/*",
                None,
            ),
            // ARN matching is case-sensitive.
            (
                "s3:GetObject",
                "arn:aws:S3:::${stackPrefix}-reports/*",
                None,
            ),
        ];
        let references = grants
            .iter()
            .map(|(action, resource, condition)| {
                inline_entry(
                    "custom/kept",
                    "Allow",
                    &[action],
                    &[resource.to_string()],
                    condition.clone(),
                )
            })
            .collect();
        let stack = Stack::new("test".to_string())
            .permission(
                "kept",
                alien_core::PermissionProfile(indexmap::indexmap! {"*".to_string() => references}),
            )
            .build();

        let (before, after) = assert_same_decisions(&stack);
        // Every grant is copied; only the scoped patterns are merged.
        assert_eq!(statements_of(&after)[1..], statements_of(&before)[1..]);
        let rendered = |resource: &str| {
            resource
                .replace("${awsRegion}", "ap-southeast-7")
                .replace("${awsAccountId}", "123456789012")
                .replace("${stackPrefix}", &"s".repeat(40))
        };
        for (action, resource, condition) in grants {
            assert!(
                statements_of(&after)
                    .iter()
                    .any(|s| s["Action"] == json!(action)
                        && s["Resource"] == json!(rendered(resource))
                        && s.get("Condition") == condition.as_ref()),
                "{action} on {resource} is missing"
            );
        }
    }

    /// Grants on prefixed resources grow with secret inputs and their consumers. Copied, they
    /// exceeded the limit; skipped, the boundary is the stack-independent ceiling plus only the
    /// grants it cannot prove.
    #[test]
    fn prefixed_grants_no_longer_grow_the_boundary_past_the_limit() {
        let mut stack = Stack::new("test".to_string()).permission("kept", kept_grants_profile());
        for (name, profile) in secret_consumer_profiles(12, 4) {
            stack = stack.permission(name, profile);
        }
        let stack = stack
            .add(
                alien_core::Key::new("encryption".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();

        let error = stack_boundary(
            &stack,
            &context(),
            |_, scope| key_context(scope),
            Rendering::Earlier,
        )
        .expect_err("copied grants exceed the managed policy limit");
        assert_eq!(error.code, "PERMISSIONS_BOUNDARY_TOO_LARGE");
        let Some(ErrorData::PermissionsBoundaryTooLarge { size, limit, .. }) = &error.error else {
            panic!("unexpected error {error:?}");
        };
        assert!(*size > *limit + 1_000, "{size}");

        let boundary =
            aws_stack_permissions_boundary(&stack, &context(), |_, scope| key_context(scope))
                .expect("the boundary fits");
        let mut expected = aws_permissions_boundary(&context(), &[]).unwrap();
        let statements = expected["Statement"].as_array_mut().unwrap();
        let scoped = statements[0]["Resource"].as_array_mut().unwrap();
        scoped.push(json!(KEY_ARN));
        scoped.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        statements.insert(
            statements.len() - 2,
            json!({
                "Effect": "Deny",
                "Action": "ssm:*",
                "Resource": format!("arn:aws:ssm:ap-southeast-7:123456789012:parameter/{}-gateway-config", "s".repeat(40)),
            }),
        );
        assert_eq!(boundary, expected);
    }

    /// Installed boundaries are checked against the requirement. Both today's document, with
    /// merged scoped patterns, and one installed earlier, listing them separately, cover it.
    #[test]
    fn merged_and_separate_scoped_patterns_both_cover_the_requirement() {
        let stack = Stack::new("test".to_string())
            .permission("kept", kept_grants_profile())
            .add(
                alien_core::Key::new("encryption".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let current =
            aws_stack_permissions_boundary(&stack, &context(), |_, scope| key_context(scope))
                .unwrap();
        let requirement =
            aws_stack_boundary_requirement(&stack, &context(), |_, scope| key_context(scope))
                .unwrap();
        assert_eq!(
            statements_of(&current)[1..],
            statements_of(&requirement)[1..]
        );
        let scoped = |boundary: &Value| -> BTreeSet<String> {
            values_of(&statements_of(boundary)[0]["Resource"])
                .into_iter()
                .map(String::from)
                .collect()
        };
        let prefix = "s".repeat(40);
        let render = |pattern: &str| {
            pattern
                .replace("{region}", "ap-southeast-7")
                .replace("{account}", "123456789012")
                .replace("{prefix}", &prefix)
        };
        let mut merged = scoped(&requirement);
        for (members, widened) in WIDENED_SCOPED_PATTERNS {
            for member in *members {
                assert!(merged.remove(&render(member)), "{member} is not scoped");
            }
            merged.insert(render(widened));
        }
        assert_eq!(scoped(&current), merged);
        let covers = super::super::aws_permissions_boundary_covers;
        assert!(covers(&current, &requirement).unwrap());
        assert!(covers(&requirement, &requirement).unwrap());
        assert!(!covers(&requirement, &current).unwrap());
    }

    /// Grants outside the stack's scope still count, and still fail with the actionable error.
    #[test]
    fn many_external_grants_still_fail_with_actionable_error() {
        let references = (0..60)
            .map(|index| {
                inline_entry(
                    "custom/partner-read",
                    "Allow",
                    &["s3:GetObject"],
                    &[format!("arn:aws:s3:::partner-exports-{index}/*")],
                    None,
                )
            })
            .collect();
        let stack = Stack::new("test".to_string())
            .permission(
                "application",
                alien_core::PermissionProfile(indexmap::indexmap! {"*".to_string() => references}),
            )
            .build();
        let error = aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context()))
            .expect_err("external grants exceed the managed policy limit");
        assert_eq!(error.code, "PERMISSIONS_BOUNDARY_TOO_LARGE");
        assert!(!error.retryable);
        let Some(ErrorData::PermissionsBoundaryTooLarge {
            size,
            limit,
            largest_contributors,
        }) = &error.error
        else {
            panic!("unexpected error {error:?}");
        };
        assert_eq!(*limit, MANAGED_POLICY_LIMIT);
        // Every character beyond the stack-independent ceiling is attributed to the grant.
        let ceiling = minified_size(&aws_permissions_boundary(&context(), &[]).unwrap());
        let statement = minified_size(&json!({
            "Effect": "Allow", "Action": "s3:GetObject", "Resource": "arn:aws:s3:::partner-exports-10/*",
        }));
        // Sixty statements with their separators; ten of the indexes have one digit.
        let grants = 60 * (statement + 1) - 10;
        assert_eq!(*size, ceiling + grants);
        assert_eq!(
            largest_contributors,
            &format!(
                "profile 'application' stack-wide (custom/partner-read): {grants} characters in \
                 60 statements, e.g. 'arn:aws:s3:::partner-exports-0/*'; \
                 stack-independent statements: {ceiling} characters"
            )
        );
        assert!(error
            .to_string()
            .contains("Reduce custom runtime permissions"));
        // A grant merged into a runtime catalog statement is charged only for what it adds,
        // and one that adds nothing is not named.
        let mut references = stack.permissions.profiles["application"].0["*"].clone();
        references.push(inline_entry(
            "custom/describe",
            "Allow",
            &["ec2:DescribeInstances", "ec2:DescribeImages"],
            &["*".to_string()],
            None,
        ));
        references.push(inline_entry(
            "custom/instances",
            "Allow",
            &["ec2:DescribeInstances"],
            &["*".to_string()],
            None,
        ));
        let stack = Stack::new("test".to_string())
            .permission(
                "application",
                alien_core::PermissionProfile(indexmap::indexmap! {"*".to_string() => references}),
            )
            .build();
        let error = aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context()))
            .expect_err("external grants exceed the managed policy limit");
        let Some(ErrorData::PermissionsBoundaryTooLarge {
            size: merged_size,
            largest_contributors,
            ..
        }) = &error.error
        else {
            panic!("unexpected error {error:?}");
        };
        // Only `ec2:DescribeImages` is new: the quoted action and its separator.
        let added = r#""ec2:DescribeImages","#.len();
        assert_eq!(*merged_size, *size + added);
        assert!(
            largest_contributors.contains(&format!(
                "profile 'application' stack-wide (custom/describe): \
                 {added} characters in 1 runtime catalog statement it adds actions to"
            )),
            "{largest_contributors}"
        );
        assert!(
            !largest_contributors.contains("custom/instances"),
            "{largest_contributors}"
        );
        assert!(
            largest_contributors.ends_with(&format!(
                "stack-independent statements: {ceiling} characters"
            )),
            "{largest_contributors}"
        );
        // The requirement is never installed, so it renders whatever its size.
        let requirement = aws_stack_boundary_requirement(&stack, &context(), |_, _| Ok(context()))
            .expect("the requirement has no size limit");
        assert!(minified_size(&requirement) > MANAGED_POLICY_LIMIT);
    }
}
