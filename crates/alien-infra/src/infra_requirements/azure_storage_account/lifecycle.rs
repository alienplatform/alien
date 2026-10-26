//! Direct setup owns the account policy; container controllers only verify it.
use super::*;
use alien_azure_clients::models::storage::{ManagementPolicy, ManagementPolicyRule};
use sha2::{Digest, Sha256};

fn policy_error(ctx: &ResourceControllerContext<'_>, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SetupRerunRequired {
        resource_id: ctx.desired_config.id().to_owned(),
        message: message.to_owned(),
    })
}

fn desired_rules(ctx: &ResourceControllerContext<'_>) -> Result<Vec<ManagementPolicyRule>> {
    shared_capabilities(ctx)?
        .lifecycle
        .iter()
        .map(|(container, prefix, days)| {
            let identity = serde_json::to_vec(&(container, prefix, days))
                .into_alien_error()
                .context(cloud_error(ctx, "Encode lifecycle identity"))?;
            let name = format!("alien{:x}", Sha256::digest(identity));
            // Expiry was ignored by older direct setup. Creating an enabled rule would
            // retroactively delete data. The installer explicitly enables this exact
            // rule in Azure after reviewing its scope; unchanged rules retain that choice.
            serde_json::from_value(serde_json::json!({
                "name": name, "enabled": false, "type": "Lifecycle",
                "definition": {"filters": {"blobTypes": ["blockBlob"],
                    "prefixMatch": [format!("{container}/{prefix}")]},
                    "actions": {"baseBlob": {"delete": {"daysAfterModificationGreaterThan": days}}}}
            }))
            .into_alien_error()
            .context(cloud_error(ctx, "Build scoped lifecycle rule"))
        })
        .collect()
}

fn same_definition(
    ctx: &ResourceControllerContext<'_>,
    a: &ManagementPolicyRule,
    b: &ManagementPolicyRule,
) -> Result<bool> {
    // Compare the entire rule except activation, including version/snapshot actions.
    let mut left = a.clone();
    let mut right = b.clone();
    left.enabled = Some(false);
    right.enabled = Some(false);
    Ok(serde_json::to_value(left)
        .into_alien_error()
        .context(cloud_error(ctx, "Encode installed lifecycle rule"))?
        == serde_json::to_value(right)
            .into_alien_error()
            .context(cloud_error(ctx, "Encode desired lifecycle rule"))?)
}

