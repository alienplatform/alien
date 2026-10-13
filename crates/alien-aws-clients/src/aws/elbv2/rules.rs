//! Listener rules for host routing and URL rewrites.
use std::collections::HashMap;

use bon::Builder;
use reqwest::Method;
use serde::{Deserialize, de::DeserializeOwned};

use super::{Action, ActionsWrapper, ElbTag, Elbv2Client};
use crate::aws::aws_request_utils::AwsRequestBuilderExt;
use alien_client_core::Result;

/// Create a listener rule. Host conditions are checked before transforms run.
#[derive(Debug, Clone, Builder)]
pub struct CreateRuleRequest {
    pub listener_arn: String,
    pub priority: i32,
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<Action>,
    #[builder(default)]
    pub transforms: Vec<RuleTransform>,
    #[builder(default)]
    pub tags: Vec<ElbTag>,
}

/// Read exact rules or enumerate a listener's rules, including pagination.
#[derive(Debug, Clone, Default, Builder)]
pub struct DescribeRulesRequest {
    pub listener_arn: Option<String>,
    #[builder(default)]
    pub rule_arns: Vec<String>,
    pub marker: Option<String>,
    pub page_size: Option<i32>,
}

/// Update only supplied fields. ResetTransforms explicitly removes rewrites.
#[derive(Debug, Clone, Builder)]
pub struct ModifyRuleRequest {
    pub rule_arn: String,
    pub conditions: Option<Vec<RuleCondition>>,
    pub actions: Option<Vec<Action>>,
    pub transforms: Option<Vec<RuleTransform>>,
    pub reset_transforms: Option<bool>,
}

/// Host-header and simple value conditions. Other provider fields remain
/// identifiable by `field`; callers must reject unexpected condition kinds.
#[derive(Debug, Clone, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct RuleCondition {
    pub field: String,
    #[serde(default)]
    #[builder(default)]
    pub values: RuleValues,
    pub host_header_config: Option<HostHeaderConditionConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuleValues {
    #[serde(rename = "member", default)]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct HostHeaderConditionConfig {
    #[serde(default)]
    #[builder(default)]
    pub values: RuleValues,
    #[serde(default)]
    #[builder(default)]
    pub regex_values: RuleValues,
}

#[derive(Debug, Clone, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct RuleTransform {
    #[serde(rename = "Type")]
    pub transform_type: String,
    pub url_rewrite_config: Option<RewriteConfig>,
    pub host_header_rewrite_config: Option<RewriteConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RewriteConfig {
    pub rewrites: Rewrites,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rewrites {
    #[serde(rename = "member", default)]
    pub members: Vec<Rewrite>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Rewrite {
    pub regex: String,
    pub replace: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateRuleResponse {
    pub create_rule_result: RulesResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyRuleResponse {
    pub modify_rule_result: RulesResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeRulesResponse {
    pub describe_rules_result: RulesResult,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RulesResult {
    pub rules: Rules,
    pub next_marker: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rules {
    #[serde(rename = "member", default)]
    pub members: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Rule {
    pub rule_arn: String,
    /// AWS also returns the literal `default` for the default rule.
    pub priority: String,
    pub is_default: bool,
    pub conditions: RuleConditions,
    pub actions: ActionsWrapper,
    #[serde(default)]
    pub transforms: RuleTransforms,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleConditions {
    #[serde(rename = "member", default)]
    pub members: Vec<RuleCondition>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuleTransforms {
    #[serde(rename = "member", default)]
    pub members: Vec<RuleTransform>,
}

pub(super) fn rule_form(action: &str) -> HashMap<String, String> {
    HashMap::from([
        ("Action".into(), action.into()),
        ("Version".into(), "2015-12-01".into()),
    ])
}

pub(super) fn add_conditions(form: &mut HashMap<String, String>, conditions: &[RuleCondition]) {
    for (index, condition) in conditions.iter().enumerate() {
        let prefix = format!("Conditions.member.{}", index + 1);
        form.insert(format!("{prefix}.Field"), condition.field.clone());
        add_values(form, &format!("{prefix}.Values"), &condition.values);
        if let Some(host) = &condition.host_header_config {
            add_values(
                form,
                &format!("{prefix}.HostHeaderConfig.Values"),
                &host.values,
            );
            add_values(
                form,
                &format!("{prefix}.HostHeaderConfig.RegexValues"),
                &host.regex_values,
            );
        }
    }
}

fn add_values(form: &mut HashMap<String, String>, prefix: &str, values: &RuleValues) {
    for (index, value) in values.members.iter().enumerate() {
        form.insert(format!("{prefix}.member.{}", index + 1), value.clone());
    }
}

pub(super) fn add_transforms(form: &mut HashMap<String, String>, transforms: &[RuleTransform]) {
    for (index, transform) in transforms.iter().enumerate() {
        let prefix = format!("Transforms.member.{}", index + 1);
        form.insert(format!("{prefix}.Type"), transform.transform_type.clone());
        for (field, config) in [
            ("UrlRewriteConfig", &transform.url_rewrite_config),
            (
                "HostHeaderRewriteConfig",
                &transform.host_header_rewrite_config,
            ),
        ] {
            if let Some(config) = config {
                for (index, rewrite) in config.rewrites.members.iter().enumerate() {
                    let prefix = format!("{prefix}.{field}.Rewrites.member.{}", index + 1);
                    form.insert(format!("{prefix}.Regex"), rewrite.regex.clone());
                    form.insert(format!("{prefix}.Replace"), rewrite.replace.clone());
                }
            }
        }
    }
}

impl Elbv2Client {
    pub(super) async fn send_rule_mutation<T: DeserializeOwned + Send + 'static>(
        &self,
        form: HashMap<String, String>,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form.iter())
            .finish();
        let builder = self
            .client
            .request(Method::POST, self.get_base_url())
            .host(&self.get_host())
            .content_type_form()
            .content_sha256(&body)
            .body(body.clone());
        // CreateRule has no idempotency token. A retry must first read the
        // existing priority and validate ownership, rather than blindly mutate.
        let result = crate::aws::aws_request_utils::sign_send_xml_retrying_when(
            builder,
            &self.sign_config(),
            |_| false,
        )
        .await;
        Self::map_result(result, operation, resource, Some(&body))
    }
}
