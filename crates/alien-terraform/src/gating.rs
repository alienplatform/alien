//! Generator-side gating post-pass for `.enabled(input)` resources.
//!
//! Emitters render plain, unconditional blocks; this pass makes a gated
//! resource conditional after the fact: every owned block gets the gate's
//! `count`, and every reference to a gated block anywhere in the module is
//! rewritten to index into the count (`type.label[0].attr`). Emitters
//! therefore need no gating knowledge at all — the fragment's shared-block
//! classification and the central residual allowlist are the only inputs.
//!
//! Two nets keep a miss loud instead of silent:
//!
//! - [`crate::emitters::enabled::gate`] refuses blocks that already carry a
//!   `count`, and this pass refuses `for_each`, so a colliding meta-argument
//!   fails at render time;
//! - [`scan_rendered_for_unindexed`] runs over the rendered files and fails
//!   generation if a gated address survives anywhere unindexed (references
//!   hidden in raw strings the AST walk cannot see), except inside
//!   `depends_on`, where Terraform requires whole-resource references.

use crate::emitter::TfFragment;
use crate::emitters::enabled;
use alien_core::{ErrorData, Result};
use alien_error::AlienError;
use hcl::{
    expr::{Expression, TemplateExpr, TraversalOperator},
    structure::{Block, Body, Structure},
    template::{Element, Template},
};
use std::collections::HashSet;

/// Block types with no cloud footprint, allowed to render ungated inside a
/// gated fragment. Central by design: an emitter author cannot assert
/// footprintlessness, only this list can grow it (as a reviewed change).
const RESIDUAL_BLOCK_TYPES: &[&str] = &["random_id"];

/// Nested blocks that execute code or open connections. A residual block
/// containing one is not footprintless, whatever its type says.
const SIDE_EFFECT_NESTED_BLOCKS: &[&str] = &["provisioner", "connection"];

/// Addresses (`type.label`) of blocks that were gated, shared across the
/// module for reference rewriting and the rendered-output scan.
#[derive(Debug, Default)]
pub(crate) struct GatedAddresses {
    addresses: HashSet<(String, String)>,
}

impl GatedAddresses {
    pub(crate) fn is_empty(&self) -> bool {
        self.addresses.is_empty()
    }

    fn contains(&self, provider_type: &str, label: &str) -> bool {
        self.addresses
            .contains(&(provider_type.to_string(), label.to_string()))
    }

    /// The addresses as `type.label` strings, for the rendered-output scan.
    pub(crate) fn rendered_forms(&self) -> Vec<String> {
        self.addresses
            .iter()
            .map(|(provider_type, label)| format!("{provider_type}.{label}"))
            .collect()
    }
}

/// Gate every owned block of a gated resource's fragment and record their
/// addresses. Shared blocks stay untouched; residual blocks stay ungated
/// after their footprintlessness is verified.
pub(crate) fn gate_fragment(
    fragment: &mut TfFragment,
    resource_id: &str,
    input_id: &str,
    gated: &mut GatedAddresses,
) -> Result<()> {
    let shared: Vec<bool> = fragment
        .resource_blocks
        .iter()
        .map(|block| fragment.is_shared(block))
        .collect();
    for (block, is_shared) in fragment.resource_blocks.iter_mut().zip(shared) {
        if block.identifier.as_str() != "resource" || is_shared {
            continue;
        }
        let (Some(provider_type), Some(label)) = (
            block.labels.first().map(|label| label.as_str().to_string()),
            block.labels.get(1).map(|label| label.as_str().to_string()),
        ) else {
            continue;
        };

        if RESIDUAL_BLOCK_TYPES.contains(&provider_type.as_str()) {
            verify_residual_is_footprintless(block, resource_id)?;
            continue;
        }

        if block
            .body
            .attributes()
            .any(|attribute| attribute.key.as_str() == "for_each")
        {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: format!("enabled() on `{provider_type}.{label}`"),
                reason: "the block declares `for_each`, and Terraform accepts only one of \
                         `for_each` and the `count` the gate needs"
                    .to_string(),
            }));
        }

        enabled::gate(block, Some(input_id))?;
        gated.addresses.insert((provider_type, label));
    }
    Ok(())
}

