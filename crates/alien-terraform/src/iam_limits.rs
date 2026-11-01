//! IAM role quotas checked against the largest policies the emitted module can submit.
//!
//! AWS caps a role at 10,240 non-whitespace characters across all of its inline policies,
//! a managed policy at 6,144, and a role at 20 managed-policy attachments by default. A module
//! applies with deployment values chosen later (resource prefix, region, account), so every
//! policy is sized here at the longest value each interpolation can resolve to. A module that
//! passes therefore fits every deployment. An interpolation this check cannot bound fails the
//! render instead of being guessed, and resources behind `count`/`for_each` count as present.

use crate::ModuleFiles;
use alien_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::generators::aws_longest_permission_context;
use hcl::{
    expr::{
        BinaryOperator, Conditional, Expression, FuncCall, ObjectKey, Operation, TemplateExpr,
        Traversal, TraversalOperator,
    },
    structure::{Block, Structure},
    template::{Element, Template},
};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

/// AWS's limit on the non-whitespace characters of all inline policies on one role.
pub(crate) const IAM_ROLE_INLINE_POLICY_CHARS: usize = 10_240;
/// AWS's limit on the non-whitespace characters of one managed policy.
pub(crate) const IAM_MANAGED_POLICY_CHARS: usize = 6_144;
/// AWS's default "Managed policies per role" quota (L-0DA4ABF3). An account can raise it,
/// but a generated module must fit an account that never asked.
pub(crate) const IAM_MANAGED_POLICIES_PER_ROLE: usize = 20;

/// Longest value of each deployment variable an IAM policy can interpolate.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeploymentLengths {
    pub prefix: usize,
    pub region: usize,
    pub account_id: usize,
    pub managing_account_id: usize,
    pub managing_role_arn: usize,
}

impl DeploymentLengths {
    /// The longest values setup accepts, shared with the permissions-boundary size check.
    pub fn longest() -> Self {
        let context = aws_longest_permission_context();
        let len = |value: Option<String>| value.map_or(0, |value| value.chars().count());
        Self {
            prefix: len(context.stack_prefix),
            region: len(context.aws_region),
            account_id: len(context.aws_account_id),
            managing_account_id: len(context.managing_account_id),
            managing_role_arn: len(context.managing_role_arn),
        }
    }
}

/// Worst-case IAM usage of one role.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RoleUsage {
    /// Inline policies on the role, embedded or as `aws_iam_role_policy`.
    pub inline_policies: usize,
    /// Largest total non-whitespace characters of those inline policies.
    pub inline_chars: usize,
    /// Managed policies attached to the role at once, at most: distinct policies, and the
    /// replacement of each one replaced create-before-destroy.
    pub attachments: usize,
}

/// Worst-case IAM usage of a module.
#[derive(Debug, Default)]
pub(crate) struct ModuleUsage {
    /// Keyed by the `aws_iam_role` label.
    pub roles: IndexMap<String, RoleUsage>,
    /// Largest non-whitespace characters of each `aws_iam_policy`, keyed by label.
    pub managed_policies: IndexMap<String, usize>,
}

/// Rejects a module whose IAM policies can exceed a role or managed-policy quota for some
/// deployment it accepts.
pub(crate) fn validate_role_policy_limits(module: &ModuleFiles) -> Result<()> {
    RenderedModule::parse(module)?.check()
}

/// The resources and locals of a rendered module, as Terraform reads them.
pub(crate) struct RenderedModule {
    resources: Vec<Block>,
    locals: IndexMap<String, Expression>,
}

impl RenderedModule {
    pub fn new(resources: Vec<Block>, locals: IndexMap<String, Expression>) -> Self {
        Self { resources, locals }
    }

