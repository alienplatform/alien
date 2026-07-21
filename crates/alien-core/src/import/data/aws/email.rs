use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// AWS Email ImportData.
///
/// Mirrors the `emit_import_ref` payload of the AWS SES email emitter: the
/// shared configuration set name, one entry per seed domain carrying its
/// Easy-DKIM CNAME records, and — when inbound mail is configured — the
/// receipt rule set name. Identities created at runtime through the
/// `email/manage-identities` grant are application data and never appear
/// here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsEmailImportData {
    /// SES configuration set name (used when sending).
    pub configuration_set: String,
    /// Per-seed-domain DNS data, keyed by mail domain.
    pub domains: BTreeMap<String, AwsEmailDomainImportData>,
    /// SES receipt rule set name, present only when inbound mail is
    /// configured. Activating the rule set is a manual post-deploy step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_set_name: Option<String>,
}

/// Per-domain import payload for an AWS Email resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsEmailDomainImportData {
    /// Easy-DKIM CNAME tokens (three per domain). The domain is verified by
    /// SES once these records exist in its DNS configuration.
    pub dkim_tokens: Vec<AwsEmailDkimTokenImportData>,
}

/// A single Easy-DKIM CNAME record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsEmailDkimTokenImportData {
    /// CNAME record host name.
    pub name: String,
    /// CNAME record value.
    pub value: String,
}
