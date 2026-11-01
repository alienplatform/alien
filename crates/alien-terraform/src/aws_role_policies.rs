//! Setup grants on AWS roles as consolidated managed policies.
//!
//! IAM caps all inline policies on one role at 10,240 characters, and the emitters write one
//! `aws_iam_role_policy` per grant, so a role with a handful of grants overflows. This pass
//! folds every role's grants into customer-managed policies of at most 5,500 characters at the
//! longest deployment values, attached to the role, as the CloudFormation generator does.
//!
//! Each grant's address stays an `aws_iam_role_policy` with its name and role, its document
//! replaced by an inert stub that depends on the role's new attachments. Deployments installed
//! when the grant was inline therefore update it in place after the managed policies are
//! attached, so the role never loses a permission. Terraform cannot order the delete of a
//! removed resource after an unrelated create, which is why the address is kept rather than
//! removed. New installs get the same stubs, so every deployment has one structure.
//!
//! Anything that depended on a grant now depends on the attachments that carry it.
//!
//! A later release that adds or removes a grant can move statements between a role's managed
//! policies. A policy whose document changes is a new policy under a name derived from its
//! content, created and attached before the old one goes: policies and attachments are
//! create-before-destroy, each attachment waits for all of its bin's policies, and a per-role
//! barrier replaced with every attachment change makes Terraform detach and delete the old ones
//! only after every new attachment exists. During such an update a role holds both, so each
//! attachment counts twice against its quota.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    emitters::aws::helpers::{iam_managed_policy_name_template, jsonencode},
    expr,
    gating::GatedAddresses,
    iam_limits::{DeploymentLengths, RenderedModule},
};
use alien_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use hcl::{
    expr::{Expression, ObjectKey, TraversalOperator},
    structure::{Block, Structure},
};
use indexmap::{IndexMap, IndexSet};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Largest consolidated policy at the longest deployment values. IAM allows 6,144 characters;
/// the rest is headroom, as the CloudFormation generator keeps.
const MAX_MANAGED_POLICY_CHARS: usize = 5_500;

/// The inert document every former inline grant keeps: it denies no action, so it neither
/// grants nor denies anything. It is the smallest document IAM accepts that policy validators
/// (IAM Access Analyzer) also pass without findings, which is why it names its version.
pub(crate) const STUB_POLICY: &str =
    r#"{"Version":"2012-10-17","Statement":[{"Effect":"Deny","NotAction":"*","Resource":"*"}]}"#;

/// One `aws_iam_role_policy` before consolidation.
struct Grant {
    fragment: String,
    index: usize,
    address: String,
    statements: Vec<Expression>,
    depends_on: Vec<Expression>,
}

/// Grants of one role under one gate, which may fold into the same managed policies.
struct Group {
    role: String,
    count: Option<Expression>,
    grants: Vec<usize>,
}