    pub fn parse(module: &ModuleFiles) -> Result<Self> {
        let mut resources = Vec::new();
        let mut locals = IndexMap::new();
        for (path, contents) in module.iter().filter(|(path, _)| path.ends_with(".tf")) {
            let body = hcl::parse(contents).into_alien_error().context(
                ErrorData::TemplateSerializationFailed {
                    format: "Terraform IAM policy".into(),
                    reason: format!("Failed to parse '{path}' to size its IAM policies"),
                },
            )?;
            for block in body.into_blocks() {
                match block.identifier.as_str() {
                    "resource" => resources.push(block),
                    "locals" => {
                        for attribute in block.body.into_attributes() {
                            locals.insert(attribute.key.to_string(), attribute.expr);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(Self::new(resources, locals))
    }

    /// Fails when a quota can be exceeded with the longest deployment values.
    pub fn check(&self) -> Result<()> {
        let lengths = DeploymentLengths::longest();
        let usage = self.usage(lengths)?;
        let mut shortfalls = Vec::new();
        for (role, role_usage) in &usage.roles {
            if role_usage.inline_chars > IAM_ROLE_INLINE_POLICY_CHARS {
                shortfalls.push(format!(
                    "IAM role '{role}' can need {} non-whitespace characters across its {} inline policies with a {}-character resource prefix and a {}-character region; AWS allows {IAM_ROLE_INLINE_POLICY_CHARS} across all inline policies on one role. Reduce permissions or move grants to managed policies.",
                    role_usage.inline_chars, role_usage.inline_policies, lengths.prefix, lengths.region
                ));
            }
            if role_usage.attachments > IAM_MANAGED_POLICIES_PER_ROLE {
                shortfalls.push(format!(
                    "IAM role '{role}' requires {} managed-policy attachments, counting twice each one replaced create-before-destroy, since an update attaches its replacement before detaching it; generated setup supports the default quota of {IAM_MANAGED_POLICIES_PER_ROLE} per role. Reduce the number of attached policies.",
                    role_usage.attachments
                ));
            }
        }
        for (policy, chars) in &usage.managed_policies {
            if *chars > IAM_MANAGED_POLICY_CHARS {
                shortfalls.push(format!(
                    "IAM managed policy '{policy}' can need {chars} non-whitespace characters with a {}-character resource prefix and a {}-character region; AWS allows {IAM_MANAGED_POLICY_CHARS} per managed policy.",
                    lengths.prefix, lengths.region
                ));
            }
        }
        if shortfalls.is_empty() {
            Ok(())
        } else {
            Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_terraform_module".into(),
                reason: shortfalls.join("\n"),
            }))
        }
    }

    /// Largest non-whitespace characters IAM counts for a `policy` attribute at `lengths`.
    pub fn policy_chars(&self, policy: &Expression, lengths: DeploymentLengths) -> Result<usize> {
        Module::new(&self.resources, &self.locals, lengths).policy_chars(policy)
    }

    /// Largest non-whitespace characters of a value inside a policy document at `lengths`.
    pub fn json_chars(&self, value: &Expression, lengths: DeploymentLengths) -> Result<usize> {
        Module::new(&self.resources, &self.locals, lengths).json_chars(value, 0)
    }

    /// Sizes every IAM role and managed policy of the module at `lengths`.
    pub fn usage(&self, lengths: DeploymentLengths) -> Result<ModuleUsage> {
        let module = Module::new(&self.resources, &self.locals, lengths);
        let mut usage = ModuleUsage::default();
        let mut attachments: IndexMap<String, HashSet<String>> = IndexMap::new();
        for block in &self.resources {
            if resource_type(block) == Some("aws_iam_role") {
                usage
                    .roles
                    .entry(resource_label(block)?.to_string())
                    .or_default();
            }
        }
        for block in &self.resources {
            let Some(kind) = resource_type(block) else {
                continue;
            };
            let label = resource_label(block)?;
            match kind {
                "aws_iam_role" => {
                    for nested in block
                        .body
                        .blocks()
                        .filter(|nested| nested.identifier.as_str() == "inline_policy")
                    {
                        let policy = required_attribute(nested, "policy", kind, label)?;
                        let chars = module
                            .policy_chars(policy)
                            .context(policy_context(kind, label))?;
                        let role = &mut usage.roles[label];
                        role.inline_policies += 1;
                        role.inline_chars += chars;
                    }
                    if let Some(arns) = attribute(block, "managed_policy_arns") {
                        let Expression::Array(arns) = arns else {
                            return Err(unbounded(arns, "managed_policy_arns must be a list"));
                        };
                        let entry = attachments.entry(label.to_string()).or_default();
                        entry.extend(arns.iter().map(policy_identity));
                    }
                }
                "aws_iam_role_policy" => {
                    let role = module.role_of(block, kind, label)?;
                    let policy = required_attribute(block, "policy", kind, label)?;
                    let chars = module
                        .policy_chars(policy)
                        .context(policy_context(kind, label))?;
                    let role = usage
                        .roles
                        .get_mut(role)
                        .ok_or_else(|| unknown_role(kind, label, role))?;
                    role.inline_policies += 1;
                    role.inline_chars += chars;
                }
                "aws_iam_role_policy_attachment" => {
                    let role = module.role_of(block, kind, label)?;
                    if !usage.roles.contains_key(role) {
                        return Err(unknown_role(kind, label, role));
                    }
                    let arn = required_attribute(block, "policy_arn", kind, label)?;
                    let identities = attachments.entry(role.to_string()).or_default();
                    let identity = policy_identity(arn);
                    // The attachment that replaces it is attached before it is detached.
                    if creates_before_destroying(block) {
                        identities.insert(format!("{identity} (replacement)"));
                    }
                    identities.insert(identity);
                }
                "aws_iam_policy" => {
                    let policy = required_attribute(block, "policy", kind, label)?;
                    let chars = module
                        .policy_chars(policy)
                        .context(policy_context(kind, label))?;
                    usage.managed_policies.insert(label.to_string(), chars);
                }
                _ => {}
            }
        }
        for (role, identities) in attachments {
            usage.roles.entry(role).or_default().attachments = identities.len();
        }
        Ok(usage)
    }
}

/// The rendered module seen by the size check: its resources by address and its locals.
struct Module<'a> {
    resources: HashMap<(&'a str, &'a str), &'a Block>,
    locals: &'a IndexMap<String, Expression>,
    lengths: DeploymentLengths,
}

/// Locals may reference locals; deeper chains than this are a cycle.
const MAX_LOCAL_DEPTH: usize = 32;
/// The longest AWS partition name.
const LONGEST_PARTITION: &str = "aws-us-gov";
/// Characters of a KMS key ID.
const KMS_KEY_ID: usize = 36;
/// EKS's limit on cluster names.
const EKS_CLUSTER_NAME: usize = 100;

impl<'a> Module<'a> {
    fn new(
        blocks: &'a [Block],
        locals: &'a IndexMap<String, Expression>,
        lengths: DeploymentLengths,
    ) -> Self {
        let resources = blocks
            .iter()
            .filter_map(|block| {
                let kind = resource_type(block)?;
                let label = block.labels.get(1)?.as_str();
                Some(((kind, label), block))
            })
            .collect();
        Self {
            resources,
            locals,
            lengths,
        }
    }