impl AzureStorageAccountController {
    async fn read_lifecycle(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<ManagementPolicy>> {
        let client = ctx
            .service_provider
            .get_azure_blob_container_client(ctx.get_azure_config()?)?;
        match client
            .get_management_policy(
                &get_resource_group_name(ctx.state)?,
                self.required_account(ctx)?,
            )
            .await
        {
            Ok(policy) => {
                if policy.properties.is_none() {
                    return Err(policy_error(
                        ctx,
                        "Azure returned a lifecycle policy without rules",
                    ));
                }
                Ok(Some(policy))
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error.context(cloud_error(ctx, "Read shared lifecycle policy"))),
        }
    }

    fn merged_lifecycle(
        &self,
        ctx: &ResourceControllerContext<'_>,
        actual: Option<&ManagementPolicy>,
    ) -> Result<Vec<ManagementPolicyRule>> {
        let desired = desired_rules(ctx)?;
        let mut rules = actual
            .and_then(|p| p.properties.as_ref().map(|p| &p.policy))
            .map(|p| p.rules.clone())
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        for rule in &mut rules {
            if !seen.insert(rule.name.clone()) {
                return Err(policy_error(
                    ctx,
                    "Azure lifecycle policy contains duplicate rule names",
                ));
            }
            if self.lifecycle_rule_names.contains(&rule.name) {
                if let Some(want) = desired.iter().find(|want| want.name == rule.name) {
                    if !same_definition(ctx, want, rule)? {
                        rule.enabled = Some(false);
                    }
                } else {
                    // Keep a disabled tombstone: an Azure policy requires at least
                    // one rule, and deleting the policy does not cancel an active run.
                    rule.enabled = Some(false);
                }
            } else if desired.iter().any(|want| want.name == rule.name) {
                return Err(policy_error(
                    ctx,
                    "A lifecycle rule has this setup's name but no ownership receipt; reconcile it with the original setup before retrying",
                ));
            }
        }
        for want in desired {
            if let Some(rule) = rules.iter_mut().find(|rule| rule.name == want.name) {
                if !same_definition(ctx, rule, &want)? {
                    *rule = want;
                }
            } else {
                rules.push(want);
            }
        }
        if rules.len() > 100 {
            return Err(policy_error(
                ctx,
                "Combined Azure lifecycle policy exceeds 100 rules; remove obsolete disabled rules after their 24-hour propagation period before rerunning setup",
            ));
        }
        Ok(rules)
    }

    pub(super) async fn reconcile_lifecycle(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<bool> {
        require_direct_setup(ctx)?;
        let actual = self.read_lifecycle(ctx).await?;
        let rules = self.merged_lifecycle(ctx, actual.as_ref())?;
        let before = actual
            .as_ref()
            .and_then(|p| p.properties.as_ref().map(|p| &p.policy))
            .map(|p| &p.rules);
        let same = serde_json::to_value(before)
            .into_alien_error()
            .context(cloud_error(ctx, "Encode installed lifecycle rules"))?
            == serde_json::to_value(&rules)
                .into_alien_error()
                .context(cloud_error(ctx, "Encode desired lifecycle rules"))?;
        if same || (actual.is_none() && rules.is_empty()) {
            return Ok(true);
        }
        let desired_names: BTreeSet<_> = desired_rules(ctx)?
            .into_iter()
            .map(|rule| rule.name)
            .collect();
        if !desired_names.is_subset(&self.lifecycle_rule_names) {
            // Yield before PUT so the runner checkpoints ownership intent. A restart
            // after Azure commits can then recognize this setup's exact planned rules.
            self.lifecycle_rule_names.extend(desired_names);
            return Ok(false);
        }
        let policy: ManagementPolicy =
            serde_json::from_value(serde_json::json!({"properties":{"policy":{"rules":rules}}}))
                .into_alien_error()
                .context(cloud_error(ctx, "Build aggregate lifecycle policy"))?;
        let client = ctx
            .service_provider
            .get_azure_blob_container_client(ctx.get_azure_config()?)?;
        client
            .set_management_policy(
                &get_resource_group_name(ctx.state)?,
                self.required_account(ctx)?,
                &policy,
            )
            .await
            .context(cloud_error(ctx, "Set shared lifecycle policy"))?;
        Ok(true)
    }

    pub(super) async fn verify_lifecycle(&self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        require_direct_setup(ctx)?;
        let actual = self.read_lifecycle(ctx).await?;
        let expected = self.merged_lifecycle(ctx, actual.as_ref())?;
        let observed = actual
            .as_ref()
            .and_then(|p| p.properties.as_ref().map(|p| &p.policy))
            .map(|p| p.rules.as_slice())
            .unwrap_or_default();
        if serde_json::to_value(observed)
            .into_alien_error()
            .context(cloud_error(ctx, "Encode observed lifecycle rules"))?
            != serde_json::to_value(expected)
                .into_alien_error()
                .context(cloud_error(ctx, "Encode expected lifecycle rules"))?
        {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: ctx.desired_config.id().to_owned(),
                message: "Azure has not confirmed the shared lifecycle policy; rerun setup without enabling newly changed expiry rules".into(),
            }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
    use alien_core::{LifecycleRule, ResourceLifecycle};
    use std::sync::{Arc, Mutex};

    fn policy(rules: &[ManagementPolicyRule]) -> ManagementPolicy {
        serde_json::from_value(serde_json::json!({"properties":{"policy":{"rules":rules}}}))
            .unwrap()
    }

    #[tokio::test]
    async fn aggregate_preserves_activation_and_foreign_rules_but_disables_changed_owned_rules() {
        let run = SingleControllerExecutor::builder()
            .resource(AzureStorageAccount::new("default-storage-account".into()).build())
            .controller(AzureStorageAccountController::mock_ready("teststorage"))
            .platform(Platform::Azure)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .with_stack_resource(
                Storage::new("first".into())
                    .lifecycle_rules(vec![LifecycleRule {
                        days: 30,
                        prefix: None,
                    }])
                    .build(),
                ResourceLifecycle::Live,
            )
            .with_stack_resource(
                Storage::new("second".into())
                    .lifecycle_rules(vec![LifecycleRule {
                        days: 90,
                        prefix: Some("archive/".into()),
                    }])
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .with_dependency(
                alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                crate::infra_requirements::AzureResourceGroupController::mock_ready(
                    "default-resource-group",
                ),
            )
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .await
            .unwrap();
        run.with_context(|ctx| {
            let mut controller = AzureStorageAccountController::mock_ready("teststorage");
            let desired = controller.merged_lifecycle(ctx, None).unwrap();
            assert_eq!(desired.len(), 2);
            assert!(desired.iter().all(|rule| rule.enabled == Some(false)));
            let prefixes: BTreeSet<_> = desired
                .iter()
                .flat_map(|r| r.definition.filters.as_ref().unwrap().prefix_match.clone())
                .collect();
            assert!(prefixes.contains(&format!("{}-first/", ctx.resource_prefix)));
            assert!(prefixes.contains(&format!("{}-second/archive/", ctx.resource_prefix)));
            // A matching name is not proof of direct-setup ownership, even if content matches.
            assert!(controller
                .merged_lifecycle(ctx, Some(&policy(&desired)))
                .is_err());
            controller
                .lifecycle_rule_names
                .extend(desired.iter().map(|rule| rule.name.clone()));
            let mut actual = desired.clone();
            actual[0].enabled = None; // Azure defaults omitted enabled to true.
            actual[1].enabled = Some(true);
            let mut foreign = actual[0].clone();
            foreign.name = "customer-rule".into();
            actual.push(foreign.clone());
            let merged = controller
                .merged_lifecycle(ctx, Some(&policy(&actual)))
                .unwrap();
            assert_eq!(
                serde_json::to_value(&merged).unwrap(),
                serde_json::to_value(&actual).unwrap()
            );
            // A changed managed definition is restored disabled, never silently activated.
            actual[0].definition.filters.as_mut().unwrap().prefix_match = vec!["".into()];
            let mut obsolete = actual[1].clone();
            obsolete.name = "previous-owned-rule".into();
            controller
                .lifecycle_rule_names
                .insert(obsolete.name.clone());
            actual.push(obsolete);
            let repaired = controller
                .merged_lifecycle(ctx, Some(&policy(&actual)))
                .unwrap();
            assert_eq!(repaired[0].enabled, Some(false));
            assert!(same_definition(ctx, &repaired[0], &desired[0]).unwrap());
            assert_eq!(repaired[1].enabled, Some(true));
            assert_eq!(
                serde_json::to_value(&repaired[2]).unwrap(),
                serde_json::to_value(&foreign).unwrap()
            );
            assert_eq!(repaired[3].enabled, Some(false));
            let mut over_limit = vec![];
            for index in 0..99 {
                let mut rule = foreign.clone();
                rule.name = format!("foreign{index}");
                over_limit.push(rule);
            }
            assert!(controller
                .merged_lifecycle(ctx, Some(&policy(&over_limit)))
                .is_err());
            over_limit.pop();
            assert_eq!(
                controller
                    .merged_lifecycle(ctx, Some(&policy(&over_limit)))
                    .unwrap()
                    .len(),
                100
            );
            let duplicate = policy(&[foreign.clone(), foreign]);
            assert!(controller.merged_lifecycle(ctx, Some(&duplicate)).is_err());
        });
    }
    #[tokio::test]
    async fn ownership_is_checkpointed_before_a_policy_commit_and_survives_a_lost_response() {
        async fn executor(
            controller: AzureStorageAccountController,
            provider: Arc<MockPlatformServiceProvider>,
        ) -> SingleControllerExecutor {
            SingleControllerExecutor::builder()
                .resource(AzureStorageAccount::new("default-storage-account".into()).build())
                .resource_prefix("owned")
                .controller(controller)
                .platform(Platform::Azure)
                .initial_setup_authority(InitialSetupAuthority::DirectSetup)
                .with_stack_resource(
                    Storage::new("first".into())
                        .lifecycle_rules(vec![LifecycleRule {
                            days: 30,
                            prefix: None,
                        }])
                        .build(),
                    ResourceLifecycle::Live,
                )
                .with_dependency(
                    alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                    crate::infra_requirements::AzureResourceGroupController::mock_ready(
                        "default-resource-group",
                    ),
                )
                .service_provider(provider)
                .build()
                .await
                .unwrap()
        }
        let actual = Arc::new(Mutex::new(None::<ManagementPolicy>));
        let writes = Arc::new(Mutex::new(0));
        let mut api = alien_azure_clients::blob_containers::MockBlobContainerApi::new();
        let read = actual.clone();
        api.expect_get_management_policy()
            .times(4)
            .returning(move |_, _| {
                read.lock().unwrap().clone().ok_or_else(|| {
                    AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "policy".into(),
                        resource_name: "default".into(),
                    })
                })
            });
        let write = actual.clone();
        let count = writes.clone();
        api.expect_set_management_policy()
            .times(1)
            .returning(move |_, _, policy| {
                *count.lock().unwrap() += 1;
                *write.lock().unwrap() = Some(policy.clone());
                // Azure committed, but the client did not receive a successful response.
                Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "injected response".into(),
                    resource_name: "default".into(),
                }))
            });
        let api = Arc::new(api);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(api.clone()));
        let provider = Arc::new(provider);
        let mut controller = AzureStorageAccountController::mock_ready("teststorage");
        let probe = executor(controller.clone(), provider.clone()).await;
        controller.pending_capabilities_revision =
            Some(probe.with_context(|ctx| shared_capabilities(ctx).unwrap().revision()));
        controller.state = AzureStorageAccountState::ConfiguringLifecycle;
        let mut first = executor(controller, provider.clone()).await;
        first.step().await.unwrap();
        assert_eq!(
            *writes.lock().unwrap(),
            0,
            "first step must persist intent without PUT"
        );
        let checkpoint = serde_json::to_value(
            first
                .internal_state::<AzureStorageAccountController>()
                .unwrap(),
        )
        .unwrap();
        let saved: AzureStorageAccountController =
            serde_json::from_value(checkpoint.clone()).unwrap();
        assert_eq!(saved.state, AzureStorageAccountState::ConfiguringLifecycle);
        assert_eq!(saved.lifecycle_rule_names.len(), 1);
        let mut writing = executor(saved, provider.clone()).await;
        assert!(writing.step().await.is_err());
        assert_eq!(*writes.lock().unwrap(), 1);
        assert!(actual.lock().unwrap().is_some());
        // Reload only the checkpoint from BEFORE the write; no failed-step memory survives.
        let mut recovered = executor(serde_json::from_value(checkpoint).unwrap(), provider).await;
        recovered.step().await.unwrap();
        assert_eq!(
            recovered
                .internal_state::<AzureStorageAccountController>()
                .unwrap()
                .state,
            AzureStorageAccountState::VerifyingLifecycle
        );
        recovered.step().await.unwrap();
        assert!(recovered
            .internal_state::<AzureStorageAccountController>()
            .unwrap()
            .capabilities_revision
            .is_some());
        assert_eq!(
            *writes.lock().unwrap(),
            1,
            "recovery confirms the committed policy instead of adopting unreceipted rules or rewriting it"
        );
    }

    #[tokio::test]
    async fn saved_lifecycle_commands_and_readbacks_fail_closed_on_denied_reads() {
        for state in [
            AzureStorageAccountState::ConfiguringLifecycle,
            AzureStorageAccountState::UpdatingLifecycle,
            AzureStorageAccountState::VerifyingLifecycle,
            AzureStorageAccountState::VerifyingUpdatedLifecycle,
        ] {
            let mut blob = alien_azure_clients::blob_containers::MockBlobContainerApi::new();
            blob.expect_get_management_policy()
                .times(1)
                .returning(|_, _| {
                    Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                        resource_type: "policy".into(),
                        resource_name: "default".into(),
                    }))
                });
            blob.expect_set_management_policy().times(0);
            let blob = Arc::new(blob);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_azure_blob_container_client()
                .returning(move |_| Ok(blob.clone()));
            let mut controller = AzureStorageAccountController::mock_ready("teststorage");
            controller.state = state;
            controller.pending_capabilities_revision =
                Some(SharedCapabilities::default().revision());
            let mut run = SingleControllerExecutor::builder()
                .resource(AzureStorageAccount::new("default-storage-account".into()).build())
                .controller(
                    serde_json::from_value::<AzureStorageAccountController>(
                        serde_json::to_value(controller).unwrap(),
                    )
                    .unwrap(),
                )
                .platform(Platform::Azure)
                .initial_setup_authority(InitialSetupAuthority::DirectSetup)
                .with_dependency(
                    alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                    crate::infra_requirements::AzureResourceGroupController::mock_ready(
                        "default-resource-group",
                    ),
                )
                .service_provider(Arc::new(provider))
                .build()
                .await
                .unwrap();
            assert!(run.step().await.is_err());
            let saved = run
                .internal_state::<AzureStorageAccountController>()
                .unwrap();
            assert_eq!(saved.account_name.as_deref(), Some("teststorage"));
            assert!(saved.capabilities_revision.is_none());
            assert!(saved.lifecycle_rule_names.is_empty());
        }
    }
}