/// Replace every role's inline grants with managed policies and stubs.
pub(crate) fn consolidate_role_policies(
    per_resource: &mut IndexMap<String, TfFragment>,
    locals: &IndexMap<String, Expression>,
    gated: &mut GatedAddresses,
) -> Result<()> {
    let role_fragments: IndexMap<String, String> = per_resource
        .iter()
        .flat_map(|(id, fragment)| {
            fragment
                .resource_blocks
                .iter()
                .filter(|block| block_type(block) == Some("aws_iam_role"))
                .filter_map(move |block| {
                    Some((block.labels.get(1)?.as_str().to_string(), id.clone()))
                })
        })
        .collect();

    let mut grants = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    for (fragment_id, fragment) in per_resource.iter() {
        for (index, block) in fragment.resource_blocks.iter().enumerate() {
            if block_type(block) != Some("aws_iam_role_policy") {
                continue;
            }
            let address = address(block)?;
            if attribute(block, "for_each").is_some() {
                return Err(invalid(&address, "uses for_each"));
            }
            let role = role_label(block)
                .ok_or_else(|| invalid(&address, "names no role of this module"))?;
            if !role_fragments.contains_key(role) {
                return Err(invalid(&address, "names no role of this module"));
            }
            let count = attribute(block, "count").cloned();
            let statements = statements(block, &address)?;
            let depends_on = match attribute(block, "depends_on") {
                Some(Expression::Array(dependencies)) => dependencies.clone(),
                Some(_) => return Err(invalid(&address, "has a depends_on that is not a list")),
                None => Vec::new(),
            };
            let grant = grants.len();
            grants.push(Grant {
                fragment: fragment_id.clone(),
                index,
                address,
                statements,
                depends_on,
            });
            match groups
                .iter_mut()
                .find(|group| group.role == role && group.count == count)
            {
                Some(group) => group.grants.push(grant),
                None => groups.push(Group {
                    role: role.to_string(),
                    count,
                    grants: vec![grant],
                }),
            }
        }
    }
    if grants.is_empty() {
        return Ok(());
    }

    let module = RenderedModule::new(
        per_resource
            .values()
            .flat_map(|fragment| fragment.resource_blocks.iter().cloned())
            .collect(),
        locals.clone(),
    );
    let lengths = DeploymentLengths::longest();
    let graph = DependencyGraph::new(per_resource);
    let bins = acyclic_bins(&graph, &grants, &groups);

    // Attachments that now carry each former grant, and every new attachment of each role.
    let mut carriers: IndexMap<String, Vec<Expression>> = IndexMap::new();
    let mut role_attachments: IndexMap<String, Vec<Expression>> = IndexMap::new();
    let mut new_blocks: IndexMap<String, Vec<Block>> = IndexMap::new();
    let mut policies_per_label: IndexMap<String, usize> = IndexMap::new();
    let mut group_attachments: IndexMap<usize, Vec<Expression>> = IndexMap::new();
    for bin in &bins {
        let group = &groups[bin.group];
        let members: HashSet<&str> = bin
            .grants
            .iter()
            .map(|grant| grants[*grant].address.as_str())
            .collect();
        let mut statements = Vec::new();
        let mut depends_on: Vec<Expression> = Vec::new();
        for grant in &bin.grants {
            let grant = &grants[*grant];
            statements.extend(grant.statements.iter().cloned());
            for dependency in &grant.depends_on {
                let folded = traversal_address(dependency)
                    .is_some_and(|address| members.contains(address.as_str()));
                if !folded && !depends_on.contains(dependency) {
                    depends_on.push(dependency.clone());
                }
            }
        }
        let documents = chunk(compact(statements), &module, lengths)?;
        let base = group_label(group);
        let labels: Vec<String> = documents
            .iter()
            .map(|_| {
                let index = policies_per_label.entry(base.clone()).or_default();
                *index += 1;
                format!("{base}_{}", *index - 1)
            })
            .collect();
        let policy_addresses: Vec<Expression> = labels
            .iter()
            .map(|label| expr::traversal(["aws_iam_policy", label]))
            .collect();
        let blocks = new_blocks
            .entry(role_fragments[&group.role].clone())
            .or_default();
        let mut attachments = Vec::new();
        for (label, statements) in labels.iter().zip(documents) {
            let document = document(statements);
            // A changed document is a new policy, created before the old one is destroyed, so
            // statements that move between policies as grants are added or removed are always
            // held by one of them. Two policies cannot share a name, so the name follows the
            // content.
            let name = format!("{}-{}", base.replace('_', "-"), content_hash(&document)?);
            let mut policy = vec![
                attr("name", iam_managed_policy_name_template(&name)),
                attr("policy", document),
            ];
            let attachment_label = format!("{label}_attachment");
            let mut attachment = vec![
                attr(
                    "role",
                    expr::traversal(["aws_iam_role", &group.role, "name"]),
                ),
                attr(
                    "policy_arn",
                    expr::traversal(["aws_iam_policy", label, "arn"]),
                ),
                // Attach the new policies only once all of them exist.
                attr("depends_on", Expression::Array(policy_addresses.clone())),
            ];
            if let Some(count) = &group.count {
                policy.insert(0, attr("count", count.clone()));
                attachment.insert(0, attr("count", count.clone()));
                gated.insert("aws_iam_policy", label);
                gated.insert("aws_iam_role_policy_attachment", &attachment_label);
            }
            if !depends_on.is_empty() {
                policy.push(attr("depends_on", Expression::Array(depends_on.clone())));
            }
            policy.push(create_before_destroy());
            attachment.push(create_before_destroy());
            blocks.push(resource_block("aws_iam_policy", label, policy));
            blocks.push(resource_block(
                "aws_iam_role_policy_attachment",
                &attachment_label,
                attachment,
            ));
            attachments.push(expr::traversal([
                "aws_iam_role_policy_attachment",
                &attachment_label,
            ]));
        }
        for grant in &bin.grants {
            carriers.insert(grants[*grant].address.clone(), attachments.clone());
        }
        group_attachments
            .entry(bin.group)
            .or_default()
            .extend(attachments.iter().cloned());
        role_attachments
            .entry(group.role.clone())
            .or_default()
            .extend(attachments);
    }

    // Terraform destroys a replaced object only after everything depending on it is replaced.
    // The barrier depends on every attachment of its role and is replaced whenever one is, so
    // the replaced attachments, and then the replaced policies, go only after every new
    // attachment is in place.
    for (group_index, attachments) in &group_attachments {
        let group = &groups[*group_index];
        let label = format!("{}_policies_barrier", group_label(group));
        let mut barrier = vec![
            attr(
                "triggers_replace",
                Expression::Array(
                    attachments
                        .iter()
                        .map(|attachment| {
                            let address = traversal_address(attachment)
                                .expect("attachments are whole-resource references");
                            let (kind, label) =
                                address.split_once('.').expect("addresses have a type");
                            expr::traversal([kind, label, "id"])
                        })
                        .collect(),
                ),
            ),
            create_before_destroy(),
        ];
        if let Some(count) = &group.count {
            barrier.insert(0, attr("count", count.clone()));
            gated.insert("terraform_data", &label);
        }
        new_blocks
            .entry(role_fragments[&group.role].clone())
            .or_default()
            .push(resource_block("terraform_data", &label, barrier));
    }

    for (fragment_id, blocks) in new_blocks {
        per_resource[&fragment_id].resource_blocks.extend(blocks);
    }
    for fragment in per_resource.values_mut() {
        for block in fragment
            .resource_blocks
            .iter_mut()
            .chain(fragment.data_blocks.iter_mut())
        {
            rewire_dependencies(block, &carriers)?;
        }
    }
    for grant in &grants {
        let block = &mut per_resource[&grant.fragment].resource_blocks[grant.index];
        let role = role_label(block)
            .expect("a grant's role was resolved above")
            .to_string();
        stub(
            block,
            role_attachments.get(&role).cloned().unwrap_or_default(),
        );
    }
    Ok(())
}

