//! Tells the operator, before a setup that writes ECR replication, which rules the account
//! already has. ECR keeps one replication configuration per account and replacing it drops
//! every rule, so the setup does not replace it: its custom resource reads the rules at the
//! moment of the write, keeps every rule that is not this registry's unchanged, and removes
//! only this registry's rule on delete. The read here uses the identity the preflight runs
//! with and is informational: the write-time read is what guarantees nothing is dropped.

use crate::error::Result;
use crate::{CheckResult, RuntimeCheck};
use alien_aws_clients::{aws::ecr::ReplicationRule, AwsCredentialProvider, EcrApi, EcrClient};
use alien_core::{
    ArtifactRegistry, ClientConfig, DeploymentConfig, Platform, RegistryReplicationOwner,
    RegistryReplicationReport, Stack, StackState,
};
use std::time::Duration;

/// Classify the registry's replication rules: none, exactly the rule this registry's setup
/// writes (its repository prefix, destinations in the same account), or anything else.
pub fn classify_replication(
    rules: &[ReplicationRule],
    repository_prefix: &str,
    account_id: &str,
) -> RegistryReplicationReport {
    if rules.is_empty() {
        return RegistryReplicationReport {
            owner: RegistryReplicationOwner::None,
            foreign_rules: vec![],
        };
    }
    let foreign_rules = rules
        .iter()
        .filter(|rule| {
            let ours = rule.repository_filters.len() == 1
                && rule.repository_filters[0].filter_type == "PREFIX_MATCH"
                && rule.repository_filters[0].filter == repository_prefix
                && !rule.destinations.is_empty()
                && rule
                    .destinations
                    .iter()
                    .all(|destination| destination.registry_id == account_id);
            !ours
        })
        .map(|rule| {
            let regions = rule
                .destinations
                .iter()
                .map(|d| format!("{} (account {})", d.region, d.registry_id))
                .collect::<Vec<_>>()
                .join(", ");
            let filters = if rule.repository_filters.is_empty() {
                "all repositories".to_string()
            } else {
                rule.repository_filters
                    .iter()
                    .map(|f| format!("{} {}", f.filter_type, f.filter))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!("copies {filters} to {regions}")
        })
        .collect::<Vec<_>>();
    RegistryReplicationReport {
        owner: if foreign_rules.is_empty() {
            RegistryReplicationOwner::Ours
        } else {
            RegistryReplicationOwner::Foreign
        },
        foreign_rules,
    }
}

/// Regions the deployer answered for a registry's replication input, in order.
fn requested_regions(config: &DeploymentConfig, input_id: &str) -> Vec<String> {
    let strings: Vec<String> = match config.input_values.get(input_id) {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        Some(serde_json::Value::String(text)) => text.split(',').map(str::to_string).collect(),
        _ => Vec::new(),
    };
    strings
        .into_iter()
        .map(|region| region.trim().to_string())
        .filter(|region| !region.is_empty())
        .collect()
}

pub struct ReplicationRulesCheck;

#[async_trait::async_trait]
impl RuntimeCheck for ReplicationRulesCheck {
    fn code(&self) -> Option<&'static str> {
        Some("ECR_REPLICATION_RULES_FOREIGN")
    }

    fn description(&self) -> &'static str {
        "Lists the account's ECR replication rules that setup keeps unchanged"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Aws
            && stack.resources.values().any(|entry| {
                entry
                    .config
                    .downcast_ref::<ArtifactRegistry>()
                    .is_some_and(|registry| {
                        registry.replication_regions_input.is_some()
                            || !registry.replication_regions.is_empty()
                    })
            })
    }

    async fn check(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
        client_config: &ClientConfig,
    ) -> Result<CheckResult> {
        let mut result = CheckResult::success();
        let registries: Vec<(&ArtifactRegistry, Vec<String>)> = stack
            .resources
            .values()
            .filter_map(|entry| entry.config.downcast_ref::<ArtifactRegistry>())
            .filter_map(|registry| {
                let regions = match registry.replication_regions_input.as_deref() {
                    Some(input) => requested_regions(config, input),
                    None => registry.replication_regions.clone(),
                };
                (!regions.is_empty()).then_some((registry, regions))
            })
            .collect();
        if registries.is_empty() {
            return Ok(result);
        }
        let Some(aws) = client_config.aws_config() else {
            result
                .add_warning("ECR replication rules could not be read: no AWS credentials".into());
            return Ok(result);
        };
        let ecr = match AwsCredentialProvider::from_config(aws.clone()).await {
            Ok(credentials) => {
                let http = reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()
                    .ok();
                match http {
                    Some(http) => EcrClient::new(http, credentials),
                    None => {
                        result.add_warning(
                            "ECR replication rules could not be read: no HTTP client".into(),
                        );
                        return Ok(result);
                    }
                }
            }
            Err(_) => {
                result.add_warning(
                    "ECR replication rules could not be read: credentials failed".into(),
                );
                return Ok(result);
            }
        };
        for (registry, _) in registries {
            let prefix = format!("{}-{}", stack_state.resource_prefix, registry.id);
            check_registry(&ecr, &prefix, &aws.account_id, &mut result).await;
        }
        Ok(result)
    }
}

