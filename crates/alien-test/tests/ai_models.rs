//! Real-AWS proof of the AI model access check.
//!
//! An AI resource declares one model the account is refused (`claude-sonnet-5`) and one it may
//! call (`claude-sonnet-4-6`). The deployment still reaches `running`; the check result is
//! recorded in the deployment's runtime metadata.
//!
//! Run in a region where the account's access matches that (`us-west-2` on the shared test
//! account):
//!   AWS_TARGET_REGION=us-west-2 cargo nextest run -p alien-test --test ai_models --run-ignored all

use std::sync::Arc;
use std::time::{Duration, Instant};

use alien_core::{Ai, AiModel, Platform, ResourceLifecycle};
use alien_test::{DeploymentModel, TestConfig, TestManager};

#[tokio::test]
#[ignore = "real AWS: sends one-token Bedrock test calls in the target account"]
async fn aws_declared_models_are_checked_and_a_refusal_does_not_block() {
    alien_test::e2e::init_tracing();
    let config = TestConfig::from_env();
    let manager = Arc::new(
        TestManager::start_with_config(&config, &[Platform::Aws])
            .await
            .expect("manager"),
    );
    let stack = alien_core::Stack::new("ai-models".to_string())
        .add(
            Ai::new("llm".to_string())
                .models(vec![
                    AiModel {
                        id: "claude-sonnet-5".into(),
                        required: true,
                    },
                    AiModel {
                        id: "claude-sonnet-4-6".into(),
                        required: true,
                    },
                ])
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let deployment = alien_test::e2e::create_release_and_deployment(
        &manager,
        Platform::Aws,
        DeploymentModel::Push,
        &stack,
        format!("e2e-ai-models-{}", &uuid::Uuid::new_v4().to_string()[..8]),
        None,
    )
    .await
    .expect("deployment");
    println!("DEPLOYMENT_ID={}", deployment.id);

    let outcome: anyhow::Result<()> = async {
        // The check needs the admin's own credentials; the least-privilege setup role cannot
        // call a model, and a check that cannot run reports "not-checked".
        alien_test::setup::setup_target_as_admin(
            &config,
            Platform::Aws,
            &deployment,
            &manager,
            manager.management_config(),
        )
        .await?;
        let start = Instant::now();
        loop {
            let resp = manager
                .client()
                .get_deployment()
                .id(&deployment.id)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("get_deployment: {e}"))?;
            let meta = resp.runtime_metadata.clone().unwrap_or_default();
            println!(
                "[{:>5.1}s] status={} aiAccessChecks={}",
                start.elapsed().as_secs_f32(),
                resp.status,
                meta["aiAccessChecks"]
            );
            if resp.status == "running" {
                let checks = meta["aiAccessChecks"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let result_of = |model: &str| {
                    checks
                        .iter()
                        .find(|c| c["modelId"] == model)
                        .and_then(|c| c["result"].as_str().map(String::from))
                };
                println!("CHECKS {}", serde_json::to_string_pretty(&checks)?);
                anyhow::ensure!(
                    result_of("claude-sonnet-5").as_deref() == Some("refused"),
                    "claude-sonnet-5 must be refused: {checks:?}"
                );
                anyhow::ensure!(
                    result_of("claude-sonnet-4-6").as_deref() == Some("verified"),
                    "claude-sonnet-4-6 must be verified: {checks:?}"
                );
                return Ok(());
            }
            if resp.status.ends_with("failed") {
                anyhow::bail!("deployment failed: {}", resp.status);
            }
            anyhow::ensure!(start.elapsed() < Duration::from_secs(900), "timed out");
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
    .await;

    let destroy = deployment.destroy().await;
    println!("destroy requested: {:?}", destroy.is_ok());
    let cleanup =
        alien_test::setup::teardown_target(&config, Platform::Aws, &deployment.id, &manager).await;
    println!(
        "teardown outcome: {:?}",
        cleanup.as_ref().map(|_| ()).map_err(|e| e.to_string())
    );
    outcome.expect("ai models proof");
    cleanup.expect("teardown");
}