/// Install the gate on every block a fragment declared as carrying another
/// resource's gate, and record its address.
///
/// A shared emitter — the management role, a service account — renders grants
/// on behalf of resources the deployer can decline, while never being gated
/// itself. Those blocks cannot go through [`gate_fragment`], which only ever
/// sees the gated resource's own fragment, so the emitter declares them and
/// this applies them. Registering the address is the point: it puts them
/// inside the same reference rewrite and rendered-output scan as everything
/// else, instead of relying on nothing ever referencing them.
pub(crate) fn apply_gated_contributions(
    fragment: &mut TfFragment,
    gated: &mut GatedAddresses,
) -> Result<()> {
    let plan: Vec<Option<Vec<String>>> = fragment
        .resource_blocks
        .iter()
        .map(|block| {
            if fragment.is_shared(block) {
                return None;
            }
            fragment
                .contribution_gates(block)
                .map(|input_ids| input_ids.to_vec())
        })
        .collect();

    for (block, input_ids) in fragment.resource_blocks.iter_mut().zip(plan) {
        let Some(input_ids) = input_ids else {
            continue;
        };
        let (Some(provider_type), Some(label)) = (
            block.labels.first().map(|label| label.as_str().to_string()),
            block.labels.get(1).map(|label| label.as_str().to_string()),
        ) else {
            continue;
        };
        enabled::gate_any(block, &input_ids)?;
        gated.addresses.insert((provider_type, label));
    }
    Ok(())
}

/// A residual block renders ungated inside a gated fragment, so it must not
/// be able to create anything or run anything.
fn verify_residual_is_footprintless(block: &Block, resource_id: &str) -> Result<()> {
    let offending = block
        .body
        .blocks()
        .find(|nested| SIDE_EFFECT_NESTED_BLOCKS.contains(&nested.identifier.as_str()));
    if let Some(nested) = offending {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("enabled() on resource '{resource_id}'",),
            reason: format!(
                "its `{}` block would stay ungated as a footprintless residual, but it \
                 contains a `{}` block, which executes even when the resource is declined",
                block
                    .labels
                    .first()
                    .map(|label| label.as_str())
                    .unwrap_or("residual"),
                nested.identifier.as_str()
            ),
        }));
    }
    Ok(())
}

/// Rewrite every reference to a gated address across the module: fragments,
/// shared locals, and the registration's import expressions. Whole-resource
/// references (no attribute access after the label) are left alone —
/// `depends_on` requires them unindexed.
pub(crate) fn rewrite_fragment_references(fragment: &mut TfFragment, gated: &GatedAddresses) {
    for block in fragment
        .resource_blocks
        .iter_mut()
        .chain(fragment.data_blocks.iter_mut())
    {
        rewrite_block(block, gated);
    }
    for (_name, expression) in fragment.locals.iter_mut() {
        rewrite_expression(expression, gated);
    }
}

