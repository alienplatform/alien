use alien_azure_clients::long_running_operation::LongRunningOperation;
use alien_azure_clients::models::managed_environments_dapr_components::DaprComponent;
use alien_core::{ResourceRef, Worker};
use alien_error::AlienError;

use super::azure::AzureWorkerController;
use super::azure_dapr_components::{
    delete_dapr_component_if_owned, ensure_dapr_component, get_dapr_component_ownership,
    service_bus_dapr_component, DaprComponentDeleteOperation, DaprComponentEnsureOperation,
    DaprComponentOwnership, TrackedDaprComponentDeleteStep,
};
use super::azure_names::{
    commands_queue_name, get_azure_blob_trigger_dapr_component_name, get_azure_dapr_component_name,
    get_azure_internal_commands_dapr_component_name, get_azure_queue_trigger_dapr_component_name,
    get_legacy_azure_blob_trigger_dapr_component_names,
    get_legacy_azure_internal_commands_dapr_component_names,
    get_legacy_azure_queue_trigger_dapr_component_names, storage_trigger_queue_name,
};
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils::get_container_apps_environment_outputs;

pub(super) const CURRENT_DAPR_COMPONENT_NAMING_VERSION: u8 = 1;

pub(super) enum DaprComponentMigrationStep {
    Complete,
    Mutated,
    LongRunning {
        operation: LongRunningOperation,
        deleted_component: Option<String>,
    },
}

enum MigrationAction {
    EnsureCommands {
        component: DaprComponent,
        legacy_names: Vec<String>,
    },
    RemoveCommands {
        names: Vec<String>,
    },
    EnsureTrigger {
        component: DaprComponent,
        legacy_names: Vec<String>,
    },
    KeepTrigger {
        name: String,
    },
}

fn push_unique(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

pub(super) fn commands_component_removal_names(
    container_app_name: &str,
    tracked_component_name: Option<&str>,
) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(component_name) = tracked_component_name {
        push_unique(&mut names, component_name.to_string());
    }
    push_unique(
        &mut names,
        get_azure_internal_commands_dapr_component_name(container_app_name),
    );
    for legacy_name in get_legacy_azure_internal_commands_dapr_component_names(container_app_name) {
        push_unique(&mut names, legacy_name);
    }
    names
}

fn append_trigger_dapr_component_deletion_candidates(
    names: &mut Vec<String>,
    worker: &Worker,
    container_app_name: &str,
) {
    let mut cron_index = 0usize;
    for trigger in &worker.triggers {
        match trigger {
            alien_core::WorkerTrigger::Queue { queue } => {
                push_unique(
                    names,
                    get_azure_queue_trigger_dapr_component_name(container_app_name, &queue.id),
                );
                for legacy_name in get_legacy_azure_queue_trigger_dapr_component_names(
                    container_app_name,
                    &queue.id,
                ) {
                    push_unique(names, legacy_name);
                }
            }
            alien_core::WorkerTrigger::Storage { storage, .. } => {
                push_unique(
                    names,
                    get_azure_blob_trigger_dapr_component_name(container_app_name, &storage.id),
                );
                for legacy_name in get_legacy_azure_blob_trigger_dapr_component_names(
                    container_app_name,
                    &storage.id,
                ) {
                    push_unique(names, legacy_name);
                }
            }
            alien_core::WorkerTrigger::Schedule { .. } => {
                push_unique(
                    names,
                    get_azure_dapr_component_name(&format!(
                        "cron-{container_app_name}-{cron_index}"
                    )),
                );
                cron_index += 1;
            }
        }
    }
}

fn dapr_component_deletion_candidates(
    worker: &Worker,
    container_app_name: &str,
    tracked_trigger_components: &[String],
    tracked_commands_component: Option<&str>,
) -> Vec<String> {
    let mut names = tracked_trigger_components.to_vec();
    if let Some(component_name) = tracked_commands_component {
        push_unique(&mut names, component_name.to_string());
    }

    push_unique(
        &mut names,
        get_azure_internal_commands_dapr_component_name(container_app_name),
    );
    for legacy_name in get_legacy_azure_internal_commands_dapr_component_names(container_app_name) {
        push_unique(&mut names, legacy_name);
    }

    append_trigger_dapr_component_deletion_candidates(&mut names, worker, container_app_name);

    names
}

