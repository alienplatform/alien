use alien_extension_sdk::prelude::*;

/// An AWS budget that emails when actual monthly cost passes 80% of the limit.
#[alien_resource(type = "budget/alert", outputs = BudgetAlertOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema, bon::Builder)]
#[serde(rename_all = "camelCase")]
#[builder(start_fn = new)]
pub struct BudgetAlert {
    #[builder(start_fn)]
    pub id: String,
    /// The monthly limit, in US dollars.
    pub monthly_limit_usd: u32,
    /// Who gets the alert.
    pub email: String,
}

/// What Alien records after initial setup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BudgetAlertOutputs {
    pub budget_name: String,
}