pub(crate) fn rewrite_expression(expression: &mut Expression, gated: &GatedAddresses) {
    match expression {
        Expression::Traversal(traversal) => {
            rewrite_expression(&mut traversal.expr, gated);
            for operator in traversal.operators.iter_mut() {
                if let TraversalOperator::Index(index) = operator {
                    rewrite_expression(index, gated);
                }
            }
            let Expression::Variable(root) = &traversal.expr else {
                return;
            };
            let [TraversalOperator::GetAttr(label), following, ..] = traversal.operators.as_slice()
            else {
                // A bare `type.label` whole-resource reference: valid on a
                // counted resource (`depends_on`), never indexed.
                return;
            };
            // Only plain attribute access needs the count index. A splat
            // (`.*` / `[*]`) over the counted resource is already list-aware
            // and stays unindexed, like `depends_on`; inserting `[0]` there
            // would splat a single instance and break when the gate is off.
            let indexes_attribute = matches!(following, TraversalOperator::GetAttr(_));
            if indexes_attribute && gated.contains(root.as_str(), label.as_str()) {
                traversal.operators.insert(
                    1,
                    TraversalOperator::Index(Expression::Number(hcl::Number::from(0))),
                );
            }
        }
        Expression::TemplateExpr(template_expr) => {
            rewrite_template(template_expr, gated);
        }
        Expression::Array(items) => {
            for item in items {
                rewrite_expression(item, gated);
            }
        }
        Expression::Object(object) => {
            let needs_key_rewrite = object
                .keys()
                .any(|key| matches!(key, hcl::expr::ObjectKey::Expression(_)));
            if needs_key_rewrite {
                let entries: Vec<(hcl::expr::ObjectKey, Expression)> = std::mem::take(object)
                    .into_iter()
                    .map(|(mut key, mut value)| {
                        if let hcl::expr::ObjectKey::Expression(expression) = &mut key {
                            rewrite_expression(expression, gated);
                        }
                        rewrite_expression(&mut value, gated);
                        (key, value)
                    })
                    .collect();
                *object = entries.into_iter().collect();
            } else {
                for (_key, value) in object.iter_mut() {
                    rewrite_expression(value, gated);
                }
            }
        }
        Expression::FuncCall(func_call) => {
            for arg in func_call.args.iter_mut() {
                rewrite_expression(arg, gated);
            }
        }
        Expression::Conditional(conditional) => {
            rewrite_expression(&mut conditional.cond_expr, gated);
            rewrite_expression(&mut conditional.true_expr, gated);
            rewrite_expression(&mut conditional.false_expr, gated);
        }
        Expression::Operation(operation) => match operation.as_mut() {
            hcl::expr::Operation::Unary(unary) => rewrite_expression(&mut unary.expr, gated),
            hcl::expr::Operation::Binary(binary) => {
                rewrite_expression(&mut binary.lhs_expr, gated);
                rewrite_expression(&mut binary.rhs_expr, gated);
            }
        },
        Expression::Parenthesis(inner) => rewrite_expression(inner, gated),
        Expression::ForExpr(for_expr) => {
            rewrite_expression(&mut for_expr.collection_expr, gated);
            rewrite_expression(&mut for_expr.value_expr, gated);
            if let Some(key_expr) = for_expr.key_expr.as_mut() {
                rewrite_expression(key_expr, gated);
            }
            if let Some(cond_expr) = for_expr.cond_expr.as_mut() {
                rewrite_expression(cond_expr, gated);
            }
        }
        _ => {}
    }
}

fn rewrite_block(block: &mut Block, gated: &GatedAddresses) {
    let body: Vec<Structure> = std::mem::take(&mut block.body).into_iter().collect();
    let rewritten = body
        .into_iter()
        .map(|structure| match structure {
            Structure::Attribute(mut attribute) => {
                // `depends_on` takes whole-resource references that stay
                // unindexed on counted resources.
                if attribute.key.as_str() != "depends_on" {
                    rewrite_expression(&mut attribute.expr, gated);
                }
                Structure::Attribute(attribute)
            }
            Structure::Block(mut nested) => {
                rewrite_block(&mut nested, gated);
                Structure::Block(nested)
            }
        })
        .collect::<Vec<_>>();
    block.body = Body::from(rewritten);
}

/// Rewrite interpolations inside a string template. The template is
/// re-serialized only when a rewrite actually happened, so untouched
/// templates render byte-identically.
fn rewrite_template(template_expr: &mut TemplateExpr, gated: &GatedAddresses) {
    let Ok(mut template) = Template::from_expr(template_expr) else {
        // Unparseable templates are left for the rendered-output scan.
        return;
    };
    let mut changed = false;
    rewrite_template_elements(&mut template, gated, &mut changed);
    if !changed {
        return;
    }
    match template_expr {
        TemplateExpr::QuotedString(quoted) => *quoted = template.to_string(),
        TemplateExpr::Heredoc(heredoc) => heredoc.template = template.to_string(),
    }
}