impl AzureWorkerController {
    pub(super) fn initialize_dapr_component_deletion_candidates(
        &mut self,
        worker: &Worker,
        container_app_name: &str,
    ) -> bool {
        if self.dapr_component_deletion_candidates_initialized {
            return false;
        }

        self.dapr_components = dapr_component_deletion_candidates(
            worker,
            container_app_name,
            &self.dapr_components,
            self.commands_dapr_component.as_deref(),
        );
        self.dapr_component_deletion_candidates_initialized = true;
        true
    }

    pub(super) fn initialize_trigger_update_teardown_candidates(
        &mut self,
        previous_worker: &Worker,
        container_app_name: &str,
    ) {
        let mut names = self.dapr_components.clone();
        append_trigger_dapr_component_deletion_candidates(
            &mut names,
            previous_worker,
            container_app_name,
        );
        self.dapr_components = names;
    }

    fn default_service_bus_namespace_name(
        ctx: &ResourceControllerContext<'_>,
        worker_id: &str,
    ) -> Result<String> {
        let namespace_ref = ResourceRef::new(
            alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
            "default-service-bus-namespace",
        );
        let namespace = ctx.require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)?;
        namespace.namespace_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_id.to_string(),
                dependency_id: namespace_ref.id,
            })
        })
    }

    fn worker_execution_client_id(
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
    ) -> Result<String> {
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            format!("{}-sa", worker.get_permissions()),
        );
        let service_account = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )?;
        service_account.identity_client_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker.id.clone(),
                dependency_id: service_account_ref.id,
            })
        })
    }

    fn dapr_component_migration_plan(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        container_app_name: &str,
    ) -> Result<Vec<MigrationAction>> {
        let needs_worker_identity = worker.commands_enabled
            || worker.triggers.iter().any(|trigger| {
                matches!(
                    trigger,
                    alien_core::WorkerTrigger::Queue { .. }
                        | alien_core::WorkerTrigger::Storage { .. }
                )
            });
        let azure_client_id = needs_worker_identity
            .then(|| Self::worker_execution_client_id(ctx, worker))
            .transpose()?;
        let needs_default_namespace = worker.commands_enabled
            || worker
                .triggers
                .iter()
                .any(|trigger| matches!(trigger, alien_core::WorkerTrigger::Storage { .. }));
        let default_namespace = needs_default_namespace
            .then(|| Self::default_service_bus_namespace_name(ctx, &worker.id))
            .transpose()?;
        let resolved_worker_execution_client_id = || {
            azure_client_id.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "Dapr migration plan requires a worker execution identity".to_string(),
                    operation: Some("build_dapr_component_migration_plan".to_string()),
                    resource_id: Some(worker.id.clone()),
                })
            })
        };
        let mut plan = Vec::new();

        if worker.commands_enabled {
            let component_name =
                get_azure_internal_commands_dapr_component_name(container_app_name);
            let namespace_name = default_namespace.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker.id.clone(),
                    dependency_id: "default-service-bus-namespace".to_string(),
                })
            })?;
            let mut legacy_names =
                get_legacy_azure_internal_commands_dapr_component_names(container_app_name);
            if let Some(persisted_name) = &self.commands_dapr_component {
                push_unique(&mut legacy_names, persisted_name.clone());
            }
            plan.push(MigrationAction::EnsureCommands {
                component: service_bus_dapr_component(
                    component_name,
                    container_app_name,
                    namespace_name,
                    commands_queue_name(container_app_name),
                    resolved_worker_execution_client_id()?,
                ),
                legacy_names,
            });
        } else {
            let names = commands_component_removal_names(
                container_app_name,
                self.commands_dapr_component.as_deref(),
            );
            plan.push(MigrationAction::RemoveCommands { names });
        }

        for trigger in &worker.triggers {
            match trigger {
                alien_core::WorkerTrigger::Queue { queue } => {
                    let queue_controller =
                        ctx.require_dependency::<crate::queue::azure::AzureQueueController>(queue)?;
                    let namespace_name =
                        queue_controller.namespace_name.as_deref().ok_or_else(|| {
                            AlienError::new(ErrorData::DependencyNotReady {
                                resource_id: worker.id.clone(),
                                dependency_id: queue.id.clone(),
                            })
                        })?;
                    let queue_name = queue_controller.queue_name.clone().ok_or_else(|| {
                        AlienError::new(ErrorData::DependencyNotReady {
                            resource_id: worker.id.clone(),
                            dependency_id: queue.id.clone(),
                        })
                    })?;
                    let component_name =
                        get_azure_queue_trigger_dapr_component_name(container_app_name, &queue.id);
                    plan.push(MigrationAction::EnsureTrigger {
                        component: service_bus_dapr_component(
                            component_name,
                            container_app_name,
                            namespace_name,
                            queue_name,
                            resolved_worker_execution_client_id()?,
                        ),
                        legacy_names: get_legacy_azure_queue_trigger_dapr_component_names(
                            container_app_name,
                            &queue.id,
                        ),
                    });
                }
                alien_core::WorkerTrigger::Storage { storage, .. } => {
                    let component_name =
                        get_azure_blob_trigger_dapr_component_name(container_app_name, &storage.id);
                    let namespace_name = default_namespace.as_deref().ok_or_else(|| {
                        AlienError::new(ErrorData::DependencyNotReady {
                            resource_id: worker.id.clone(),
                            dependency_id: "default-service-bus-namespace".to_string(),
                        })
                    })?;
                    plan.push(MigrationAction::EnsureTrigger {
                        component: service_bus_dapr_component(
                            component_name,
                            container_app_name,
                            namespace_name,
                            storage_trigger_queue_name(container_app_name, &storage.id),
                            resolved_worker_execution_client_id()?,
                        ),
                        legacy_names: get_legacy_azure_blob_trigger_dapr_component_names(
                            container_app_name,
                            &storage.id,
                        ),
                    });
                }
                alien_core::WorkerTrigger::Schedule { .. } => {
                    let name = get_azure_dapr_component_name(&format!(
                        "cron-{container_app_name}-{}",
                        plan.iter()
                            .filter(|action| matches!(action, MigrationAction::KeepTrigger { .. }))
                            .count()
                    ));
                    plan.push(MigrationAction::KeepTrigger { name });
                }
            }
        }

        Ok(plan)
    }

    async fn reconcile_dapr_component(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        container_app_name: &str,
        component: &DaprComponent,
        legacy_names: &[String],
    ) -> Result<DaprComponentMigrationStep> {
        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_config)?;
        let desired_name = component.name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker.id.clone(),
                message: "Dapr migration component has no name".to_string(),
            })
        })?;
        if matches!(
            get_dapr_component_ownership(
                client.as_ref(),
                &env_outputs.resource_group_name,
                &env_outputs.environment_name,
                container_app_name,
                desired_name,
                &worker.id,
            )
            .await?,
            DaprComponentOwnership::Foreign
        ) {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: worker.id.clone(),
                message: format!(
                    "Dapr component '{desired_name}' is owned by another Container App"
                ),
            }));
        }
        for legacy_name in legacy_names {
            if legacy_name == desired_name {
                continue;
            }
            match delete_dapr_component_if_owned(
                client.as_ref(),
                &env_outputs.resource_group_name,
                &env_outputs.environment_name,
                container_app_name,
                legacy_name,
                &worker.id,
            )
            .await?
            {
                DaprComponentDeleteOperation::NotFound | DaprComponentDeleteOperation::Foreign => {}
                DaprComponentDeleteOperation::Completed => {
                    return Ok(DaprComponentMigrationStep::Mutated);
                }
                DaprComponentDeleteOperation::LongRunning(operation) => {
                    return Ok(DaprComponentMigrationStep::LongRunning {
                        operation,
                        deleted_component: None,
                    });
                }
            }
        }

        match ensure_dapr_component(
            client.as_ref(),
            &env_outputs.resource_group_name,
            &env_outputs.environment_name,
            container_app_name,
            component,
            &worker.id,
        )
        .await?
        {
            DaprComponentEnsureOperation::Unchanged => Ok(DaprComponentMigrationStep::Complete),
            DaprComponentEnsureOperation::Completed => Ok(DaprComponentMigrationStep::Mutated),
            DaprComponentEnsureOperation::LongRunning(operation) => {
                Ok(DaprComponentMigrationStep::LongRunning {
                    operation,
                    deleted_component: None,
                })
            }
        }
    }

    async fn remove_commands_dapr_components(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        container_app_name: &str,
        names: &[String],
    ) -> Result<DaprComponentMigrationStep> {
        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_config)?;
        for name in names {
            match delete_dapr_component_if_owned(
                client.as_ref(),
                &env_outputs.resource_group_name,
                &env_outputs.environment_name,
                container_app_name,
                name,
                &worker.id,
            )
            .await?
            {
                DaprComponentDeleteOperation::NotFound | DaprComponentDeleteOperation::Foreign => {}
                DaprComponentDeleteOperation::Completed => {
                    return Ok(DaprComponentMigrationStep::Mutated);
                }
                DaprComponentDeleteOperation::LongRunning(operation) => {
                    return Ok(DaprComponentMigrationStep::LongRunning {
                        operation,
                        deleted_component: None,
                    });
                }
            }
        }
        Ok(DaprComponentMigrationStep::Complete)
    }

    pub(super) async fn migrate_dapr_component_names(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<DaprComponentMigrationStep> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;
        let plan = self.dapr_component_migration_plan(ctx, worker, &container_app_name)?;
        let desired_trigger_names =
            plan.iter()
                .filter_map(|action| match action {
                    MigrationAction::EnsureTrigger { component, .. } => component.name.clone(),
                    MigrationAction::KeepTrigger { name } => Some(name.clone()),
                    MigrationAction::EnsureCommands { .. }
                    | MigrationAction::RemoveCommands { .. } => None,
                })
                .collect::<Vec<_>>();

        if let Some(component_name) = self
            .dapr_components
            .iter()
            .find(|name| !desired_trigger_names.contains(name))
            .cloned()
        {
            return match self
                .delete_tracked_dapr_component(
                    ctx,
                    &container_app_name,
                    &worker.id,
                    &component_name,
                )
                .await?
            {
                TrackedDaprComponentDeleteStep::Complete => {
                    unreachable!("a component was supplied")
                }
                TrackedDaprComponentDeleteStep::Mutated => {
                    self.dapr_components.retain(|name| name != &component_name);
                    Ok(DaprComponentMigrationStep::Mutated)
                }
                TrackedDaprComponentDeleteStep::LongRunning {
                    operation,
                    component_name,
                } => Ok(DaprComponentMigrationStep::LongRunning {
                    operation,
                    deleted_component: Some(component_name),
                }),
            };
        }

        for action in &plan {
            match action {
                MigrationAction::EnsureCommands {
                    component,
                    legacy_names,
                } => match self
                    .reconcile_dapr_component(
                        ctx,
                        worker,
                        &container_app_name,
                        component,
                        legacy_names,
                    )
                    .await?
                {
                    DaprComponentMigrationStep::Complete => {
                        if self.commands_dapr_component != component.name {
                            self.commands_dapr_component = component.name.clone();
                            return Ok(DaprComponentMigrationStep::Mutated);
                        }
                    }
                    step => return Ok(step),
                },
                MigrationAction::RemoveCommands { names } => match self
                    .remove_commands_dapr_components(ctx, worker, &container_app_name, names)
                    .await?
                {
                    DaprComponentMigrationStep::Complete => {
                        if self.commands_dapr_component.take().is_some() {
                            return Ok(DaprComponentMigrationStep::Mutated);
                        }
                    }
                    step => return Ok(step),
                },
                MigrationAction::EnsureTrigger {
                    component,
                    legacy_names,
                } => match self
                    .reconcile_dapr_component(
                        ctx,
                        worker,
                        &container_app_name,
                        component,
                        legacy_names,
                    )
                    .await?
                {
                    DaprComponentMigrationStep::Complete => {}
                    step => return Ok(step),
                },
                MigrationAction::KeepTrigger { .. } => {}
            }
        }

        if self.dapr_components != desired_trigger_names {
            self.dapr_components = desired_trigger_names;
            return Ok(DaprComponentMigrationStep::Mutated);
        }
        self.dapr_component_naming_version = CURRENT_DAPR_COMPONENT_NAMING_VERSION;
        Ok(DaprComponentMigrationStep::Complete)
    }

    pub(super) async fn delete_tracked_dapr_component(
        &self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_id: &str,
        component_name: &str,
    ) -> Result<TrackedDaprComponentDeleteStep> {
        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_config)?;
        match delete_dapr_component_if_owned(
            client.as_ref(),
            &env_outputs.resource_group_name,
            &env_outputs.environment_name,
            container_app_name,
            component_name,
            worker_id,
        )
        .await?
        {
            DaprComponentDeleteOperation::NotFound
            | DaprComponentDeleteOperation::Foreign
            | DaprComponentDeleteOperation::Completed => {
                Ok(TrackedDaprComponentDeleteStep::Mutated)
            }
            DaprComponentDeleteOperation::LongRunning(operation) => {
                Ok(TrackedDaprComponentDeleteStep::LongRunning {
                    operation,
                    component_name: component_name.to_string(),
                })
            }
        }
    }

    pub(super) fn complete_pending_dapr_component_deletion(&mut self) {
        if let Some(component_name) = self.pending_dapr_component_deletion_name.take() {
            self.dapr_components.retain(|name| name != &component_name);
        }
    }
}

#[cfg(test)]
#[path = "azure_dapr_names_migration_tests.rs"]
mod tests;
