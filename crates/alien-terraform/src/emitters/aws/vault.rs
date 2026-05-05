//! AWS Vault — SSM Parameter Store namespace.
//!
//! AWS Systems Manager Parameter Store is account-and-region-scoped, not
//! a discrete resource. The vault is realized as a name prefix
//! (`${var.stack_name}-{vault.id}`) the controller uses for
//! `ssm:PutParameter`. ImportData carries the prefix so importers can
//! reconstruct the namespace without a cloud lookup.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{downcast, required_label},
    expr,
};
use alien_core::{import::EmitContext, Result, Vault};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsVaultEmitter;

impl TfEmitter for AwsVaultEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        Ok(TfFragment::empty())
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        Ok(expr::object([
            (
                "accountId",
                expr::raw("data.aws_caller_identity.current.account_id"),
            ),
            ("region", expr::raw("data.aws_region.current.region")),
            (
                "parameterPrefix",
                expr::template(format!("${{var.stack_name}}-{}", vault.id())),
            ),
        ]))
    }
}
