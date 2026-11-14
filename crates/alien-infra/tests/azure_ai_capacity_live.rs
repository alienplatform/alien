//! Opt-in Azure test: the AI controller caps a model deployment to the subscription's free quota.
//!
//! Set `ALIEN_AZURE_AI_CAPACITY_LIVE_TEST=1`, the usual `AZURE_SUBSCRIPTION_ID`, `AZURE_TENANT_ID`,
//! `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_REGION`, and
//! `ALIEN_AZURE_AI_CAPACITY_LIVE_GROUP` (an existing, disposable resource group). Run with
//! `cargo test -p alien-infra --features all-platforms,test-utils --test azure_ai_capacity_live -- --ignored --nocapture`.
//!
//! A second account holds most of the `gpt-4.1` quota, so only 25 units remain free. The
//! controller then must create `gpt-4.1` with 25 units and every other model with its catalog
//! capacity. Both accounts are deleted at the end.
#![cfg(all(feature = "azure", feature = "test-utils"))]

use futures_util::FutureExt;
use std::sync::Arc;

use alien_azure_clients::azure::cognitive_services::*;
use alien_azure_clients::{AzureClientConfig, AzureClientConfigExt};
use alien_core::{Ai, ClientConfig, Platform, ResourceStatus};
use alien_infra::{
    controller_test::SingleControllerExecutor, AzureAiController, AzureResourceGroupController,
    DefaultPlatformServiceProvider, PlatformServiceProvider,
};

#[tokio::test]
#[ignore = "creates real AI Foundry accounts and model deployments; see module docs"]
async fn a_deployment_is_capped_to_the_free_quota_on_real_azure() {
    assert_eq!(
        std::env::var("ALIEN_AZURE_AI_CAPACITY_LIVE_TEST").as_deref(),
        Ok("1")
    );
    let group = std::env::var("ALIEN_AZURE_AI_CAPACITY_LIVE_GROUP").unwrap();
    let config = AzureClientConfig::from_std_env().await.unwrap();
    let location = config.region.clone().unwrap();
    let provider = DefaultPlatformServiceProvider::default();
    let client = provider.get_azure_cognitive_services_client(&config).unwrap();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let prefix = format!("capl{}", &suffix[..6]);
    let holder = format!("{prefix}-holder");

    let free_before = |usages: Vec<CognitiveServicesUsage>| {
        usages
            .into_iter()
            .find(|u| u.name.value.as_deref() == Some("OpenAI.GlobalStandard.gpt4.1"))
            .map(|u| u.limit - u.current_value)
            .unwrap()
    };
    let before = free_before(client.list_usages(&location).await.unwrap());
    eprintln!("gpt4.1 free units before: {before}");

    let result = std::panic::AssertUnwindSafe(async {
        // A second account takes all but 25 units of the gpt-4.1 quota.
        client
            .create_account(
                &group,
                &holder,
                &CognitiveServicesAccountCreateParameters {
                    location: location.clone(),
                    kind: "AIServices".to_string(),
                    sku: CognitiveServicesSku { name: "S0".to_string() },
                    properties: CognitiveServicesAccountCreateProperties {
                        custom_sub_domain_name: holder.clone(),
                    },
                },
            )
            .await
            .unwrap();
        for _ in 0..60 {
            let state = client
                .get_account(&group, &holder)
                .await
                .ok()
                .and_then(|a| a.properties)
                .and_then(|p| p.provisioning_state);
            if state.as_deref() == Some("Succeeded") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
        let hold = (before as i32) - 25;
        client
            .create_deployment(
                &group,
                &holder,
                "gpt-4.1",
                &CognitiveServicesDeploymentCreateParameters {
                    sku: CognitiveServicesDeploymentSku {
                        name: "GlobalStandard".to_string(),
                        capacity: hold,
                    },
                    properties: CognitiveServicesDeploymentCreateProperties {
                        model: CognitiveServicesDeploymentModel {
                            format: "OpenAI".to_string(),
                            name: "gpt-4.1".to_string(),
                            version: "2025-04-14".to_string(),
                        },
                    },
                },
            )
            .await
            .unwrap();
        let free = free_before(client.list_usages(&location).await.unwrap());
        eprintln!("holder deployment {hold} units; gpt4.1 free now: {free}");

        let mut run = SingleControllerExecutor::builder()
            .resource(Ai::new("live-ai".to_string()).build())
            .resource_prefix(&prefix)
            .controller(AzureAiController::default())
            .platform(Platform::Azure)
            .client_config(ClientConfig::Azure(Box::new(config.clone())))
            .real_delays()
            .with_dependency(
                alien_core::AzureResourceGroup::new("default-resource-group".to_string()).build(),
                AzureResourceGroupController::mock_ready(&group),
            )
            .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
            .build()
            .await
            .unwrap();
        for _ in 0..200 {
            if run.status() == ResourceStatus::Running {
                break;
            }
            run.step().await.unwrap();
        }
        assert_eq!(run.status(), ResourceStatus::Running);
        run.step().await.unwrap();

        let account = format!("{prefix}-live-ai");
        let mut seen = std::collections::BTreeMap::new();
        for d in client.list_deployments(&group, &account).await.unwrap() {
            let capacity = d.sku.as_ref().map(|s| s.capacity);
            eprintln!("DEPLOYED {:?} capacity {:?}", d.name, capacity);
            seen.insert(d.name.unwrap(), capacity.unwrap());
        }
        eprintln!(
            "HEARTBEAT {:?}",
            run.last_heartbeats().last().map(|h| format!("{:?}", h.data))
        );
        assert_eq!(seen["gpt-4.1"], 25, "capped to the free quota");
        assert_eq!(seen["model-router"], 50);
    })
    .catch_unwind()
    .await;

    for name in [format!("{prefix}-live-ai"), holder.clone()] {
        eprintln!(
            "delete account {name}: {:?}",
            client.delete_account(&group, &name).await
        );
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