    /// The `aws_iam_role` label a policy or attachment's `role` attribute names.
    fn role_of(&self, block: &'a Block, kind: &str, label: &str) -> Result<&'a str> {
        let role = required_attribute(block, "role", kind, label)?;
        let reference = resource_reference(role).filter(|reference| {
            reference.kind == "aws_iam_role" && matches!(reference.attribute, "id" | "name")
        });
        reference.map(|reference| reference.label).ok_or_else(|| {
            unbounded(
                role,
                &format!("{kind}.{label} must name a role created by this module"),
            )
        })
    }

    /// Largest non-whitespace characters IAM counts for a `policy` attribute.
    fn policy_chars(&self, policy: &Expression) -> Result<usize> {
        match policy {
            Expression::FuncCall(call)
                if call.name.to_string() == "jsonencode"
                    && call.args.len() == 1
                    && !call.expand_final =>
            {
                self.json_chars(&call.args[0], 0)
            }
            Expression::String(document) => Ok(non_whitespace(document)),
            other => Err(unbounded(
                other,
                "a policy must be jsonencode(...) or JSON text",
            )),
        }
    }

    /// Largest non-whitespace characters of `jsonencode(expression)`.
    fn json_chars(&self, expression: &Expression, depth: usize) -> Result<usize> {
        match expression {
            Expression::Null => Ok(4),
            Expression::Bool(value) => Ok(if *value { 4 } else { 5 }),
            Expression::Number(number) => Ok(number.to_string().len()),
            Expression::String(value) => json_string_chars(value),
            Expression::Array(values) => {
                let mut size = 2 + values.len().saturating_sub(1);
                for value in values {
                    size += self.json_chars(value, depth)?;
                }
                Ok(size)
            }
            Expression::Object(values) => {
                let mut size = 2 + values.len().saturating_sub(1);
                for (key, value) in values {
                    let key = match key {
                        ObjectKey::Identifier(key) => key.as_str(),
                        ObjectKey::Expression(Expression::String(key)) => key.as_str(),
                        _ => return Err(unbounded(expression, "object keys must be literal")),
                    };
                    size += json_string_chars(key)? + 1 + self.json_chars(value, depth)?;
                }
                Ok(size)
            }
            Expression::Parenthesis(inner) => self.json_chars(inner, depth),
            Expression::Conditional(conditional) => Ok(self
                .json_chars(&conditional.true_expr, depth)?
                .max(self.json_chars(&conditional.false_expr, depth)?)),
            // `compact` drops empty strings, so it is never longer than its argument.
            Expression::FuncCall(call)
                if call.name.to_string() == "compact"
                    && call.args.len() == 1
                    && !call.expand_final =>
            {
                self.json_chars(&call.args[0], depth)
            }
            Expression::FuncCall(call)
                if call.name.to_string() == "concat" && !call.expand_final =>
            {
                let mut items = Vec::new();
                for argument in &call.args {
                    let Expression::Array(values) = argument else {
                        return Err(unbounded(argument, "concat arguments must be lists"));
                    };
                    items.extend(values.iter().cloned());
                }
                self.json_chars(&Expression::Array(items), depth)
            }
            Expression::Traversal(traversal) => match local_path(traversal) {
                Some((name, attributes)) if name != "resource_prefix" => {
                    let local = self.local(expression, name, depth)?;
                    projections(local, &attributes, expression)?
                        .into_iter()
                        .try_fold(0, |size, value| {
                            Ok(size.max(self.json_chars(value, depth + 1)?))
                        })
                }
                _ => Ok(2 + self.string_chars(expression, depth)?),
            },
            Expression::TemplateExpr(_) | Expression::FuncCall(_) => {
                Ok(2 + self.string_chars(expression, depth)?)
            }
            other => Err(unbounded(other, "unsupported in a policy document")),
        }
    }

    /// Largest number of characters a string-valued expression resolves to.
    ///
    /// Interpolated values are names, identifiers and ARNs, which JSON never escapes, so this
    /// also bounds their JSON characters. Literal text around them is checked to be the same.
    fn string_chars(&self, expression: &Expression, depth: usize) -> Result<usize> {
        match expression {
            Expression::String(value) => plain_literal_chars(value, expression),
            Expression::Number(number) => Ok(number.to_string().len()),
            Expression::Bool(value) => Ok(if *value { 4 } else { 5 }),
            Expression::Parenthesis(inner) => self.string_chars(inner, depth),
            Expression::TemplateExpr(template) => self.template_chars(template, expression, depth),
            Expression::Traversal(traversal) => self.traversal_chars(traversal, expression, depth),
            Expression::Conditional(conditional) => {
                self.conditional_string_chars(conditional, depth)
            }
            Expression::FuncCall(call) => self.call_chars(call, expression, depth),
            other => Err(unbounded(other, "not a string this check can bound")),
        }
    }

    fn template_chars(
        &self,
        template: &TemplateExpr,
        expression: &Expression,
        depth: usize,
    ) -> Result<usize> {
        let parsed = Template::from_expr(template).into_alien_error().context(
            ErrorData::TemplateSerializationFailed {
                format: "Terraform IAM policy".into(),
                reason: format!("Failed to parse template `{expression}`"),
            },
        )?;
        let mut size = 0;
        for element in parsed.elements() {
            size += match element {
                Element::Literal(literal) => plain_literal_chars(literal, expression)?,
                Element::Interpolation(interpolation) => {
                    self.string_chars(&interpolation.expr, depth)?
                }
                Element::Directive(_) => {
                    return Err(unbounded(expression, "template directives are not bounded"))
                }
            };
        }
        Ok(size)
    }

    fn traversal_chars(
        &self,
        traversal: &Traversal,
        expression: &Expression,
        depth: usize,
    ) -> Result<usize> {
        let lengths = self.lengths;
        let Expression::Variable(root) = &traversal.expr else {
            return Err(unbounded(expression, "not a reference"));
        };
        let path = attribute_path(traversal)
            .ok_or_else(|| unbounded(expression, "splats and computed indexes are not bounded"))?;
        match (root.as_str(), path.as_slice()) {
            ("local", ["resource_prefix"]) => Ok(lengths.prefix),
            ("local", [name, attributes @ ..]) => {
                let local = self.local(expression, name, depth)?;
                projections(local, attributes, expression)?
                    .into_iter()
                    .try_fold(0, |size, value| {
                        Ok(size.max(self.string_chars(value, depth + 1)?))
                    })
            }
            ("data", ["aws_region", _, "region" | "name"]) => Ok(lengths.region),
            ("data", ["aws_caller_identity", _, "account_id"]) => Ok(lengths.account_id),
            ("data", ["aws_partition", _, "partition"]) => Ok(LONGEST_PARTITION.len()),
            ("var", ["managing_account_id"]) => Ok(lengths.managing_account_id),
            ("var", ["managing_role_arn"]) => Ok(lengths.managing_role_arn),
            // An existing cluster's name, which EKS caps at 100 characters.
            ("var", ["eks_cluster_name"]) => Ok(EKS_CLUSTER_NAME),
            // The key's identity postcondition admits only a KMS key ARN in this deployment's
            // account and region.
            ("var", [name, "arn"]) if name.ends_with("_existing_key") => {
                Ok(self.kms_key_arn_chars(LONGEST_PARTITION.len()))
            }
            (kind, [label, attribute]) => {
                self.resource_attribute_chars(kind, label, attribute, expression, depth)
            }
            _ => Err(unbounded(expression, "no known longest value")),
        }
    }

    /// Longest value of a resource attribute, derived from the name the module gives it.
    fn resource_attribute_chars(
        &self,
        kind: &str,
        label: &str,
        attribute_name: &str,
        expression: &Expression,
        depth: usize,
    ) -> Result<usize> {
        let block = self
            .resources
            .get(&(kind, label))
            .ok_or_else(|| unbounded(expression, "the resource is not in this module"))?;
        let named = |key: &str| -> Result<usize> {
            let name = attribute(block, key).ok_or_else(|| {
                unbounded(
                    expression,
                    &format!("{kind}.{label} has no literal `{key}`"),
                )
            })?;
            self.string_chars(name, depth)
        };
        // IAM paths default to `/`.
        let iam_path = || -> Result<usize> {
            attribute(block, "path").map_or(Ok(1), |path| self.string_chars(path, depth))
        };
        let region = self.lengths.region;
        let account = self.lengths.account_id;
        Ok(match (kind, attribute_name) {
            ("aws_s3_bucket", "bucket" | "id") => named("bucket")?,
            ("aws_s3_bucket", "arn") => "arn:aws:s3:::".len() + named("bucket")?,
            ("aws_ecr_repository", "name" | "id") => named("name")?,
            ("aws_ecr_repository", "arn") => {
                "arn:aws:ecr:".len() + region + 1 + account + ":repository/".len() + named("name")?
            }
            ("aws_ecr_repository", "repository_url") => {
                account + ".dkr.ecr.".len() + region + ".amazonaws.com/".len() + named("name")?
            }
            ("aws_dynamodb_table", "name" | "id") => named("name")?,
            ("aws_dynamodb_table", "arn") => {
                "arn:aws:dynamodb:".len() + region + 1 + account + ":table/".len() + named("name")?
            }
            ("aws_sqs_queue", "name") => named("name")?,
            ("aws_sqs_queue", "arn") => {
                "arn:aws:sqs:".len() + region + 1 + account + 1 + named("name")?
            }
            ("aws_sqs_queue", "id" | "url") => {
                "https://sqs.".len()
                    + region
                    + ".amazonaws.com/".len()
                    + account
                    + 1
                    + named("name")?
            }
            ("aws_iam_role", "name" | "id") => named("name")?,
            ("aws_iam_role", "arn") => {
                "arn:aws:iam::".len() + account + ":role".len() + iam_path()? + named("name")?
            }
            ("aws_iam_policy", "name") => named("name")?,
            ("aws_iam_policy", "arn") => {
                "arn:aws:iam::".len() + account + ":policy".len() + iam_path()? + named("name")?
            }
            ("aws_kms_key", "key_id" | "id") => KMS_KEY_ID,
            ("aws_kms_key", "arn") => self.kms_key_arn_chars("aws".len()),
            ("aws_eks_cluster", "name" | "id") => named("name")?,
            ("aws_eks_cluster", "arn") => {
                "arn:aws:eks:".len() + region + 1 + account + ":cluster/".len() + named("name")?
            }
            // A setup-recorded value is the first `input` it was given.
            ("terraform_data", "output") => named("input")?,
            _ => {
                return Err(unbounded(
                    expression,
                    "no known longest value for this attribute",
                ))
            }
        })
    }

    /// A KMS key ARN, whose key ID is a UUID or a multi-Region `mrk-` ID of the same length.
    fn kms_key_arn_chars(&self, partition: usize) -> usize {
        "arn:".len()
            + partition
            + ":kms:".len()
            + self.lengths.region
            + 1
            + self.lengths.account_id
            + ":key/".len()
            + KMS_KEY_ID
    }

    fn call_chars(&self, call: &FuncCall, expression: &Expression, depth: usize) -> Result<usize> {
        let name = call.name.to_string();
        let arguments = call.args.as_slice();
        if call.expand_final {
            return Err(unbounded(expression, "expanded arguments are not bounded"));
        }
        match (name.as_str(), arguments) {
            ("format", [Expression::String(format), values @ ..]) => {
                let mut values = values.iter();
                let mut size = 0;
                let mut characters = format.chars();
                while let Some(character) = characters.next() {
                    if character != '%' {
                        size += plain_literal_chars(&character.to_string(), expression)?;
                        continue;
                    }
                    match characters.next() {
                        Some('%') => size += 1,
                        Some('s') => {
                            let value = values.next().ok_or_else(|| {
                                unbounded(expression, "format has fewer values than verbs")
                            })?;
                            size += self.string_chars(value, depth)?;
                        }
                        _ => return Err(unbounded(expression, "only %s and %% are bounded")),
                    }
                }
                if values.next().is_some() {
                    return Err(unbounded(expression, "format has more values than verbs"));
                }
                Ok(size)
            }
            ("replace", [value, search, replacement]) => {
                if matches!(search, Expression::String(search) if search.starts_with('/')) {
                    return Err(unbounded(expression, "regular expressions are not bounded"));
                }
                // Replacing never lengthens the string when every match is at least as long
                // as the replacement.
                if self.string_chars(replacement, depth)? <= self.min_chars(search) {
                    self.string_chars(value, depth)
                } else {
                    Err(unbounded(
                        expression,
                        "the replacement can be longer than the match",
                    ))
                }
            }
            ("substr", [value, _offset, Expression::Number(length)]) => {
                let value = self.string_chars(value, depth)?;
                match length.as_i64() {
                    Some(-1) => Ok(value),
                    Some(length) if length >= 0 => Ok(value.min(length as usize)),
                    _ => Err(unbounded(expression, "invalid substring length")),
                }
            }
            ("sha1", [_]) => Ok(40),
            ("sha256", [_]) => Ok(64),
            ("md5", [_]) => Ok(32),
            ("lower" | "upper" | "trimspace" | "tostring", [value]) => {
                self.string_chars(value, depth)
            }
            _ => Err(unbounded(expression, "function not bounded")),
        }
    }

    /// Fewest characters a string-valued expression resolves to. Every deployment variable
    /// resolves to at least one character.
    fn min_chars(&self, expression: &Expression) -> usize {
        match expression {
            Expression::String(value) => value.chars().count(),
            Expression::FuncCall(call) if call.name.to_string() == "format" => {
                match call.args.split_first() {
                    Some((Expression::String(format), values)) => {
                        let verbs = format.matches("%s").count();
                        format.chars().count() - 2 * verbs
                            + values
                                .iter()
                                .map(|value| self.min_chars(value))
                                .sum::<usize>()
                    }
                    _ => 0,
                }
            }
            Expression::Traversal(traversal) => match traversal.expr {
                Expression::Variable(ref root)
                    if matches!(root.as_str(), "local" | "data" | "var") =>
                {
                    1
                }
                _ => 0,
            },
            _ => 0,
        }
    }

    /// Longest value of a conditional: the longer branch. Name templates are written
    /// `length(x) <= n ? x : shortened`: that yields `x` only while it is at most `n` characters
    /// long, and never the shortened name when `x` cannot exceed `n`.
    fn conditional_string_chars(&self, conditional: &Conditional, depth: usize) -> Result<usize> {
        let mut first = self.string_chars(&conditional.true_expr, depth)?;
        if let Some((value, limit)) = length_at_most(&conditional.cond_expr) {
            if self.string_chars(value, depth)? <= limit {
                return Ok(first);
            }
            if *value == conditional.true_expr {
                first = first.min(limit);
            }
        }
        Ok(first.max(self.string_chars(&conditional.false_expr, depth)?))
    }

    fn local(&self, expression: &Expression, name: &str, depth: usize) -> Result<&'a Expression> {
        if depth >= MAX_LOCAL_DEPTH {
            return Err(unbounded(
                expression,
                "locals reference each other in a cycle",
            ));
        }
        self.locals
            .get(name)
            .ok_or_else(|| unbounded(expression, "the local is not defined by this module"))
    }
}

/// `(x, n)` for a `length(x) <= n` condition.
fn length_at_most(condition: &Expression) -> Option<(&Expression, usize)> {
    let Expression::Operation(operation) = condition else {
        return None;
    };
    let Operation::Binary(binary) = operation.as_ref() else {
        return None;
    };
    let (Expression::FuncCall(call), BinaryOperator::LessEq, Expression::Number(limit)) =
        (&binary.lhs_expr, binary.operator, &binary.rhs_expr)
    else {
        return None;
    };
    match call.args.as_slice() {
        [value] if call.name.to_string() == "length" && !call.expand_final => {
            Some((value, usize::try_from(limit.as_u64()?).ok()?))
        }
        _ => None,
    }
}

/// A reference to an attribute of a managed resource, `kind.label[index].attribute`.
struct ResourceReference<'a> {
    kind: &'a str,
    label: &'a str,
    attribute: &'a str,
}