/// Interpolations plus `%{if}` / `%{for}` directives, recursively — a gated
/// reference inside a directive's condition or body needs the same index as
/// one in a plain interpolation.
fn rewrite_template_elements(template: &mut Template, gated: &GatedAddresses, changed: &mut bool) {
    for element in template.elements_mut() {
        match element {
            Element::Interpolation(interpolation) => {
                let before = interpolation.expr.clone();
                rewrite_expression(&mut interpolation.expr, gated);
                if interpolation.expr != before {
                    *changed = true;
                }
            }
            Element::Directive(directive) => match directive.as_mut() {
                hcl::template::Directive::If(directive) => {
                    let before = directive.cond_expr.clone();
                    rewrite_expression(&mut directive.cond_expr, gated);
                    if directive.cond_expr != before {
                        *changed = true;
                    }
                    rewrite_template_elements(&mut directive.true_template, gated, changed);
                    if let Some(false_template) = directive.false_template.as_mut() {
                        rewrite_template_elements(false_template, gated, changed);
                    }
                }
                hcl::template::Directive::For(directive) => {
                    let before = directive.collection_expr.clone();
                    rewrite_expression(&mut directive.collection_expr, gated);
                    if directive.collection_expr != before {
                        *changed = true;
                    }
                    rewrite_template_elements(&mut directive.template, gated, changed);
                }
            },
            Element::Literal(_) => {}
        }
    }
}

/// Fail generation when a gated address survives unindexed in the rendered
/// output — the net for references hidden in raw strings the AST walk cannot
/// see. `depends_on = [...]` spans are exempt.
pub(crate) fn scan_rendered_for_unindexed(
    files: &indexmap::IndexMap<String, String>,
    gated: &GatedAddresses,
) -> Result<()> {
    let addresses = gated.rendered_forms();
    if addresses.is_empty() {
        return Ok(());
    }
    for (file_name, contents) in files {
        if !file_name.ends_with(".tf") {
            continue;
        }
        let exempt = depends_on_spans(contents);
        for address in &addresses {
            let mut search_from = 0;
            while let Some(found) = contents[search_from..].find(address.as_str()) {
                let start = search_from + found;
                let end = start + address.len();
                search_from = end;

                let preceded = contents[..start].chars().next_back();
                if preceded.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.') {
                    continue;
                }
                let follower = contents[end..].chars().next();
                if follower.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
                    continue;
                }
                if follower == Some('[') && is_instance_index(&contents[end..]) {
                    continue;
                }
                // The dot-form attribute splat is the other legal unindexed
                // continuation: the rewrite deliberately leaves splats alone,
                // and `.*` renders verbatim, not normalized to `[*]`.
                if contents[end..].starts_with(".*") {
                    continue;
                }
                if exempt.iter().any(|span| span.contains(&start)) {
                    continue;
                }
                // The resource's own declaration is `resource "type" "label"`,
                // quoted, so it never matches the dotted form. Deterministic:
                // the same stack renders the same escape every time, so the
                // refusal must not be retryable.
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: format!("enabled() on `{address}`"),
                    reason: format!(
                        "the resource is referenced without its count index in `{file_name}`; \
                         the reference escaped the rewrite (raw string?) and would fail or, \
                         worse, silently mis-resolve when the deployer declines the resource"
                    ),
                }));
            }
        }
    }
    Ok(())
}

/// True when the text starts with a real instance index — `[0]` (any digits)
/// or the full splat `[*]` — the only bracket forms that are valid directly
/// on a counted resource address.
fn is_instance_index(text: &str) -> bool {
    let Some(inner) = text.strip_prefix('[') else {
        return false;
    };
    let Some(close) = inner.find(']') else {
        return false;
    };
    let index = inner[..close].trim();
    index == "*" || (!index.is_empty() && index.chars().all(|ch| ch.is_ascii_digit()))
}