/// Grants of one group that fold into the same managed policies.
struct Bin {
    group: usize,
    grants: Vec<usize>,
}

/// Splits each group's grants into bins that each become one set of managed policies without a
/// dependency cycle.
///
/// The grants come from different stack resources, each ordered after what it depends on.
/// Folding two of them makes the managed policy wait for everything either waited for, and
/// makes everything that waited for either wait for it. That closes a cycle when one grant
/// reaches the other through a third resource. A grant joins the first bin of its group that
/// keeps the module acyclic, so a group whose grants never reach each other is a single bin.
fn acyclic_bins(graph: &DependencyGraph, grants: &[Grant], groups: &[Group]) -> Vec<Bin> {
    let mut bins: Vec<Bin> = Vec::new();
    let mut bin_of: IndexMap<String, usize> = IndexMap::new();
    for (group_index, group) in groups.iter().enumerate() {
        for grant in &group.grants {
            let address = &grants[*grant].address;
            let fitting = (0..bins.len())
                .filter(|bin| bins[*bin].group == group_index)
                .find(|bin| {
                    bin_of.insert(address.clone(), *bin);
                    let acyclic = !graph.contracted(&bin_of).has_cycle();
                    bin_of.shift_remove(address);
                    acyclic
                });
            let bin = fitting.unwrap_or_else(|| {
                bins.push(Bin {
                    group: group_index,
                    grants: Vec::new(),
                });
                bins.len() - 1
            });
            bin_of.insert(address.clone(), bin);
            bins[bin].grants.push(*grant);
        }
    }
    bins
}

/// Which module addresses each address refers to, by reference or `depends_on`.
struct DependencyGraph {
    edges: IndexMap<String, IndexSet<String>>,
}

impl DependencyGraph {
    fn new(per_resource: &IndexMap<String, TfFragment>) -> Self {
        let rendered: Vec<(String, String)> = per_resource
            .values()
            .flat_map(|fragment| fragment.resource_blocks.iter().chain(&fragment.data_blocks))
            .filter_map(|block| {
                let address = block_address_text(block)?;
                Some((address, hcl::format::to_string(&block.body).ok()?))
            })
            .collect();
        let edges = rendered
            .iter()
            .map(|(address, body)| {
                let targets = rendered
                    .iter()
                    .map(|(other, _)| other)
                    .filter(|other| *other != address && references_address(body, other))
                    .cloned()
                    .collect();
                (address.clone(), targets)
            })
            .collect();
        Self { edges }
    }

