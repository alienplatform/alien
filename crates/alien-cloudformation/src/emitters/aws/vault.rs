//! AWS Vault — Parameter Store namespace.
//!
//! AWS Systems Manager Parameter Store is account-and-region-scoped, not
//! a CloudFormation resource. The vault is realized as a name prefix
//! (`${AWS::StackName}-{vault.id}`) that the controller uses for
//! `ssm:PutParameter`. ImportData carries the prefix so importers can
//! reconstruct the vault namespace without a cloud lookup.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::resource_config,
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, Result, Vault};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsVaultEmitter;

impl CfEmitter for AwsVaultEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        resource_config::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        Ok(vec![])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let vault = resource_config::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        Ok(CfExpression::object([
            ("accountId", CfExpression::ref_("AWS::AccountId")),
            ("region", CfExpression::ref_("AWS::Region")),
            (
                "parameterPrefix",
                CfExpression::sub(format!("${{AWS::StackName}}-{}", vault.id())),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let vault = resource_config::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("parameter-store")),
            (
                "vaultPrefix",
                CfExpression::sub(format!("${{AWS::StackName}}-{}", vault.id())),
            ),
        ])))
    }
}