fn resource_reference(expression: &Expression) -> Option<ResourceReference<'_>> {
    let Expression::Traversal(traversal) = expression else {
        return None;
    };
    let Expression::Variable(root) = &traversal.expr else {
        return None;
    };
    match attribute_path(traversal)?.as_slice() {
        [label, attribute] => Some(ResourceReference {
            kind: root.as_str(),
            label,
            attribute,
        }),
        _ => None,
    }
}

/// Attribute names of a traversal, skipping literal instance indexes (`[0]`) that gating adds.
fn attribute_path(traversal: &Traversal) -> Option<Vec<&str>> {
    traversal
        .operators
        .iter()
        .filter_map(|operator| match operator {
            TraversalOperator::GetAttr(name) => Some(Some(name.as_str())),
            TraversalOperator::Index(Expression::Number(_)) => None,
            _ => Some(None),
        })
        .collect()
}

/// The local a traversal reads and the attributes it then selects.
fn local_path(traversal: &Traversal) -> Option<(&str, Vec<&str>)> {
    let Expression::Variable(root) = &traversal.expr else {
        return None;
    };
    if root.as_str() != "local" {
        return None;
    }
    let path = attribute_path(traversal)?;
    let (name, attributes) = path.split_first()?;
    Some((name, attributes.to_vec()))
}

