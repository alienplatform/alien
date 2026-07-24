use alien_core::Worker;
use alien_error::{AlienError, Context};
use serde::{Deserialize, Serialize};

use super::{management_profile_dispatches_commands, AzureWorkerController};
use crate::core::{AzurePermissionsHelper, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use crate::worker::azure::role_assignments::discover_proven_role_assignments;
use crate::worker::azure_names::{commands_sender_role_assignment_name, service_bus_queue_scope};

const SERVICE_BUS_DATA_SENDER_ROLE_DEFINITION_GUID: &str = "69a216fc-b8fb-44d8-bc22-1f3c2cd27a39";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AzureCommandsSenderRoleAssignmentIntent {
    pub(crate) assignment_id: String,
    pub(crate) assignment_name: String,
    pub(crate) principal_id: String,
    pub(crate) resource_group_name: String,
    pub(crate) namespace_name: String,
    pub(crate) queue_name: String,
}

pub(super) enum CommandsSenderReconcileResult {
    Complete,
    Pending,
}

enum CommandsSenderDiscoveryResult {
    Complete { desired_found: bool },
    Mutated,
}

pub(super) fn commands_sender_role_definition_id(subscription_id: &str) -> String {
    format!(
        "/subscriptions/{subscription_id}/providers/Microsoft.Authorization/roleDefinitions/{SERVICE_BUS_DATA_SENDER_ROLE_DEFINITION_GUID}"
    )
}

impl AzureWorkerController {
    async fn desired_commands_sender_role_assignment(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
    ) -> Result<Option<AzureCommandsSenderRoleAssignmentIntent>> {
        if !worker.commands_enabled
            || !management_profile_dispatches_commands(ctx, &worker.id)
            || AzurePermissionsHelper::get_management_uami_principal_id(ctx)?.is_some()
        {
            return Ok(None);
        }

        let resource_group_name = self.commands_resource_group_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker.id.clone(),
                dependency_id: "commands-resource-group".to_string(),
            })
        })?;
        let namespace_name = self.commands_namespace_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker.id.clone(),
                dependency_id: "commands-namespace".to_string(),
            })
        })?;
        let queue_name = self.commands_queue_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker.id.clone(),
                dependency_id: "commands-queue".to_string(),
            })
        })?;
        let azure_config = ctx.get_azure_config()?;
        let principal_id = ctx
            .service_provider
            .get_azure_caller_principal_id(azure_config)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to resolve Azure command sender principal".to_string(),
                resource_id: Some(worker.id.clone()),
            })?;
        let queue_scope = service_bus_queue_scope(resource_group_name, namespace_name, queue_name);
        let assignment_name = commands_sender_role_assignment_name(
            ctx.resource_prefix,
            &worker.id,
            &principal_id,
            namespace_name,
            queue_name,
        );
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;
        let assignment_id =
            authorization_client.build_role_assignment_id(&queue_scope, assignment_name.clone());

        Ok(Some(AzureCommandsSenderRoleAssignmentIntent {
            assignment_id,
            assignment_name,
            principal_id,
            resource_group_name: resource_group_name.clone(),
            namespace_name: namespace_name.clone(),
            queue_name: queue_name.clone(),
        }))
    }

    async fn discover_commands_sender_role_assignments(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        desired: Option<&AzureCommandsSenderRoleAssignmentIntent>,
    ) -> Result<CommandsSenderDiscoveryResult> {
        let Some((resource_group_name, namespace_name, queue_name)) =
            self.commands_cleanup_target(&worker.id)?
        else {
            self.commands_sender_role_assignment_discovery_complete = true;
            return Ok(CommandsSenderDiscoveryResult::Complete {
                desired_found: false,
            });
        };
        let azure_config = ctx.get_azure_config()?;
        let queue_scope =
            service_bus_queue_scope(&resource_group_name, &namespace_name, &queue_name);
        let role_definition_id = commands_sender_role_definition_id(&azure_config.subscription_id);
        let assignments = discover_proven_role_assignments(
            ctx,
            &queue_scope,
            &role_definition_id,
            &worker.id,
            "command sender",
            |principal_id| {
                commands_sender_role_assignment_name(
                    ctx.resource_prefix,
                    &worker.id,
                    principal_id,
                    &namespace_name,
                    &queue_name,
                )
            },
        )
        .await?;

        let mut desired_found = false;
        for assignment in assignments {
            if desired
                .is_some_and(|desired| assignment.id.eq_ignore_ascii_case(&desired.assignment_id))
            {
                desired_found = true;
                continue;
            }

            Self::delete_commands_role_assignment(ctx, &assignment.id, "discovered sender").await?;
            self.commands_sender_role_assignment_discovery_complete = false;
            return Ok(CommandsSenderDiscoveryResult::Mutated);
        }

        self.commands_sender_role_assignment_discovery_complete = true;
        Ok(CommandsSenderDiscoveryResult::Complete { desired_found })
    }

    pub(super) async fn reconcile_commands_sender_role_assignment(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
    ) -> Result<CommandsSenderReconcileResult> {
        let desired = self
            .desired_commands_sender_role_assignment(ctx, worker)
            .await?;

        if let Some(applied_id) = self.commands_sender_role_assignment_id.clone() {
            if self
                .commands_sender_role_assignment_intent
                .as_ref()
                .is_some_and(|intent| intent.assignment_id == applied_id)
            {
                self.commands_sender_role_assignment_intent = None;
                return Ok(CommandsSenderReconcileResult::Pending);
            }
            if desired
                .as_ref()
                .is_some_and(|intent| intent.assignment_id == applied_id)
            {
                if self.commands_sender_role_assignment_discovery_complete {
                    return Ok(CommandsSenderReconcileResult::Complete);
                }
            } else {
                self.commands_sender_role_assignment_id = None;
                self.commands_sender_role_assignment_discovery_complete = false;
                return Ok(CommandsSenderReconcileResult::Pending);
            }
        }

        if let Some(planned) = self.commands_sender_role_assignment_intent.clone() {
            if desired.as_ref() != Some(&planned) {
                self.commands_sender_role_assignment_intent = None;
                self.commands_sender_role_assignment_discovery_complete = false;
                return Ok(CommandsSenderReconcileResult::Pending);
            }
        }

        if !self.commands_sender_role_assignment_discovery_complete {
            match self
                .discover_commands_sender_role_assignments(ctx, worker, desired.as_ref())
                .await?
            {
                CommandsSenderDiscoveryResult::Mutated => {
                    return Ok(CommandsSenderReconcileResult::Pending);
                }
                CommandsSenderDiscoveryResult::Complete { desired_found } => {
                    if let Some(desired) = desired.as_ref() {
                        if desired_found {
                            self.commands_sender_role_assignment_id =
                                Some(desired.assignment_id.clone());
                            self.commands_sender_role_assignment_intent = None;
                        } else if self
                            .commands_sender_role_assignment_id
                            .as_ref()
                            .is_some_and(|applied| applied == &desired.assignment_id)
                        {
                            self.commands_sender_role_assignment_id = None;
                        }
                    }
                    return Ok(CommandsSenderReconcileResult::Pending);
                }
            }
        }

        if let Some(planned) = self.commands_sender_role_assignment_intent.clone() {
            let azure_config = ctx.get_azure_config()?;
            let authorization_client = ctx
                .service_provider
                .get_azure_authorization_client(azure_config)?;
            let queue_scope = service_bus_queue_scope(
                &planned.resource_group_name,
                &planned.namespace_name,
                &planned.queue_name,
            );
            let role_definition_id =
                commands_sender_role_definition_id(&azure_config.subscription_id);
            AzurePermissionsHelper::create_role_assignment(
                &authorization_client,
                azure_config,
                &queue_scope,
                &planned.assignment_name,
                &planned.principal_id,
                &role_definition_id,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to grant Azure command sender role".to_string(),
                resource_id: Some(worker.id.clone()),
            })?;
            self.commands_sender_role_assignment_id = Some(planned.assignment_id);
            self.commands_sender_role_assignment_intent = None;
            return Ok(CommandsSenderReconcileResult::Pending);
        }

        if let Some(desired) = desired {
            self.commands_sender_role_assignment_intent = Some(desired);
            return Ok(CommandsSenderReconcileResult::Pending);
        }

        Ok(CommandsSenderReconcileResult::Complete)
    }

    pub(super) async fn delete_commands_sender_role_assignment_step(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
    ) -> Result<CommandsSenderReconcileResult> {
        if self.commands_sender_role_assignment_id.is_some() {
            self.commands_sender_role_assignment_id = None;
            self.commands_sender_role_assignment_discovery_complete = false;
            return Ok(CommandsSenderReconcileResult::Pending);
        }
        if self.commands_sender_role_assignment_intent.is_some() {
            self.commands_sender_role_assignment_intent = None;
            self.commands_sender_role_assignment_discovery_complete = false;
            return Ok(CommandsSenderReconcileResult::Pending);
        }
        if !self.commands_sender_role_assignment_discovery_complete {
            return match self
                .discover_commands_sender_role_assignments(ctx, worker, None)
                .await?
            {
                CommandsSenderDiscoveryResult::Mutated => {
                    Ok(CommandsSenderReconcileResult::Pending)
                }
                CommandsSenderDiscoveryResult::Complete { .. } => {
                    Ok(CommandsSenderReconcileResult::Pending)
                }
            };
        }
        Ok(CommandsSenderReconcileResult::Complete)
    }
}

#[cfg(test)]
#[path = "azure_command_sender_tests.rs"]
mod tests;
