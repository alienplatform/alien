use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureRemoteBindingsImportData {
    pub tenant_id: String,
    pub identity_id: String,
    pub principal_id: String,
    pub client_id: String,
}