/// Read the account's rules and name every one that is not this registry's, with the note
/// that setup keeps them. A failed read is a warning: it does not change what setup writes.
pub async fn check_registry(
    ecr: &dyn EcrApi,
    repository_prefix: &str,
    account_id: &str,
    result: &mut CheckResult,
) {
    match ecr.describe_registry().await {
        Ok(registry) => {
            let report: RegistryReplicationReport = classify_replication(
                &registry.replication_configuration.rules,
                repository_prefix,
                account_id,
            );
            if report.owner == RegistryReplicationOwner::Foreign {
                result.add_warning(format!(
                    "This AWS account already has ECR replication rules that are not this registry's ({}). Setup keeps them unchanged and adds this registry's rule; deleting the setup removes only this registry's rule.",
                    report.foreign_rules.join("; ")
                ));
            }
        }
        Err(error) => {
            result.add_warning(format!(
                "ECR replication rules could not be read ({error}); setup reads them again when it writes and keeps rules that are not this registry's"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::aws::ecr::{
        DescribeRegistryResponse, MockEcrApi, ReplicationConfiguration, ReplicationDestination,
        RepositoryFilter,
    };

    const ACCOUNT: &str = "123456789012";

    fn rule(prefix: Option<&str>, registry_id: &str, region: &str) -> ReplicationRule {
        ReplicationRule {
            destinations: vec![ReplicationDestination {
                region: region.to_string(),
                registry_id: registry_id.to_string(),
            }],
            repository_filters: prefix
                .map(|p| {
                    vec![RepositoryFilter {
                        filter: p.to_string(),
                        filter_type: "PREFIX_MATCH".to_string(),
                    }]
                })
                .unwrap_or_default(),
        }
    }

    fn registry_with(rules: Vec<ReplicationRule>) -> MockEcrApi {
        let mut ecr = MockEcrApi::new();
        ecr.expect_describe_registry().returning(move || {
            Ok(DescribeRegistryResponse {
                registry_id: ACCOUNT.to_string(),
                replication_configuration: ReplicationConfiguration {
                    rules: rules.clone(),
                },
            })
        });
        ecr
    }

    async fn run(rules: Vec<ReplicationRule>) -> CheckResult {
        let mut result = CheckResult::success();
        check_registry(&registry_with(rules), "t-images", ACCOUNT, &mut result).await;
        result
    }

    #[tokio::test]
    async fn no_rules_and_exactly_our_rule_pass() {
        assert!(run(vec![]).await.success);
        assert!(
            run(vec![rule(Some("t-images"), ACCOUNT, "us-east-2")])
                .await
                .success
        );
    }

    #[tokio::test]
    async fn a_foreign_rule_is_named_and_kept_not_refused() {
        for foreign in [
            rule(None, ACCOUNT, "eu-west-1"),
            rule(Some("other-"), ACCOUNT, "eu-west-1"),
            rule(Some("t-images"), "999999999999", "eu-west-1"),
        ] {
            let result = run(vec![foreign]).await;
            assert!(result.success, "{:?}", result.errors);
            assert!(
                result.warnings[0].contains("eu-west-1") && result.warnings[0].contains("keeps"),
                "{:?}",
                result.warnings
            );
        }
        let mixed = run(vec![
            rule(Some("t-images"), ACCOUNT, "us-east-2"),
            rule(None, ACCOUNT, "eu-west-1"),
        ])
        .await;
        assert!(mixed.success);
        assert_eq!(mixed.warnings.len(), 1);
    }

    #[tokio::test]
    async fn a_failed_read_is_a_warning_and_does_not_change_the_write() {
        let mut ecr = MockEcrApi::new();
        ecr.expect_describe_registry().returning(|| {
            Err(alien_aws_clients::Error::new(
                alien_aws_clients::ErrorData::RemoteAccessDenied {
                    resource_type: "ecr".into(),
                    resource_name: "registry".into(),
                },
            ))
        });
        let mut result = CheckResult::success();
        check_registry(&ecr, "t-images", ACCOUNT, &mut result).await;
        assert!(result.success);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn regions_come_from_a_list_or_a_comma_string() {
        let mut config = DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build();
        config
            .input_values
            .insert("r".into(), serde_json::json!(["us-east-2", " "]));
        assert_eq!(requested_regions(&config, "r"), vec!["us-east-2"]);
        config
            .input_values
            .insert("r".into(), serde_json::json!("us-east-2, eu-west-1"));
        assert_eq!(
            requested_regions(&config, "r"),
            vec!["us-east-2", "eu-west-1"]
        );
        assert!(requested_regions(&config, "missing").is_empty());
    }
}
