//! The one-profile rule for Azure Builds is enforced where a release cannot skip it: the
//! preflights that run on every deploy, updates included.
#![cfg(feature = "runtime-checks")]

use alien_core::{
    Build, DeploymentConfig, EnvironmentVariablesSnapshot, Platform, ResourceLifecycle, Stack,
    StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;

fn stack(builds: &[(&str, &str)]) -> Stack {
    let mut stack = Stack::new("builds".to_string());
    for (id, profile) in builds {
        stack = stack.add(
            Build::new(id.to_string())
                .permissions(profile.to_string())
                .build(),
            ResourceLifecycle::Frozen,
        );
    }
    stack.build()
}

fn config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build()
}

async fn deploy_time(
    platform: Platform,
    target: Stack,
    installed: Option<&Stack>,
) -> Result<(), String> {
    PreflightRunner::new()
        .run_deployment_time_preflights(
            target,
            &StackState::new(platform),
            &config(),
            &alien_core::ClientConfig::Test,
            installed,
            None,
            None,
        )
        .await
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
}

#[tokio::test]
async fn the_deploy_path_refuses_azure_builds_with_two_profiles() {
    // A fresh install and an update of an install that had one profile both go through it.
    let one = stack(&[("a", "build")]);
    let two = stack(&[("a", "build"), ("b", "other")]);
    for installed in [None, Some(&one)] {
        let error = deploy_time(Platform::Azure, two.clone(), installed)
            .await
            .expect_err("two build profiles on Azure are refused at deploy time");
        assert!(
            error.contains("one permission profile") && error.contains("'other': b"),
            "{error}"
        );
    }
}

#[tokio::test]
async fn the_deploy_path_does_not_apply_the_rule_to_other_clouds() {
    let two = stack(&[("a", "build"), ("b", "other")]);
    for platform in [Platform::Aws, Platform::Gcp] {
        if let Err(error) = deploy_time(platform, two.clone(), None).await {
            assert!(!error.contains("one permission profile"), "{platform}: {error}");
        }
    }
}