/// The values `attributes` select from `value`. Either branch of a conditional can be taken,
/// except a null one: reading an attribute of null fails the apply.
fn projections<'e>(
    value: &'e Expression,
    attributes: &[&str],
    expression: &Expression,
) -> Result<Vec<&'e Expression>> {
    let Some((attribute, rest)) = attributes.split_first() else {
        return Ok(vec![value]);
    };
    match value {
        Expression::Object(values) => {
            let selected = values
                .iter()
                .find_map(|(key, value)| {
                    let key = match key {
                        ObjectKey::Identifier(key) => key.as_str(),
                        ObjectKey::Expression(Expression::String(key)) => key.as_str(),
                        _ => return None,
                    };
                    (key == *attribute).then_some(value)
                })
                .ok_or_else(|| unbounded(expression, "the local has no such attribute"))?;
            projections(selected, rest, expression)
        }
        Expression::Conditional(conditional) => {
            let mut values = Vec::new();
            for branch in [&conditional.true_expr, &conditional.false_expr] {
                if *branch != Expression::Null {
                    values.extend(projections(branch, attributes, expression)?);
                }
            }
            Ok(values)
        }
        Expression::Parenthesis(inner) => projections(inner, attributes, expression),
        _ => Err(unbounded(
            expression,
            "the local is not an object this check can read",
        )),
    }
}

