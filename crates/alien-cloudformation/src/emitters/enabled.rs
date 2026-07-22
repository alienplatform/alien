//! Rendering for a resource whose creation follows a deployer input.
//!
//! `.enabled(input)` in the SDK means the deployer decides whether the resource
//! exists. CloudFormation learns that answer as a parameter at deploy time, not
//! when we render, so the decision has to live in the template: the resource
//! carries a `Condition`, and nothing that outlives it may reference it.
//!
//! Two things follow, and the generator does both:
//!
//! - every resource the emitter returns gets the gate's `Condition`
//! - the resource's registration entry is dropped from the payload entirely
//!
//! Dropping the whole entry is the part that is easy to get wrong. Registration
//! runs the typed importer over every entry it receives, so an entry that is
//! present but null — or present with null fields — fails deserialization
//! instead of being skipped. There is no skip-on-null anywhere in that path.
//! `AWS::NoValue` removes the list element outright, which is the only shape
//! that leaves nothing to deserialize.

use crate::{generator::stack_input_parameter_name_for_id, template::CfExpression};

/// Name of the condition carrying the gate's value. The generator declares it
/// and stamps it onto resources, so both derive it from the input id alone.
pub fn condition_name(input_id: &str) -> String {
    format!("{}IsTrue", stack_input_parameter_name_for_id(input_id))
}

/// Yields `value` while the gate is on and `when_disabled` while it is off.
///
/// Pass `CfExpression::no_value()` as `when_disabled` to delete the value rather
/// than blank it: inside a list that removes the element, and inside an object
/// it removes the key.
pub fn when_enabled(
    enabled_when: Option<&str>,
    value: CfExpression,
    when_disabled: CfExpression,
) -> CfExpression {
    match enabled_when {
        Some(input_id) => CfExpression::if_(condition_name(input_id), value, when_disabled),
        None => value,
    }
}