    /// The graph with each bin's grants replaced by one node standing for its policies.
    fn contracted(&self, bin_of: &IndexMap<String, usize>) -> Self {
        let node = |address: &String| {
            bin_of
                .get(address)
                .map_or_else(|| address.clone(), |bin| format!("#bin{bin}"))
        };
        let mut edges: IndexMap<String, IndexSet<String>> = IndexMap::new();
        for (address, targets) in &self.edges {
            let from = node(address);
            let entry = edges.entry(from.clone()).or_default();
            entry.extend(targets.iter().map(node).filter(|target| *target != from));
        }
        Self { edges }
    }

    fn has_cycle(&self) -> bool {
        // 0: unvisited, 1: on the current path, 2: done.
        let mut state: IndexMap<&str, u8> = IndexMap::new();
        for start in self.edges.keys() {
            if state.get(start.as_str()).copied().unwrap_or(0) != 0 {
                continue;
            }
            let mut stack: Vec<(&str, usize)> = vec![(start.as_str(), 0)];
            state.insert(start.as_str(), 1);
            while let Some((node, next)) = stack.pop() {
                let targets = self.edges.get(node);
                let Some(target) = targets.and_then(|targets| targets.get_index(next)) else {
                    state.insert(node, 2);
                    continue;
                };
                stack.push((node, next + 1));
                match state.get(target.as_str()).copied().unwrap_or(0) {
                    1 => return true,
                    0 => {
                        state.insert(target.as_str(), 1);
                        stack.push((target.as_str(), 0));
                    }
                    _ => {}
                }
            }
        }
        false
    }
}

/// Merges statements that differ only in their resources, then only in their actions, without
/// granting any action on a resource it was not granted on. Sids stay unique.
fn compact(statements: Vec<Expression>) -> Vec<Expression> {
    let statements = merge_values(statements, "Resource");
    let mut statements = merge_values(statements, "Action");
    let mut seen = HashSet::new();
    for statement in &mut statements {
        let Expression::Object(object) = statement else {
            continue;
        };
        // Merging moves the merged key last; keep the order IAM documents are read in.
        let mut entries: Vec<_> = std::mem::take(object).into_iter().collect();
        entries.sort_by_key(|(key, _)| statement_key_order(key));
        object.extend(entries);
        let key = ObjectKey::Identifier(hcl::Identifier::sanitized("Sid"));
        let Some(Expression::String(sid)) = object.get(&key).cloned() else {
            continue;
        };
        let unique = (1..)
            .map(|n| {
                if n == 1 {
                    sid.clone()
                } else {
                    format!("{sid}{n}")
                }
            })
            .find(|candidate| !seen.contains(candidate))
            .expect("an unused Sid exists");
        seen.insert(unique.clone());
        object.insert(key, Expression::String(unique));
    }
    statements
}

fn statement_key_order(key: &ObjectKey) -> u8 {
    match object_key(key) {
        Some("Sid") => 0,
        Some("Effect") => 1,
        Some("Principal" | "NotPrincipal") => 2,
        Some("Action" | "NotAction") => 3,
        Some("Resource" | "NotResource") => 4,
        Some("Condition") => 5,
        _ => u8::MAX,
    }
}

/// Statements equal but for one property: what they share, the first Sid, and the union of
/// their values of the property.
type Merged = (
    hcl::Object<ObjectKey, Expression>,
    Option<Expression>,
    Vec<Expression>,
);

fn merge_values(statements: Vec<Expression>, property: &str) -> Vec<Expression> {
    let property_key = ObjectKey::Identifier(hcl::Identifier::sanitized(property));
    let sid_key = ObjectKey::Identifier(hcl::Identifier::sanitized("Sid"));
    let mut merged: Vec<Merged> = Vec::new();
    let mut unchanged = Vec::new();
    for statement in statements {
        let Expression::Object(mut object) = statement else {
            unchanged.push(statement);
            continue;
        };
        let Some(value) = object.remove(&property_key) else {
            unchanged.push(Expression::Object(object));
            continue;
        };
        let sid = object.remove(&sid_key);
        let values = match value {
            Expression::Array(values) => values,
            value => vec![value],
        };
        match merged.iter_mut().find(|(rest, _, _)| *rest == object) {
            Some((_, _, existing)) => {
                for value in values {
                    if !existing.contains(&value) {
                        existing.push(value);
                    }
                }
            }
            None => merged.push((object, sid, values)),
        }
    }
    unchanged.extend(merged.into_iter().map(|(rest, sid, values)| {
        let mut statement = hcl::Object::new();
        if let Some(sid) = sid {
            statement.insert(sid_key.clone(), sid);
        }
        statement.extend(rest);
        statement.insert(property_key.clone(), Expression::Array(values));
        Expression::Object(statement)
    }));
    unchanged
}