/// Byte ranges of `depends_on = [ ... ]` attribute values, where
/// whole-resource references are required to stay unindexed.
fn depends_on_spans(contents: &str) -> Vec<std::ops::Range<usize>> {
    let mut spans = Vec::new();
    let mut search_from = 0;
    while let Some(found) = contents[search_from..].find("depends_on") {
        let start = search_from + found;
        search_from = start + "depends_on".len();
        // Only the attribute form `depends_on = [` opens an exemption; the
        // word appearing in prose (a README paragraph, a comment) must not
        // exempt whatever bracket happens to follow it.
        let after = &contents[start + "depends_on".len()..];
        let assignment: String = after
            .chars()
            .take_while(|ch| ch.is_whitespace() || *ch == '=')
            .collect();
        if !assignment.contains('=') || !after[assignment.len()..].starts_with('[') {
            continue;
        }
        let open = start + "depends_on".len() + assignment.len();
        let mut depth = 0usize;
        let mut close = None;
        for (offset, ch) in contents[open..].char_indices() {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(close) = close {
            spans.push(start..close + 1);
            search_from = close + 1;
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{attr, resource_block};
    use crate::expr;

    fn gated_analytics() -> GatedAddresses {
        let mut gated = GatedAddresses::default();
        gated
            .addresses
            .insert(("aws_dynamodb_table".to_string(), "analytics".to_string()));
        gated
    }

    /// A contributed block is gated AND registered. Registration is the half
    /// that matters: without it the block carries a count that the reference
    /// rewrite and the rendered-output scan know nothing about, and the only
    /// thing keeping that safe is nothing ever referencing it.
    #[test]
    fn a_declared_contribution_is_gated_and_registered() {
        let mut fragment = TfFragment::default();
        fragment.push_gated_resource(
            resource_block(
                "aws_iam_role_policy",
                "mgmt_jobs",
                [attr("role", expr::raw("x"))],
            ),
            std::slice::from_ref(&"jobsEnabled".to_string()),
        );
        let mut gated = GatedAddresses::default();

        apply_gated_contributions(&mut fragment, &mut gated).expect("a plain block gates");

        let rendered = crate::generator::render_body(hcl::structure::Body::from(vec![
            hcl::structure::Structure::Block(fragment.resource_blocks[0].clone()),
        ]))
        .expect("renders");
        assert!(
            rendered.contains("var.input_jobs_enabled ? 1 : 0"),
            "the contribution carries the gate:\n{rendered}"
        );
        assert!(
            gated
                .rendered_forms()
                .contains(&"aws_iam_role_policy.mgmt_jobs".to_string()),
            "and its address reaches the rewrite and the scan: {:?}",
            gated.rendered_forms()
        );
    }

    /// A block merged from several contributors exists while ANY of them is
    /// enabled, and is registered once.
    #[test]
    fn a_contribution_merged_from_several_gates_carries_all_of_them() {
        let mut fragment = TfFragment::default();
        fragment.push_gated_resource(
            resource_block(
                "azurerm_role_assignment",
                "mgmt",
                [attr("scope", expr::raw("x"))],
            ),
            &["auditEnabled".to_string(), "jobsEnabled".to_string()],
        );
        let mut gated = GatedAddresses::default();

        apply_gated_contributions(&mut fragment, &mut gated).expect("a plain block gates");

        let rendered = crate::generator::render_body(hcl::structure::Body::from(vec![
            hcl::structure::Structure::Block(fragment.resource_blocks[0].clone()),
        ]))
        .expect("renders");
        assert!(
            rendered.contains("var.input_audit_enabled || var.input_jobs_enabled ? 1 : 0"),
            "the merged grant survives while any contributor is enabled:\n{rendered}"
        );
        assert_eq!(gated.rendered_forms().len(), 1);
    }

    #[test]
    fn attribute_references_gain_the_count_index() {
        let mut expression = expr::traversal(["aws_dynamodb_table", "analytics", "name"]);
        rewrite_expression(&mut expression, &gated_analytics());
        assert_eq!(
            expression,
            expr::traversal_indexed("aws_dynamodb_table", "analytics", "name")
        );
    }

    #[test]
    fn whole_resource_references_stay_unindexed() {
        let mut expression = expr::traversal(["aws_dynamodb_table", "analytics"]);
        let before = expression.clone();
        rewrite_expression(&mut expression, &gated_analytics());
        assert_eq!(expression, before);
    }

    #[test]
    fn ungated_references_are_untouched() {
        let mut expression = expr::traversal(["aws_dynamodb_table", "other", "name"]);
        let before = expression.clone();
        rewrite_expression(&mut expression, &gated_analytics());
        assert_eq!(expression, before);
    }

    #[test]
    fn splat_references_stay_unindexed() {
        for splat in [
            "aws_dynamodb_table.analytics[*].name",
            "aws_dynamodb_table.analytics.*.name",
        ] {
            let mut expression = expr::parse(splat).expect("valid splat");
            let before = expression.clone();
            rewrite_expression(&mut expression, &gated_analytics());
            assert_eq!(expression, before, "{splat}");
        }
    }

    #[test]
    fn scan_accepts_instance_indexes_but_not_key_indexes() {
        let mut ok_files = indexmap::IndexMap::new();
        ok_files.insert(
            "main.tf".to_string(),
            "locals { a = aws_dynamodb_table.analytics[0].name, b = aws_dynamodb_table.analytics[*].name }"
                .to_string(),
        );
        scan_rendered_for_unindexed(&ok_files, &gated_analytics())
            .expect("instance indexes and splats are valid on a counted resource");

        let mut bad_files = indexmap::IndexMap::new();
        bad_files.insert(
            "main.tf".to_string(),
            "locals { x = aws_dynamodb_table.analytics[\"key\"].name }".to_string(),
        );
        scan_rendered_for_unindexed(&bad_files, &gated_analytics())
            .expect_err("a key index on a counted resource is not an instance index");
    }

    #[test]
    fn scan_accepts_dot_form_splats_but_not_plain_attributes() {
        let mut ok_files = indexmap::IndexMap::new();
        ok_files.insert(
            "main.tf".to_string(),
            "locals { a = aws_dynamodb_table.analytics.*.name }".to_string(),
        );
        scan_rendered_for_unindexed(&ok_files, &gated_analytics())
            .expect("the dot-form splat renders verbatim and is list-aware, exactly like `[*]`");

        let mut bad_files = indexmap::IndexMap::new();
        bad_files.insert(
            "main.tf".to_string(),
            "locals { x = aws_dynamodb_table.analytics.name }".to_string(),
        );
        scan_rendered_for_unindexed(&bad_files, &gated_analytics())
            .expect_err("a plain attribute access is still an unindexed escape");
    }

    #[test]
    fn prose_mentioning_depends_on_exempts_nothing() {
        let mut files = indexmap::IndexMap::new();
        files.insert(
            "main.tf".to_string(),
            "# depends_on ordering notes\nlocals { x = [aws_dynamodb_table.analytics.name] }"
                .to_string(),
        );
        scan_rendered_for_unindexed(&files, &gated_analytics())
            .expect_err("only `depends_on = [` opens an exemption");
    }

    #[test]
    fn template_directives_are_rewritten() {
        let mut expression =
            expr::template("%{ if aws_dynamodb_table.analytics.name != \"\" }yes%{ endif }");
        rewrite_expression(&mut expression, &gated_analytics());
        assert_eq!(
            expression,
            expr::template("%{ if aws_dynamodb_table.analytics[0].name != \"\" }yes%{ endif }")
        );
    }

    #[test]
    fn template_interpolations_are_rewritten() {
        let mut expression = expr::template("${aws_dynamodb_table.analytics.name}-suffix");
        rewrite_expression(&mut expression, &gated_analytics());
        assert_eq!(
            expression,
            expr::template("${aws_dynamodb_table.analytics[0].name}-suffix")
        );
    }

    #[test]
    fn untouched_templates_are_not_reserialized() {
        let original = "${var.something}-${local.other}";
        let mut expression = expr::template(original);
        rewrite_expression(&mut expression, &gated_analytics());
        assert_eq!(expression, expr::template(original));
    }

    #[test]
    fn depends_on_attributes_are_never_rewritten() {
        let mut block = resource_block(
            "aws_s3_bucket",
            "logs",
            [attr(
                "depends_on",
                Expression::Array(vec![expr::traversal(["aws_dynamodb_table", "analytics"])]),
            )],
        );
        let before = block.clone();
        rewrite_block(&mut block, &gated_analytics());
        assert_eq!(block, before);
    }

    #[test]
    fn residual_random_id_stays_ungated_and_forbids_provisioners() {
        let mut fragment = TfFragment::default()
            .with_resource(resource_block(
                "random_id",
                "suffix",
                [attr(
                    "byte_length",
                    Expression::Number(hcl::Number::from(4)),
                )],
            ))
            .with_resource(resource_block(
                "aws_dynamodb_table",
                "analytics",
                [attr("name", expr::template("x"))],
            ));
        let mut gated = GatedAddresses::default();
        gate_fragment(&mut fragment, "analytics", "analyticsEnabled", &mut gated)
            .expect("residual random_id must not block gating");

        let random_id = &fragment.resource_blocks[0];
        assert!(
            !random_id
                .body
                .attributes()
                .any(|attribute| attribute.key.as_str() == "count"),
            "residual block must stay ungated"
        );
        let table = &fragment.resource_blocks[1];
        assert!(
            table
                .body
                .attributes()
                .any(|attribute| attribute.key.as_str() == "count"),
            "owned block must be gated"
        );
        assert!(gated.contains("aws_dynamodb_table", "analytics"));
        assert!(!gated.contains("random_id", "suffix"));
    }

    #[test]
    fn residual_with_provisioner_is_refused() {
        let mut residual = resource_block(
            "random_id",
            "suffix",
            [attr(
                "byte_length",
                Expression::Number(hcl::Number::from(4)),
            )],
        );
        let provisioner =
            crate::block::block("provisioner", [attr("command", expr::template("rm -rf /"))]);
        let body: Vec<Structure> = std::mem::take(&mut residual.body)
            .into_iter()
            .chain([Structure::Block(provisioner)])
            .collect();
        residual.body = Body::from(body);

        let mut fragment = TfFragment::default().with_resource(residual);
        let error = gate_fragment(
            &mut fragment,
            "analytics",
            "analyticsEnabled",
            &mut GatedAddresses::default(),
        )
        .expect_err("a provisioner inside a residual must be refused");
        assert!(error.message.contains("provisioner"), "{}", error.message);
    }

    #[test]
    fn for_each_on_an_owned_block_is_refused() {
        let mut fragment = TfFragment::default().with_resource(resource_block(
            "aws_dynamodb_table",
            "analytics",
            [attr("for_each", expr::raw("toset([\"a\"])"))],
        ));
        let error = gate_fragment(
            &mut fragment,
            "analytics",
            "analyticsEnabled",
            &mut GatedAddresses::default(),
        )
        .expect_err("for_each cannot compose with the gate's count");
        assert!(error.message.contains("for_each"), "{}", error.message);
    }

    #[test]
    fn shared_blocks_are_not_gated() {
        let mut fragment = TfFragment::default();
        fragment.push_shared_resource(resource_block(
            "google_project_iam_custom_role",
            "gcp_role_reader",
            [attr(
                "count",
                expr::raw("var.gcp_manage_custom_roles ? 1 : 0"),
            )],
        ));
        gate_fragment(
            &mut fragment,
            "analytics",
            "analyticsEnabled",
            &mut GatedAddresses::default(),
        )
        .expect("a shared block carrying its own count must be skipped, not an error");
    }

    #[test]
    fn scan_flags_unindexed_gated_addresses_outside_depends_on() {
        let mut files = indexmap::IndexMap::new();
        files.insert(
            "main.tf".to_string(),
            "locals { x = \"${aws_dynamodb_table.analytics.name}\" }".to_string(),
        );
        let error = scan_rendered_for_unindexed(&files, &gated_analytics())
            .expect_err("unindexed reference must fail generation");
        assert!(error.message.contains("aws_dynamodb_table.analytics"));

        let mut ok_files = indexmap::IndexMap::new();
        ok_files.insert(
            "main.tf".to_string(),
            "resource \"aws_dynamodb_table\" \"analytics\" { count = var.x ? 1 : 0 }\n\
             locals { x = aws_dynamodb_table.analytics[0].name }\n\
             resource \"null_resource\" \"import\" { depends_on = [\n  aws_dynamodb_table.analytics,\n] }"
                .to_string(),
        );
        scan_rendered_for_unindexed(&ok_files, &gated_analytics())
            .expect("indexed references and depends_on spans are fine");
    }
}