/// Identity of an attached managed policy. References to the same resource or the same literal
/// ARN attach one policy; anything else counts as its own.
fn policy_identity(expression: &Expression) -> String {
    match resource_reference(expression) {
        Some(reference) if reference.kind == "aws_iam_policy" && reference.attribute == "arn" => {
            format!("aws_iam_policy.{}", reference.label)
        }
        _ => expression.to_string(),
    }
}

fn creates_before_destroying(block: &Block) -> bool {
    block
        .body
        .blocks()
        .filter(|nested| nested.identifier.as_str() == "lifecycle")
        .any(|lifecycle| {
            attribute(lifecycle, "create_before_destroy") == Some(&Expression::Bool(true))
        })
}

fn resource_type(block: &Block) -> Option<&str> {
    (block.identifier.as_str() == "resource")
        .then(|| block.labels.first().map(|label| label.as_str()))
        .flatten()
}

fn resource_label(block: &Block) -> Result<&str> {
    block
        .labels
        .get(1)
        .map(|label| label.as_str())
        .ok_or_else(|| {
            AlienError::new(ErrorData::TemplateSerializationFailed {
                format: "Terraform IAM policy".into(),
                reason: "an IAM resource block has no label".into(),
            })
        })
}

fn attribute<'a>(block: &'a Block, name: &str) -> Option<&'a Expression> {
    block.body.0.iter().find_map(|structure| match structure {
        Structure::Attribute(attr) if attr.key.as_str() == name => Some(&attr.expr),
        _ => None,
    })
}

