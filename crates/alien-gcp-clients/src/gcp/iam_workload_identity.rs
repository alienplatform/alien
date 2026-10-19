//! OIDC workload identity pool/provider messages from the IAM v1 API.

use std::collections::HashMap;

use bon::Builder;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadIdentityPool {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[builder(default)]
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadIdentityPoolProvider {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[builder(default)]
    #[serde(default)]
    pub disabled: bool,
    #[builder(default)]
    #[serde(default)]
    pub attribute_mapping: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute_condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc: Option<WorkloadIdentityOidc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadIdentityOidc {
    pub issuer_uri: String,
    #[builder(default)]
    #[serde(default)]
    pub allowed_audiences: Vec<String>,
}