/// Splits statements, in order, into documents of at most [`MAX_MANAGED_POLICY_CHARS`] at the
/// longest deployment values.
fn chunk(
    statements: Vec<Expression>,
    module: &RenderedModule,
    lengths: DeploymentLengths,
) -> Result<Vec<Vec<Expression>>> {
    let empty = module.policy_chars(&document(Vec::new()), lengths)?;
    let mut documents: Vec<Vec<Expression>> = Vec::new();
    let mut current: Vec<Expression> = Vec::new();
    let mut size = empty;
    for statement in statements {
        let statement_size = module.json_chars(&statement, lengths)?;
        if empty + statement_size > MAX_MANAGED_POLICY_CHARS {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_terraform_module".into(),
                reason: format!(
                    "An IAM statement can need {statement_size} characters, more than a managed policy of {MAX_MANAGED_POLICY_CHARS} holds: {statement}"
                ),
            }));
        }
        let separator = usize::from(!current.is_empty());
        if size + separator + statement_size > MAX_MANAGED_POLICY_CHARS {
            documents.push(std::mem::take(&mut current));
            size = empty;
        }
        size += usize::from(!current.is_empty()) + statement_size;
        current.push(statement);
    }
    if !current.is_empty() {
        documents.push(current);
    }
    Ok(documents)
}

fn create_before_destroy() -> Structure {
    nested(block(
        "lifecycle",
        [attr("create_before_destroy", Expression::Bool(true))],
    ))
}

/// Eight hex characters of the SHA-256 of a policy document as rendered.
fn content_hash(document: &Expression) -> Result<String> {
    let rendered = hcl::format::to_string(document)
        .into_alien_error()
        .context(ErrorData::TemplateSerializationFailed {
            format: "Terraform IAM policy".into(),
            reason: "Failed to render a managed policy document".into(),
        })?;
    let digest = Sha256::digest(rendered.as_bytes());
    Ok(digest[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn document(statements: Vec<Expression>) -> Expression {
    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".into())),
        ("Statement", Expression::Array(statements)),
    ]))
}

/// The label stem of a group's managed policies: the role's, and the gate's inputs.
fn group_label(group: &Group) -> String {
    let mut label = group.role.clone();
    if let Some(count) = &group.count {
        let inputs = count
            .to_string()
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .filter_map(|token| token.strip_prefix("var."))
            .map(str::to_string)
            .collect::<IndexSet<_>>();
        for input in inputs {
            label.push('_');
            label.push_str(&input);
        }
    }
    label.push_str("_setup");
    label
}

/// Points `depends_on` entries naming a former grant at the attachments now carrying it.
/// Any other reference to a former grant would read its stub, so it fails the render.
fn rewire_dependencies(
    block: &mut Block,
    carriers: &IndexMap<String, Vec<Expression>>,
) -> Result<()> {
    let own = block_address_text(block);
    for structure in block.body.0.iter_mut() {
        let Structure::Attribute(attribute) = structure else {
            continue;
        };
        if attribute.key.as_str() == "depends_on" {
            let Expression::Array(dependencies) = &mut attribute.expr else {
                continue;
            };
            let mut rewired: Vec<Expression> = Vec::new();
            for dependency in dependencies.drain(..) {
                let replacements = match traversal_address(&dependency)
                    .and_then(|address| carriers.get(&address))
                {
                    Some(attachments) => attachments.clone(),
                    None => vec![dependency],
                };
                for replacement in replacements {
                    if !rewired.contains(&replacement) {
                        rewired.push(replacement);
                    }
                }
            }
            *dependencies = rewired;
            continue;
        }
        let rendered = attribute.expr.to_string();
        if let Some(address) = carriers
            .keys()
            .find(|address| references_address(&rendered, address))
        {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!(
                    "{} reads {address} in `{}`, which becomes an inert stub; depend on its grant's attachments instead",
                    own.as_deref().unwrap_or("a block"),
                    attribute.key
                ),
            }));
        }
    }
    Ok(())
}