fn required_attribute<'a>(
    block: &'a Block,
    name: &str,
    kind: &str,
    label: &str,
) -> Result<&'a Expression> {
    attribute(block, name).ok_or_else(|| {
        AlienError::new(ErrorData::TemplateSerializationFailed {
            format: "Terraform IAM policy".into(),
            reason: format!("{kind}.{label} has no `{name}` attribute"),
        })
    })
}

fn non_whitespace(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

/// Non-whitespace characters of a JSON string literal, quotes and escapes included.
fn json_string_chars(value: &str) -> Result<usize> {
    serde_json::to_string(value)
        .into_alien_error()
        .context(ErrorData::TemplateSerializationFailed {
            format: "Terraform IAM policy".into(),
            reason: "Failed to measure IAM policy string".into(),
        })
        .map(|json| non_whitespace(&json))
}

/// Characters of literal text inside an interpolated string. Such text must need no JSON
/// escape and hold no whitespace, so its length is also what IAM counts.
fn plain_literal_chars(literal: &str, expression: &Expression) -> Result<usize> {
    if literal
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\\')
    {
        return Err(unbounded(
            expression,
            "literal text with whitespace or JSON escapes",
        ));
    }
    Ok(literal.chars().count())
}

fn unbounded(expression: &Expression, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::TemplateSerializationFailed {
        format: "Terraform IAM policy".into(),
        reason: format!("Cannot bound the size of `{expression}` in an IAM policy: {reason}"),
    })
}

fn policy_context(kind: &str, label: &str) -> ErrorData {
    ErrorData::TemplateSerializationFailed {
        format: "Terraform IAM policy".into(),
        reason: format!("Failed to size the policy of {kind}.{label}"),
    }
}

fn unknown_role(kind: &str, label: &str, role: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::TemplateSerializationFailed {
        format: "Terraform IAM policy".into(),
        reason: format!(
            "{kind}.{label} targets aws_iam_role.{role}, which this module does not create"
        ),
    })
}

#[cfg(test)]
#[path = "iam_limits_tests.rs"]
mod tests;
