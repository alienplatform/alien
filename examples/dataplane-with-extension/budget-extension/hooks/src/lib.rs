use alien_extension_sdk::prelude::*;
use budget_types::{BudgetAlert, BudgetAlertOutputs};

pub struct CloudFormation;

impl CfEmitter for CloudFormation {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let alert = resource_config::<BudgetAlert>(ctx, BudgetAlert::RESOURCE_TYPE)?;
        let mut budget = CfResource::new(required_logical_id(ctx)?.to_string(), "AWS::Budgets::Budget".into());
        budget.properties.insert("Budget".into(), CfExpression::object([
            ("BudgetName", stack_name(&alert.id)),
            ("BudgetType", CfExpression::String("COST".into())),
            ("TimeUnit", CfExpression::String("MONTHLY".into())),
            ("BudgetLimit", cf_from_json(json!({ "Amount": alert.monthly_limit_usd, "Unit": "USD" }))?),
        ]));
        budget.properties.insert("NotificationsWithSubscribers".into(), cf_from_json(json!([{
            "Notification": {
                "NotificationType": "ACTUAL",
                "ComparisonOperator": "GREATER_THAN",
                "Threshold": 80,
                "ThresholdType": "PERCENTAGE",
            },
            "Subscribers": [{ "SubscriptionType": "EMAIL", "Address": alert.email }],
        }]))?);
        Ok(vec![budget])
    }

    /// What the importer reads after initial setup. Ref on a budget is its name.
    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        Ok(CfExpression::object([("budgetName", CfExpression::ref_(required_logical_id(ctx)?))]))
    }
}

pub struct Terraform;

impl TfEmitter for Terraform {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let alert = downcast::<BudgetAlert>(ctx, BudgetAlert::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(TfFragment::default().with_resource(resource_block("aws_budgets_budget", label, [
            attr("name", resource_prefix_template(&alert.id)),
            attr("budget_type", Expression::String("COST".into())),
            attr("time_unit", Expression::String("MONTHLY".into())),
            attr("limit_amount", Expression::String(alert.monthly_limit_usd.to_string())),
            attr("limit_unit", Expression::String("USD".into())),
            nested(block("notification", [
                attr("notification_type", Expression::String("ACTUAL".into())),
                attr("comparison_operator", Expression::String("GREATER_THAN".into())),
                attr("threshold", Expression::Number(80.into())),
                attr("threshold_type", Expression::String("PERCENTAGE".into())),
                attr("subscriber_email_addresses", Expression::Array(vec![Expression::String(alert.email.clone())])),
            ])),
        ])))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([("budgetName", expr::traversal(["aws_budgets_budget", label, "name"]))]))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetImport {
    pub budget_name: String,
}

pub struct Importer;

impl ResourceImporter for Importer {
    type ImportData = BudgetImport;

    fn import(&self, data: BudgetImport, ctx: &ImportContext<'_>) -> Result<StackResourceState> {
        // No controller, so there are no fields to save, only the outputs.
        ctx.ready((), BudgetAlertOutputs { budget_name: data.budget_name })
    }
}

fn register(ext: &mut Extension) {
    ext.resource::<BudgetAlert>()
        .cloudformation_emitter(Platform::Aws, CloudFormation)
        .terraform_emitter(Platform::Aws, Terraform)
        .importer(Platform::Aws, Importer);
}

alien_extension_sdk::export!("budget", register);
