//! Real-cloud updates with a single initial setup and the installed management identity.

use alien_aws_clients::iam::{IamApi, IamClient};
use alien_core::permissions::{ManagementPermissions, PermissionProfile};
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
        self.install().await?;
        verify_updates(&self.manager, &self.deployment, self.platform).await
    }

    /// A release that widens the management grant stops for setup, and the setup run
    /// applies it. A release that narrows it runs on the wider installed grant.
    pub async fn run_management_change(&mut self) -> anyhow::Result<()> {
        self.install().await?;
        let installed_revision = self.management_revision().await?;
        let stuck = if self.platform == Platform::Aws {
            Some(self.leave_role_at_the_quota().await?)
        } else {
            None
        };

        let wider = publish(&self.manager, self.platform, &management_stack(true)).await?;
        let error = self.wait_for_setup_required(&wider).await?;
        tracing::info!(%error, "Wider management grant stopped for setup");
        anyhow::ensure!(
            self.management_revision().await? == installed_revision,
            "The runtime must not change its own management grant"
        );

        self.setup().await?;
        self.wait_until_running_release(&wider).await?;
        let applied_revision = self.management_revision().await?;
        anyhow::ensure!(
            applied_revision != installed_revision,
            "The setup run must apply the wider management grant"
        );
        tracing::info!("Setup run applied the wider management grant");
        if let Some(stuck) = &stuck {
            stuck.verify_repaired().await?;
        }

        let narrower = publish(&self.manager, self.platform, &management_stack(false)).await?;
        self.wait_until_running_release(&narrower).await?;
        anyhow::ensure!(
            self.management_revision().await? == applied_revision,
            "Narrowing keeps the wider grant until the next setup run"
        );
        tracing::info!("Narrower management grant ran without setup");
        Ok(())
    }

    async fn install(&mut self) -> anyhow::Result<()> {
        self.setup().await?;
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
        Ok(())
    }

    /// Runs setup the way a deployer reruns it: it applies a pending or stopped update, and
    /// leaves a settled deployment alone.
    async fn setup(&self) -> anyhow::Result<()> {
        alien_test::setup_target(
            &self.config,
            self.platform,
            &self.deployment,
            &Stack::new("automatic-setup".into()).build(),
            &self.manager,
            self.manager.management_config(),
        )
        .await
    }

    async fn deployment_state(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::to_value(
            self.manager
                .client()
                .get_deployment()
                .id(&self.deployment.id)
                .send()
                .await?
                .into_inner(),
        )?)
    }

    /// The management permissions revision the management controller last applied.
    async fn management_revision(&self) -> anyhow::Result<String> {
        let state = self.deployment_state().await?;
        let management = &state["stackState"]["resources"]["management"];
        anyhow::ensure!(
            management["status"] == "running",
            "The management identity must be running: {management}"
        );
        management["_internal"]["managementPermissionsRevision"]
            .as_str()
            .map(str::to_owned)
            .with_context(|| format!("No applied management revision: {management}"))
    }

    /// Leaves the management role as a setup that hit the AWS quota of 20 attached
    /// managed policies did: 22 numbered policies exist and the first 20 are attached,
    /// so the policies that hold the role's Deny statements past index 19 are not.
    async fn leave_role_at_the_quota(&self) -> anyhow::Result<StuckRole> {
        let state = self.deployment_state().await?;
        let management = &state["stackState"]["resources"]["management"]["_internal"];
        let role = management["roleName"]
            .as_str()
            .context("The management role name is required")?
            .to_owned();
        let prefix = state["stackState"]["resourcePrefix"]
            .as_str()
            .context("The resource prefix is required")?
            .to_owned();
        let account = management["roleArn"]
            .as_str()
            .and_then(|arn| arn.split(':').nth(4))
            .context("The management role ARN names the account")?
            .to_owned();
        let stuck = StuckRole {
            iam: super::sandbox::target_iam().await?,
            role,
            prefix,
            account,
        };
        let packed = stuck.attached().await?.len();
        let filler = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"iam:GetRole","Resource":"*"}]}"#;
        for index in packed..22 {
            let name = stuck.numbered(index);
            stuck
                .iam
                .create_policy(&name, filler, None, None)
                .await
                .with_context(|| format!("create {name}"))?;
            if index < 20 {
                stuck
                    .iam
                    .attach_role_policy(&stuck.role, &stuck.arn(&name))
                    .await
                    .with_context(|| format!("attach {name}"))?;
            }
        }
        anyhow::ensure!(
            stuck.attached().await?
                == (0..20)
                    .map(|index| stuck.numbered(index))
                    .collect::<Vec<_>>(),
            "The role must be left at the quota"
        );
        tracing::info!(
            role = %stuck.role,
            packed,
            "Seeded 22 numbered management policies with 20 attached"
        );
        Ok(stuck)
    }

    async fn wait_for_setup_required(&self, release: &str) -> anyhow::Result<String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
        loop {
            let state = self.deployment_state().await?;
            if state["status"] == "running" && state["currentReleaseId"] == release {
                bail!("The release went running without setup");
            }
            if state["status"] == "update-failed" {
                // Preflight failures wrap the setup request, so look through the sources.
                let setup_required =
                    std::iter::successors(Some(&state["error"]), |error| error.get("source"))
                        .find(|error| error["code"] == "DEPLOYMENT_SETUP_REQUIRED");
                let Some(setup_required) = setup_required else {
                    bail!("Expected the release to stop for setup: {}", state["error"]);
                };
                return Ok(setup_required["message"].to_string());
            }
            if state["status"]
                .as_str()
                .is_some_and(|status| status.ends_with("-failed"))
            {
                bail!(
                    "The release failed without asking for setup: {} {}",
                    state["status"],
                    state["error"]
                );
            }
            anyhow::ensure!(
                tokio::time::Instant::now() < deadline,
                "The release neither stopped for setup nor ran: {}",
                state["status"]
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn wait_until_running_release(&self, release: &str) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(900);
        loop {
            let state = self.deployment_state().await?;
            let status = state["status"].as_str().unwrap_or_default();
            if status.ends_with("-failed") || status == "error" {
                bail!("Release {release} failed: {}", state["error"]);
            }
            if status == "running" && state["currentReleaseId"] == release {
                return Ok(());
            }
            anyhow::ensure!(
                tokio::time::Instant::now() < deadline,
                "Release {release} did not converge: {status}"
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
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

/// A management role left at the AWS attached-policy quota.
struct StuckRole {
    iam: IamClient,
    role: String,
    prefix: String,
    account: String,
}

impl StuckRole {
    fn numbered(&self, index: usize) -> String {
        format!("{}-deployment-management-{index}", self.prefix)
    }

    fn arn(&self, name: &str) -> String {
        format!("arn:aws:iam::{}:policy/{name}", self.account)
    }

    /// Names of the managed policies attached to the role, in index order.
    async fn attached(&self) -> anyhow::Result<Vec<String>> {
        let mut names: Vec<String> = self
            .iam
            .list_attached_role_policies(&self.role)
            .await?
            .list_attached_role_policies_result
            .attached_policies
            .map(|attached| attached.member)
            .unwrap_or_default()
            .into_iter()
            .map(|policy| policy.policy_name)
            .collect();
        names.sort_by_key(|name| {
            name.rsplit('-')
                .next()
                .and_then(|index| index.parse::<usize>().ok())
        });
        Ok(names)
    }

    /// The setup run packed the grant again: a contiguous run of numbered policies from 0,
    /// all attached, with nothing numbered left above it.
    async fn verify_repaired(&self) -> anyhow::Result<()> {
        let attached = self.attached().await?;
        let packed = attached.len();
        anyhow::ensure!(
            (1..20).contains(&packed),
            "The repaired role holds {packed} managed policies"
        );
        anyhow::ensure!(
            attached
                == (0..packed)
                    .map(|index| self.numbered(index))
                    .collect::<Vec<_>>(),
            "The repaired role must attach policies 0..{packed}: {attached:?}"
        );
        for index in packed..22 {
            match self.iam.get_policy(&self.arn(&self.numbered(index))).await {
                Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => {}
                Ok(_) => bail!("Stale management policy {index} was left behind"),
                Err(error) => bail!("GetPolicy {index} failed: {error}"),
            }
        }
        tracing::info!(role = %self.role, packed, "Setup run repaired the role left at the quota");
        Ok(())
    }
}

/// An empty stack whose management grant either adds reading application buckets or not.
fn management_stack(read_buckets: bool) -> Stack {
    let management = if read_buckets {
        ManagementPermissions::Extend(PermissionProfile::new().global(["storage/data-read"]))
    } else {
        ManagementPermissions::Auto
    };
    Stack::new("automatic-setup".into())
        .management(management)
        .build()
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
