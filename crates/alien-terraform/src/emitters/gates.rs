//! Deploy-time permission gating as Terraform `count` expressions.
//!
//! Emitters ask [`permission_gate_count`] for the `count` to place on a
//! permission grant block. `Ok(None)` means the grant is ungated and the
//! block is emitted unconditionally, so a stack with no gates renders identically.

use crate::{expr, generator::terraform_stack_input_variable_name};
use alien_core::{import::EmitContext, ErrorData, Result, StackInputKind};
use alien_error::AlienError;

/// Re-exported so the Terraform and CloudFormation emitters share one definition.
pub use alien_core::TrackedPermissionRef;
use hcl::expr::Expression;

/// A resolved deployer gate for one permission grant: the gating stack
/// input's identity plus the `count` expression to place on the grant
/// block. The identity fields let emitters derive stable per-gate names
/// (e.g. Azure role-assignment UUID seeds) without a second lookup.
pub struct PermissionGateExpression {
    pub input_id: String,
    pub enabled_value: String,
    pub count: Expression,
}

/// `count` expression gating a permission set's grant block on the stack
/// input selected by [`alien_core::Stack::deployer_permission_gate`], or
/// `None` when the set is ungated for this platform across `origin_keys`.
pub fn permission_gate_count(
    ctx: &EmitContext<'_>,
    profile: &str,
    permission_set_id: &str,
    origin_keys: &[&str],
) -> Result<Option<Expression>> {
    Ok(permission_gate(ctx, profile, permission_set_id, origin_keys)?.map(|gate| gate.count))
}

/// [`permission_gate_count`] plus the gating input's identity.
pub fn permission_gate(
    ctx: &EmitContext<'_>,
    profile: &str,
    permission_set_id: &str,
    origin_keys: &[&str],
) -> Result<Option<PermissionGateExpression>> {
    let Some(gate) =
        ctx.stack
            .deployer_permission_gate(ctx.platform, profile, permission_set_id, origin_keys)
    else {
        return Ok(None);
    };

    let input = ctx
        .stack
        .inputs()
        .iter()
        .find(|input| input.id == gate.input_id)
        .ok_or_else(|| {
            AlienError::new(ErrorData::TemplateSerializationFailed {
                format: "Terraform".to_string(),
                reason: format!(
                    "permission gate on '{permission_set_id}' references unknown stack input '{}'",
                    gate.input_id
                ),
            })
        })?;

    match input.kind {
        // String/Enum/Boolean need no value check here: the count compares as a
        // string and fails closed on any mismatch.
        StackInputKind::String | StackInputKind::Enum | StackInputKind::Boolean => {}
        StackInputKind::Number | StackInputKind::Integer => {
            if gate.enabled_value.parse::<f64>().is_err() {
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "gate Terraform permission grant".to_string(),
                    reason: format!(
                        "enabled value '{}' for numeric stack input '{}' is not a number",
                        gate.enabled_value, gate.input_id
                    ),
                }));
            }
        }
        StackInputKind::StringList | StackInputKind::Secret => {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "gate Terraform permission grant".to_string(),
                reason: format!(
                    "stack input '{}' has kind {:?}, which has no single comparable enabled value",
                    gate.input_id, input.kind
                ),
            }));
        }
    }

    // Fail closed: an unset optional input (null) denies the gated grant rather
    // than granting it. A self-hosted `terraform apply` is the final IAM with no
    // later prune, so an omitted input must not silently keep a gated permission.
    // The null check is a separate ternary (not `&&`) so `tostring(null)` never
    // evaluates; `tostring` because Boolean inputs are `bool` vars.
    let variable = format!("var.{}", terraform_stack_input_variable_name(input));
    let enabled_literal =
        serde_json::to_string(&gate.enabled_value).expect("serializing a string cannot fail");
    Ok(Some(PermissionGateExpression {
        input_id: gate.input_id.clone(),
        enabled_value: gate.enabled_value.clone(),
        count: expr::raw(format!(
            "{variable} == null ? 0 : (tostring({variable}) == {enabled_literal} ? 1 : 0)"
        )),
    }))
}

