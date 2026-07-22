//! Rendering for a resource whose creation follows a deployer input.
//!
//! `.enabled(input)` in the SDK means the deployer decides whether the resource
//! exists. Terraform learns that answer as a variable at apply time, not when we
//! render, so the decision has to live in the template: the resource block takes
//! a `count`, and anything the emitter publishes about the resource has to stay
//! renderable when that count is zero.
//!
//! Two things follow, and both are mandatory for a gated resource:
//!
//! - references to its own blocks need `[0]`, since a counted resource is a list
//! - it must drop out of the registration list entirely, because the manager
//!   deserializes every entry there into typed import data with required fields

use crate::{block::attr, expr};
use alien_core::{ErrorData, Result};
use alien_error::AlienError;
use hcl::{
    expr::{Conditional, Expression, FuncCall},
    structure::{Block, Body, Structure},
    Identifier,
};

/// Terraform variable carrying the gate's value.
///
/// Mirrors `terraform_stack_input_variable_name`; the compile-time check
/// guarantees the input is a non-null boolean, so the expressions below never
/// need a null guard.
fn gate_variable(input_id: &str) -> String {
    format!(
        "var.{}",
        crate::generator::stack_input_variable_name(input_id)
    )
}

/// `count` for a gated resource block, or `None` when the resource is ungated
/// and should render exactly as it did before. Private so that [`gate`] stays
/// the only way a block acquires the gate, and its checks cannot be bypassed.
fn enabled_count(enabled_when: Option<&str>) -> Option<Expression> {
    enabled_when.map(|input_id| expr::raw(format!("{} ? 1 : 0", gate_variable(input_id))))
}

/// Put the resource's gate on a block that is already built. An ungated block
/// passes through byte-identical, so an existing stack re-renders without a diff.
///
/// Every gated block goes through here, including the ones that come back from
/// the shared IAM emitters that ungated resources call too: the caller gates the
/// blocks it just appended instead of pushing the gate into a signature every
/// other call site would thread `None` through.
///
/// Fails when the block already carries a `count`. Terraform allows only one, so
/// prepending a second would render invalid HCL, and silently keeping either one
/// would drop a meta-argument someone wrote on purpose. No emitter that opts into
/// `.enabled()` counts its own blocks today; this is here so the first one that
/// tries finds out at render time.
pub fn gate(block: &mut Block, enabled_when: Option<&str>) -> Result<()> {
    let Some(count) = enabled_count(enabled_when) else {
        return Ok(());
    };
    if block
        .body
        .attributes()
        .any(|attribute| attribute.key.as_str() == "count")
    {
        let address = block
            .labels
            .iter()
            .map(|label| label.as_str())
            .collect::<Vec<_>>()
            .join(".");
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("enabled() on `{address}`"),
            reason: "the block already declares its own `count`, and Terraform accepts only one \
                     count per block"
                .to_string(),
        }));
    }
    let mut body: Vec<Structure> = std::mem::take(&mut block.body).into_iter().collect();
    body.insert(0, attr("count", count));
    block.body = Body::from(body);
    Ok(())
}

/// Reference to an attribute of a resource that may be gated, indexing into the
/// count when it is.
pub fn attribute(
    enabled_when: Option<&str>,
    resource_type: &str,
    label: &str,
    attribute: &str,
) -> Expression {
    match enabled_when {
        Some(_) => expr::traversal_indexed(resource_type, label, attribute),
        None => expr::traversal([resource_type, label, attribute]),
    }
}

/// The registration list the manager consumes, one entry per resource.
///
/// A resource the deployer declined contributes no entry at all. It does not
/// exist, so there is nothing to describe, and a null placeholder would not
/// survive the manager's typed deserialization of every entry. Gated entries
/// therefore render as single-element lists that collapse to empty, spliced
/// together with `concat`.
///
/// A stack with nothing gated renders a plain array, byte-identical to what
/// an existing stack carries, so a re-apply shows no diff.
pub fn registration_list(entries: Vec<(Option<String>, Expression)>) -> Expression {
    if entries.iter().all(|(gate, _)| gate.is_none()) {
        return Expression::Array(entries.into_iter().map(|(_, entry)| entry).collect());
    }

    let mut concat = FuncCall::builder(Identifier::sanitized("concat"));
    for (gate, entry) in entries {
        let single = Expression::Array(vec![entry]);
        concat = concat.arg(match gate {
            Some(input_id) => Expression::Conditional(Box::new(Conditional::new(
                expr::raw(gate_variable(&input_id)),
                single,
                Expression::Array(Vec::new()),
            ))),
            None => single,
        });
    }
    Expression::FuncCall(Box::new(concat.build()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::resource_block;

    /// No emitter counts its own blocks today, so this is the only place the
    /// collision path runs. Without it the guard would ship unexecuted.
    #[test]
    fn gating_a_block_that_already_counts_itself_fails() {
        let mut block = resource_block(
            "aws_s3_bucket",
            "already_counted",
            [attr("count", Expression::Number(hcl::Number::from(2)))],
        );

        let error = gate(&mut block, Some("bucketEnabled"))
            .expect_err("a second count would render invalid HCL");

        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
        assert!(
            error.message.contains("aws_s3_bucket.already_counted"),
            "the error should name the offending block: {}",
            error.message
        );
        assert!(
            error.message.contains("count"),
            "the error should say which meta-argument collided: {}",
            error.message
        );
    }

    #[test]
    fn gating_an_ungated_block_leaves_it_alone() {
        let mut block = resource_block("aws_s3_bucket", "plain", [attr("bucket", "x".into())]);
        let before = block.clone();

        gate(&mut block, None).expect("an ungated block is never rewritten");

        assert_eq!(block, before);
    }
}

/// [`gate`] reading the resource's own gate off the emit context, so an
/// emitter's own blocks structurally cannot miss it. The `Option<&str>` form
/// stays for callers gating blocks on a *different* resource's gate.
pub fn gate_own(ctx: &alien_core::import::EmitContext<'_>, block: &mut Block) -> Result<()> {
    gate(block, ctx.resource.enabled_when.as_deref())
}

/// [`attribute`] reading the resource's own gate off the emit context.
pub fn self_attribute(
    ctx: &alien_core::import::EmitContext<'_>,
    resource_type: &str,
    label: &str,
    attr_name: &str,
) -> Expression {
    attribute(
        ctx.resource.enabled_when.as_deref(),
        resource_type,
        label,
        attr_name,
    )
}
