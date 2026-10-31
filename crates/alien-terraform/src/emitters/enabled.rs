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
use alien_core::Result;
use hcl::{
    Identifier,
    expr::{Conditional, Expression, FuncCall},
    structure::{Block, Body, Structure},
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

/// Put the resource's gate on a block that is already built. An ungated block
/// passes through byte-identical, so an existing stack re-renders without a diff.
///
/// Every gated block goes through here, including the ones that come back from
/// the shared IAM emitters that ungated resources call too: the caller gates the
/// blocks it just appended instead of pushing the gate into a signature every
/// other call site would thread `None` through.
///
/// Preserve an emitter's existing count when enabled, and set it to zero when
/// disabled. For example, a Key creates its native key only when the installer
/// has not selected an existing key. Both decisions must hold before creation.
pub fn gate(block: &mut Block, enabled_when: Option<&str>) -> Result<()> {
    let Some(input_id) = enabled_when else {
        return Ok(());
    };
    gate_with_condition(block, expr::parse(&gate_variable(input_id))?);
    Ok(())
}

/// Shared tail of [`gate`] and [`gate_any`]: compose the gate with any existing
/// count without changing resource instance addresses.
fn gate_with_condition(block: &mut Block, condition: Expression) {
    let count = |enabled_count| {
        Expression::Conditional(Box::new(Conditional::new(
            condition,
            enabled_count,
            Expression::Number(0.into()),
        )))
    };
    if let Some(attribute) = block
        .body
        .attributes_mut()
        .find(|attribute| attribute.key.as_str() == "count")
    {
        attribute.expr = count(std::mem::replace(&mut attribute.expr, Expression::Null));
        return;
    }
    let mut body: Vec<Structure> = std::mem::take(&mut block.body).into_iter().collect();
    body.insert(0, attr("count", count(Expression::Number(1.into()))));
    block.body = Body::from(body);
}

/// Put a merged contribution's gate on a block: the grant exists when ANY of
/// the resources that asked for it is enabled.
///
/// Some grants collapse several resources into one cloud object — Azure folds
/// every resource-scoped management permission set into a single role
/// assignment at one scope. Such a block cannot carry one resource's gate; it
/// has to carry all of them, or declining one resource would revoke a grant a
/// sibling still needs. Callers must pass gates for EVERY contributor, so an
/// ungated contributor means the block stays ungated (there is no expression
/// that can represent "always" alongside the others).
pub fn gate_any(block: &mut Block, input_ids: &[String]) -> Result<()> {
    match input_ids {
        [] => Ok(()),
        [input_id] => gate(block, Some(input_id)),
        _ => {
            let condition = input_ids
                .iter()
                .map(|input_id| gate_variable(input_id))
                .collect::<Vec<_>>()
                .join(" || ");
            gate_with_condition(block, expr::parse(&condition)?);
            Ok(())
        }
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

    #[test]
    fn gating_an_ungated_block_leaves_it_alone() {
        let mut block = resource_block("aws_s3_bucket", "plain", [attr("bucket", "x".into())]);
        let before = block.clone();

        gate(&mut block, None).expect("an ungated block is never rewritten");

        assert_eq!(block, before);
    }
}
