use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpRemoteBindingsImportData {
    pub project_id: String,
    pub service_account_email: String,
    pub service_account_unique_id: String,
}
