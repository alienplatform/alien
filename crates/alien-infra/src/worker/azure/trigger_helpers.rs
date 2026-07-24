use super::*;

impl AzureWorkerController {
    pub(super) async fn create_dapr_service_bus_component(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_config: &alien_core::Worker,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<DaprComponentOperation> {
        let azure_config = ctx.get_azure_config()?;
        // Dapr components live on the Container Apps Environment, which may be in a
        // different resource group than the deployment (shared/external environments).
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        // Get queue controller to access Service Bus namespace
        let queue_controller =
            ctx.require_dependency::<crate::queue::azure::AzureQueueController>(queue_ref)?;
        let namespace = queue_controller.namespace_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;
        let component_name =
            get_azure_queue_trigger_dapr_component_name(container_app_name, &queue_ref.id);

        // Use Dapr input binding — the manager/user code sends directly to Service Bus
        // via Azure SDK, not through Dapr pubsub. Input bindings auto-deliver from the
        // named queue without requiring GET /dapr/subscribe subscriptions.
        let queue_name = queue_controller.queue_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;

        let service_account_id = format!("{}-sa", worker_config.get_permissions());
        let service_account_ref = alien_core::ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id,
        );
        let service_account = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )?;
        let client_id = service_account
            .identity_client_id
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id.clone(),
                    dependency_id: service_account_ref.id,
                })
            })?;
        let dapr_component = service_bus_dapr_component(
            component_name.clone(),
            container_app_name,
            namespace,
            queue_name.clone(),
            client_id,
        );

        info!(
            worker=%worker_config.id,
            queue=%queue_ref.id,
            component=%component_name,
            environment=%environment_name,
            "Creating Dapr Service Bus component"
        );

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        match delete_owned_legacy_dapr_components(
            client.as_ref(),
            &resource_group_name,
            &environment_name,
            container_app_name,
            &component_name,
            &get_legacy_azure_queue_trigger_dapr_component_names(container_app_name, &queue_ref.id),
            &worker_config.id,
        )
        .await?
        {
            LegacyDaprComponentCleanupStep::Complete => {}
            LegacyDaprComponentCleanupStep::Mutated => {
                return Ok(DaprComponentOperation::Pending(Duration::from_secs(1)));
            }
            LegacyDaprComponentCleanupStep::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|delay| delay.as_secs());
                return Ok(DaprComponentOperation::Deleting(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        match ensure_dapr_component(
            client.as_ref(),
            &resource_group_name,
            &environment_name,
            container_app_name,
            &dapr_component,
            &worker_config.id,
        )
        .await?
        {
            DaprComponentEnsureOperation::Unchanged | DaprComponentEnsureOperation::Completed => {}
            DaprComponentEnsureOperation::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::Creating(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        if !self.dapr_components.contains(&component_name) {
            self.dapr_components.push(component_name.clone());
        }

        info!(
            worker=%worker_config.id,
            component=%component_name,
            "Successfully created Dapr Service Bus component"
        );

        Ok(DaprComponentOperation::Completed)
    }

    /// Creates supported Azure storage-trigger delivery:
    /// Event Grid -> dedicated Service Bus queue -> Dapr Service Bus input binding.
    pub(super) async fn create_azure_storage_trigger(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_config: &alien_core::Worker,
        storage_ref: &alien_core::ResourceRef,
        events: &[String],
    ) -> Result<DaprComponentOperation> {
        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let environment_resource_group = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        let desired_target = self
            .desired_storage_trigger_target(ctx, worker_config, container_app_name, storage_ref)
            .await?;
        let desired_infrastructure = &desired_target.infrastructure;
        let event_subscription_name = desired_infrastructure.event_subscription_name.clone();
        let namespace_name = desired_infrastructure.namespace_name.clone();
        let queue_name = desired_infrastructure.queue_name.clone();

        let component_name =
            get_azure_blob_trigger_dapr_component_name(container_app_name, &storage_ref.id);

        if matches!(
            self.prepare_storage_trigger_target(ctx, desired_infrastructure)
                .await?,
            StorageTargetPreparation::Pending
        ) {
            return Ok(DaprComponentOperation::Pending(Duration::from_secs(1)));
        }
        match self
            .ensure_storage_delivery_infrastructure(
                ctx,
                worker_config,
                storage_ref,
                events,
                &desired_target,
            )
            .await?
        {
            StorageDeliveryReconcileResult::Complete => {}
            StorageDeliveryReconcileResult::Pending(delay) => {
                return Ok(DaprComponentOperation::Pending(delay));
            }
        }

        let dapr_component = service_bus_dapr_component(
            component_name.clone(),
            container_app_name,
            &namespace_name,
            queue_name.clone(),
            &desired_target.execution_client_id,
        );

        let container_apps_client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        match delete_owned_legacy_dapr_components(
            container_apps_client.as_ref(),
            &environment_resource_group,
            &environment_name,
            container_app_name,
            &component_name,
            &get_legacy_azure_blob_trigger_dapr_component_names(
                container_app_name,
                &storage_ref.id,
            ),
            &worker_config.id,
        )
        .await?
        {
            LegacyDaprComponentCleanupStep::Complete => {}
            LegacyDaprComponentCleanupStep::Mutated => {
                return Ok(DaprComponentOperation::Pending(Duration::from_secs(1)));
            }
            LegacyDaprComponentCleanupStep::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|delay| delay.as_secs());
                return Ok(DaprComponentOperation::Deleting(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        match ensure_dapr_component(
            container_apps_client.as_ref(),
            &environment_resource_group,
            &environment_name,
            container_app_name,
            &dapr_component,
            &worker_config.id,
        )
        .await?
        {
            DaprComponentEnsureOperation::Unchanged | DaprComponentEnsureOperation::Completed => {}
            DaprComponentEnsureOperation::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::Creating(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        if !self.dapr_components.contains(&component_name) {
            self.dapr_components.push(component_name.clone());
        }

        info!(
            worker=%worker_config.id,
            component=%component_name,
            subscription=%event_subscription_name,
            "Azure storage trigger delivery is ready"
        );

        Ok(DaprComponentOperation::Completed)
    }

    /// Creates a Dapr cron input binding for a schedule trigger
    pub(super) async fn create_dapr_cron_component(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_config: &alien_core::Worker,
        cron: &str,
        index: usize,
    ) -> Result<DaprComponentOperation> {
        use alien_azure_clients::models::managed_environments_dapr_components::{
            DaprComponent, DaprComponentProperties, DaprMetadata,
        };

        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        let component_name =
            get_azure_dapr_component_name(&format!("cron-{container_app_name}-{index}"));

        let dapr_component = DaprComponent {
            name: Some(component_name.clone()),
            properties: Some(DaprComponentProperties {
                component_type: Some("bindings.cron".to_string()),
                ignore_errors: false,
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata: vec![
                    DaprMetadata {
                        name: Some("schedule".into()),
                        value: Some(cron.to_string()),
                        secret_ref: None,
                    },
                    DaprMetadata {
                        name: Some("direction".into()),
                        value: Some("input".into()),
                        secret_ref: None,
                    },
                ],
                scopes: vec![container_app_name.to_string()],
                secret_store_component: None,
                secrets: vec![],
            }),
            id: None,
            system_data: None,
            type_: None,
        };

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        match ensure_dapr_component(
            client.as_ref(),
            &resource_group_name,
            &environment_name,
            container_app_name,
            &dapr_component,
            &worker_config.id,
        )
        .await?
        {
            DaprComponentEnsureOperation::Unchanged | DaprComponentEnsureOperation::Completed => {}
            DaprComponentEnsureOperation::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::Creating(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        if !self.dapr_components.contains(&component_name) {
            self.dapr_components.push(component_name.clone());
        }

        info!(
            worker=%worker_config.id,
            component=%component_name,
            schedule=%cron,
            "Successfully created Dapr cron component"
        );

        Ok(DaprComponentOperation::Completed)
    }

    /// Deletes tracked trigger components without touching a foreign component
    /// that happens to share a historical name.
    pub(super) async fn delete_all_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<TrackedDaprComponentDeleteStep> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let Some(component_name) = self.dapr_components.first().cloned() else {
            return Ok(TrackedDaprComponentDeleteStep::Complete);
        };
        let step = self
            .delete_tracked_dapr_component(
                ctx,
                &container_app_name,
                &worker_config.id,
                &component_name,
            )
            .await?;
        if matches!(step, TrackedDaprComponentDeleteStep::Mutated) {
            self.dapr_components.retain(|name| name != &component_name);
            if self.pending_dapr_component_deletion_name.as_deref() == Some(component_name.as_str())
            {
                self.pending_dapr_component_deletion_name = None;
            }
        }
        Ok(step)
    }
}
