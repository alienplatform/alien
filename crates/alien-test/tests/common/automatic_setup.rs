//! Real-cloud updates with a single initial setup and the installed management identity.

use alien_core::{Platform, ResourceLifecycle, Stack, Storage};
use alien_test::{TestConfig, TestDeployment, TestManager};
use anyhow::{bail, Context};
use std::{sync::Arc, time::Duration};

pub struct AutomaticSetup {
    config: TestConfig,
    manager: Arc<TestManager>,
    deployment: TestDeployment,
    platform: Platform,
}

impl AutomaticSetup {
    pub async fn new(platform: Platform) -> anyhow::Result<Self> {
        alien_test::e2e::init_tracing();
        let config = TestConfig::from_env();
        anyhow::ensure!(
            config.has_platform(platform),
            "Cloud credentials are required"
        );
        let manager = Arc::new(
            TestManager::start_with_config(&config, &[platform])
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );
        // This test must exercise the runtime's installed identity. An administrator fallback
        // would make a missing grant invisible, especially in local Azure runs.
        anyhow::ensure!(
            manager.management_config().is_some(),
            "A remote management identity is required; target-static mode cannot prove this flow"
        );
        let initial = Stack::new("automatic-setup".into()).build();
        publish(&manager, platform, &initial).await?;
        let group = manager
            .client()
            .create_deployment_group()
            .body(alien_manager_api::types::CreateDeploymentGroupRequest {
                name: "automatic-setup".into(),
                max_deployments: None,
                environment_variables: Default::default(),
                input_values: Default::default(),
            })
            .send()
            .await?;
        let response = manager
            .client()
            .create_deployment()
            .body(alien_manager_api::types::CreateDeploymentRequest {
                base_platform: None,
                pinned_release_id: None,
                setup_handoff: Default::default(),
                setup_method: None,
                name: format!("automatic-{}", uuid::Uuid::new_v4().simple()),
                platform: serde_json::from_value(serde_json::json!(platform))?,
                deployment_group_id: Some(group.id.clone()),
                stack_settings: Some(serde_json::from_value(
                    serde_json::json!({ "setupManagement": "automatic", "telemetry": "off" }),
                )?),
                environment_variables: None,
                resource_prefix: Some(alien_test::config::e2e_resource_prefix()?),
                input_values: Default::default(),
            })
            .send()
            .await?;
        let deployment = TestDeployment::new(
            response.deployment.id.clone(),
            response.deployment.name.clone(),
            platform.as_str().into(),
            None,
            response
                .token
                .clone()
                .context("Deployment token required")?,
            manager.clone(),
        );
        Ok(Self {
            config,
            manager,
            deployment,
            platform,
        })
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        alien_test::setup_target(
            &self.config,
            self.platform,
            &self.deployment,
            &Stack::new("automatic-setup".into()).build(),
            &self.manager,
            self.manager.management_config(),
        )
        .await?;
        self.deployment
            .wait_until_running(Duration::from_secs(900))
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let installed = self
            .manager
            .client()
            .get_deployment()
            .id(&self.deployment.id)
            .send()
            .await?;
        let metadata = serde_json::to_value(&installed.runtime_metadata)?;
        anyhow::ensure!(
            metadata["automaticSetup"] == true,
            "Setup must persist its grant"
        );

        verify_updates(&self.manager, &self.deployment, self.platform).await
    }

    pub async fn cleanup(self) -> anyhow::Result<()> {
        self.deployment
            .destroy()
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        alien_test::teardown_target(
            &self.config,
            self.platform,
            &self.deployment.id,
            &self.manager,
        )
        .await
    }
}

/// Exercise later frozen-resource changes using only the identity installed at setup.
pub async fn verify_updates(
    manager: &Arc<TestManager>,
    deployment: &TestDeployment,
    platform: Platform,
) -> anyhow::Result<()> {
    // No further setup calls occur. A resource type absent from the first release must be
    // provisioned with the installed management grant, then updated and removed by runtime.
    for (name, stack) in [
        ("add", storage_stack(platform, false)),
        ("change", storage_stack(platform, true)),
        ("remove", Stack::new("automatic-setup".into()).build()),
    ] {
        let release = publish(manager, platform, &stack).await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(900);
        loop {
            let state = manager
                .client()
                .get_deployment()
                .id(&deployment.id)
                .send()
                .await?;
            if state.status.ends_with("-failed") || state.status == "error" {
                bail!("{name} failed: {:?}", state.error);
            }
            if state.status == "running" && state.current_release_id.as_deref() == Some(&release) {
                let state = serde_json::to_value(
                    state.stack_state.as_ref().context("Stack state required")?,
                )?;
                anyhow::ensure!(
                    state["resources"]["management"]["status"] == "running",
                    "Automatic updates must retain the installed management identity"
                );
                if name == "remove" {
                    for id in ["archive", "archive-next"] {
                        let resource = &state["resources"][id];
                        anyhow::ensure!(
                            resource.is_null() || resource["status"] == "deleted",
                            "Removed storage must finish deletion before release promotion"
                        );
                    }
                } else {
                    let storage_id = if platform == Platform::Azure && name == "change" {
                        let previous = &state["resources"]["archive"];
                        anyhow::ensure!(
                            previous.is_null() || previous["status"] == "deleted",
                            "Azure replacement must delete the previous container"
                        );
                        "archive-next"
                    } else {
                        "archive"
                    };
                    anyhow::ensure!(
                        state["resources"][storage_id]["status"] == "running",
                        "Storage must reach running"
                    );
                    anyhow::ensure!(
                        state["resources"][storage_id]["config"][if platform == Platform::Gcp {
                            "versioning"
                        } else {
                            "publicRead"
                        }] == (name == "change" && platform != Platform::Azure),
                        "The installed configuration must converge"
                    );
                }
                tracing::info!(
                    phase = name,
                    "Automatic infrastructure update completed with installed credentials"
                );
                break;
            }
            anyhow::ensure!(
                tokio::time::Instant::now() < deadline,
                "{name} did not converge"
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
    Ok(())
}

fn storage_stack(platform: Platform, changed: bool) -> Stack {
    // Exercise a real supported change without relaxing provider restrictions.
    // GCP changes bucket versioning under domain-restricted IAM policy. Azure
    // replaces a private container because its storage account disallows public
    // access; ignored versioning/lifecycle fields would not prove a cloud change.
    let storage = match platform {
        Platform::Gcp => Storage::new("archive".into()).versioning(changed).build(),
        Platform::Azure => {
            Storage::new(if changed { "archive-next" } else { "archive" }.into()).build()
        }
        _ => Storage::new("archive".into()).public_read(changed).build(),
    };
    Stack::new("automatic-setup".into())
        .add(storage, ResourceLifecycle::Frozen)
        .build()
}

async fn publish(
    manager: &TestManager,
    platform: Platform,
    stack: &Stack,
) -> anyhow::Result<String> {
    Ok(manager
        .client()
        .create_release()
        .body(alien_manager_api::types::CreateReleaseRequest {
            project_id: "default".into(),
            channel: None,
            git_metadata: None,
            stack: serde_json::from_value(serde_json::json!({ (platform.as_str()): stack }))?,
        })
        .send()
        .await?
        .id
        .clone())
}