/// Turns a former grant into the inert stub, ordered after the role's attachments.
fn stub(block: &mut Block, attachments: Vec<Expression>) {
    let mut body: Vec<Structure> = std::mem::take(&mut block.body)
        .into_iter()
        .filter(|structure| {
            !matches!(structure, Structure::Attribute(attribute)
                if matches!(attribute.key.as_str(), "policy" | "depends_on"))
        })
        .collect();
    body.push(attr("policy", Expression::String(STUB_POLICY.to_string())));
    if !attachments.is_empty() {
        body.push(attr("depends_on", Expression::Array(attachments)));
    }
    block.body = hcl::Body::from(body);
}

/// The statements of a grant. A stub contributes none.
fn statements(block: &Block, address: &str) -> Result<Vec<Expression>> {
    let policy = attribute(block, "policy").ok_or_else(|| invalid(address, "has no policy"))?;
    if *policy == Expression::String(STUB_POLICY.to_string()) {
        return Ok(Vec::new());
    }
    let Expression::FuncCall(call) = policy else {
        return Err(invalid(address, "has a policy that is not jsonencode(...)"));
    };
    let (Some(Expression::Object(document)), true) = (
        call.args.first(),
        call.name.to_string() == "jsonencode" && call.args.len() == 1,
    ) else {
        return Err(invalid(
            address,
            "has a policy that is not jsonencode({...})",
        ));
    };
    let mut statements = None;
    for (key, value) in document {
        match (object_key(key), value) {
            (Some("Version"), Expression::String(version)) if version == "2012-10-17" => {}
            (Some("Statement"), Expression::Array(values)) => statements = Some(values.clone()),
            _ => {
                return Err(invalid(
                    address,
                    "has a policy document this pass cannot merge",
                ))
            }
        }
    }
    statements.ok_or_else(|| invalid(address, "has no Statement list"))
}

fn object_key(key: &ObjectKey) -> Option<&str> {
    match key {
        ObjectKey::Identifier(key) => Some(key.as_str()),
        ObjectKey::Expression(Expression::String(key)) => Some(key.as_str()),
        _ => None,
    }
}

fn role_label(block: &Block) -> Option<&str> {
    let Expression::Traversal(traversal) = attribute(block, "role")? else {
        return None;
    };
    let Expression::Variable(root) = &traversal.expr else {
        return None;
    };
    if root.as_str() != "aws_iam_role" {
        return None;
    }
    match traversal.operators.as_slice() {
        [TraversalOperator::GetAttr(label), TraversalOperator::GetAttr(attribute)]
            if matches!(attribute.as_str(), "id" | "name") =>
        {
            Some(label.as_str())
        }
        _ => None,
    }
}

/// `type.label` of a whole-resource reference such as a `depends_on` entry.
fn traversal_address(expression: &Expression) -> Option<String> {
    let Expression::Traversal(traversal) = expression else {
        return None;
    };
    let Expression::Variable(root) = &traversal.expr else {
        return None;
    };
    match traversal.operators.as_slice() {
        [TraversalOperator::GetAttr(label)] => {
            Some(format!("{}.{}", root.as_str(), label.as_str()))
        }
        _ => None,
    }
}

fn block_type(block: &Block) -> Option<&str> {
    (block.identifier.as_str() == "resource")
        .then(|| block.labels.first().map(|label| label.as_str()))
        .flatten()
}

fn block_address_text(block: &Block) -> Option<String> {
    let kind = block.labels.first()?.as_str();
    let label = block.labels.get(1)?.as_str();
    match block.identifier.as_str() {
        "resource" => Some(format!("{kind}.{label}")),
        "data" => Some(format!("data.{kind}.{label}")),
        _ => None,
    }
}

fn address(block: &Block) -> Result<String> {
    block_address_text(block).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: "an aws_iam_role_policy block has no label".into(),
        })
    })
}

fn attribute<'a>(block: &'a Block, name: &str) -> Option<&'a Expression> {
    block.body.0.iter().find_map(|structure| match structure {
        Structure::Attribute(attribute) if attribute.key.as_str() == name => Some(&attribute.expr),
        _ => None,
    })
}

/// Whether rendered HCL mentions `address` as a whole reference and not as a prefix of a
/// longer label.
fn references_address(body: &str, address: &str) -> bool {
    body.match_indices(address).any(|(index, _)| {
        body[index + address.len()..]
            .chars()
            .next()
            .is_none_or(|next| !(next.is_alphanumeric() || next == '_'))
    })
}

fn invalid(address: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::GenericError {
        message: format!("Cannot move {address} into a managed policy: it {reason}"),
    })
}

#[cfg(test)]
#[path = "aws_role_policies_tests.rs"]
mod tests;
