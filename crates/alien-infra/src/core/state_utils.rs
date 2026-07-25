use alien_core::{
    ownership_policy_for_resource_type, ResourceLifecycle, ResourceStatus, StackResourceState,
    StackState,
};

use alien_error::{AlienError, Context, IntoAlienError};

use crate::{ErrorData, ResourceController, Result};

/// Extension trait for StackResourceState that adds controller-specific functionality.
/// This allows StackResourceState to be moved to alien-core while keeping controller
/// methods in alien-infra.
pub trait StackResourceStateExt {
    /// Deserializes the internal state to a ResourceController if present.
    /// Returns None if there's no internal state or deserialization fails.
    fn get_internal_controller(&self) -> Result<Option<Box<dyn ResourceController>>>;

    /// Deserializes the internal state to a specific controller type.
    /// Returns the typed controller if successful, or an error if not found or wrong type.
    fn get_internal_controller_typed<
        T: ResourceController + serde::de::DeserializeOwned + 'static,
    >(
        &self,
    ) -> Result<T>;

    /// Deserializes the last failed state to a ResourceController if present.
    /// Returns None if there's no last failed state or deserialization fails.
    fn get_last_failed_controller(&self) -> Result<Option<Box<dyn ResourceController>>>;

    /// Sets the internal state from a ResourceController, serializing it to JSON.
    fn set_internal_controller(
        &mut self,
        controller: Option<Box<dyn ResourceController>>,
    ) -> Result<()>;

    /// Sets the last failed state from a ResourceController, serializing it to JSON.
    fn set_last_failed_controller(
        &mut self,
        controller: Option<Box<dyn ResourceController>>,
    ) -> Result<()>;

    /// Convenience method to check if internal state is present (avoiding deserialization).
    fn has_internal_state(&self) -> bool;

    /// Convenience method to check if last failed state is present (avoiding deserialization).
    fn has_last_failed_state(&self) -> bool;

    /// Takes the last failed state and converts it to a ResourceController, removing it from this state.
    /// Returns None if there's no last failed state or deserialization fails.
    fn take_last_failed_controller(&mut self) -> Result<Option<Box<dyn ResourceController>>>;

    /// Attempts to retry this resource if it's currently in a failed state.
    /// Uses the stored `last_failed_state` to resume from exactly where the failure occurred.
    fn retry_failed(&mut self) -> Result<bool>;
}

impl StackResourceStateExt for StackResourceState {
    fn get_internal_controller(&self) -> Result<Option<Box<dyn ResourceController>>> {
        match &self.internal_state {
            Some(value) => {
                let controller: Box<dyn ResourceController> =
                    crate::core::deserialize_controller(value.clone())
                        .into_alien_error()
                        .context(ErrorData::ResourceStateSerializationFailed {
                            resource_id: self.config.id().to_string(),
                            message: "Failed to deserialize internal state".to_string(),
                        })?;
                Ok(Some(controller))
            }
            None => Ok(None),
        }
    }

    fn get_internal_controller_typed<
        T: ResourceController + serde::de::DeserializeOwned + 'static,
    >(
        &self,
    ) -> Result<T> {
        let value = self.internal_state.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceStateSerializationFailed {
                resource_id: self.config.id().to_string(),
                message: "No internal state available".to_string(),
            })
        })?;

        serde_json::from_value::<T>(value.clone())
            .into_alien_error()
            .context(ErrorData::ControllerStateTypeMismatch {
                expected: std::any::type_name::<T>().to_string(),
                resource_id: self.config.id().to_string(),
            })
    }

    fn get_last_failed_controller(&self) -> Result<Option<Box<dyn ResourceController>>> {
        match &self.last_failed_state {
            Some(value) => {
                let controller: Box<dyn ResourceController> =
                    crate::core::deserialize_controller(value.clone())
                        .into_alien_error()
                        .context(ErrorData::ResourceStateSerializationFailed {
                            resource_id: self.config.id().to_string(),
                            message: "Failed to deserialize last failed state".to_string(),
                        })?;
                Ok(Some(controller))
            }
            None => Ok(None),
        }
    }

    fn set_internal_controller(
        &mut self,
        controller: Option<Box<dyn ResourceController>>,
    ) -> Result<()> {
        self.internal_state = match controller {
            Some(c) => Some(
                crate::core::serialize_controller(&*c)
                    .into_alien_error()
                    .context(ErrorData::ResourceStateSerializationFailed {
                        resource_id: self.config.id().to_string(),
                        message: "Failed to serialize controller state".to_string(),
                    })?,
            ),
            None => None,
        };
        Ok(())
    }

    fn set_last_failed_controller(
        &mut self,
        controller: Option<Box<dyn ResourceController>>,
    ) -> Result<()> {
        self.last_failed_state = match controller {
            Some(c) => Some(
                crate::core::serialize_controller(&*c)
                    .into_alien_error()
                    .context(ErrorData::ResourceStateSerializationFailed {
                        resource_id: self.config.id().to_string(),
                        message: "Failed to serialize last failed controller state".to_string(),
                    })?,
            ),
            None => None,
        };
        Ok(())
    }

    fn has_internal_state(&self) -> bool {
        self.internal_state.is_some()
    }

    fn has_last_failed_state(&self) -> bool {
        self.last_failed_state.is_some()
    }

    fn take_last_failed_controller(&mut self) -> Result<Option<Box<dyn ResourceController>>> {
        match self.last_failed_state.take() {
            Some(value) => {
                let controller: Box<dyn ResourceController> =
                    crate::core::deserialize_controller(value)
                        .into_alien_error()
                        .context(ErrorData::ResourceStateSerializationFailed {
                            resource_id: self.config.id().to_string(),
                            message: "Failed to deserialize last failed state".to_string(),
                        })?;
                Ok(Some(controller))
            }
            None => Ok(None),
        }
    }

    fn retry_failed(&mut self) -> Result<bool> {
        let resource_id = self.config.id().to_string();

        // Check if this resource is in a failed state
        let is_failed = matches!(
            self.status,
            ResourceStatus::ProvisionFailed
                | ResourceStatus::UpdateFailed
                | ResourceStatus::DeleteFailed
                | ResourceStatus::RefreshFailed
        );

        if !is_failed {
            return Ok(false);
        }

        // Use the stored last failed state to resume from where the failure occurred
        match self.take_last_failed_controller() {
            Ok(Some(mut last_failed_state)) => {
                tracing::info!(
                    resource_id = %resource_id,
                    "Resuming resource from last failed state"
                );
                last_failed_state.reset_stay_count();
                let next_status = last_failed_state.get_status();
                let next_outputs = last_failed_state.get_outputs();
                self.set_internal_controller(Some(last_failed_state))?;
                self.retry_attempt = 0;
                self.error = None;
                self.status = next_status;
                self.outputs = next_outputs;
                Ok(true)
            }
            Ok(None) => {
                // No last failed state — the resource was interrupted before its controller was
                // initialized (i.e. it was in Pending when the deployment stopped). Reset it to
                // Pending so it starts fresh on the next deployment attempt.
                tracing::info!(
                    resource_id = %resource_id,
                    "No last failed state; resource was interrupted before initialization — resetting to Pending"
                );
                self.status = ResourceStatus::Pending;
                self.internal_state = None;
                self.retry_attempt = 0;
                self.error = None;
                Ok(true)
            }
            Err(e) => {
                tracing::error!(
                    resource_id = %resource_id,
                    error = %e,
                    "Failed to deserialize last failed state for resource"
                );
                Err(e)
            }
        }
    }
}

/// Extension trait for StackState that adds retry functionality for all failed resources.
pub trait StackStateExt {
    /// Attempts to retry all failed resources in the stack.
    /// Returns the IDs of resources that were successfully retried.
    fn retry_failed(&mut self) -> Result<Vec<String>>;

    /// Prepares the stack for destroy operations by handling failed resources appropriately.
    /// - For ProvisionFailed/UpdateFailed/RefreshFailed resources: transitions them to delete start
    /// - For DeleteFailed resources: retries the delete operation
    /// Returns the IDs of resources that were successfully prepared.
    fn prepare_for_destroy(&mut self) -> Result<Vec<String>>;

    /// Same as [`StackStateExt::prepare_for_destroy`], limited to resources
    /// whose lifecycle matches the provided filter.
    fn prepare_for_destroy_with_lifecycle_filter(
        &mut self,
        lifecycle_filter: &[ResourceLifecycle],
    ) -> Result<Vec<String>>;

    /// Same as [`StackStateExt::prepare_for_destroy`], limited to resources
    /// whose lifecycle matches the provided filter, and using setup teardown
    /// transitions instead of runtime delete transitions.
    fn prepare_for_teardown_with_lifecycle_filter(
        &mut self,
        lifecycle_filter: &[ResourceLifecycle],
    ) -> Result<Vec<String>>;

    /// Same as [`StackStateExt::prepare_for_destroy`], limited to resources
    /// owned by runtime cleanup: Live resources and Frozen resources with
    /// explicit runtime cleanup before teardown.
    fn prepare_for_runtime_cleanup_destroy(&mut self) -> Result<Vec<String>>;
}

impl StackStateExt for StackState {
    fn retry_failed(&mut self) -> Result<Vec<String>> {
        let mut retried_resource_ids = Vec::new();

        for (resource_id, resource_state) in &mut self.resources {
            match resource_state.retry_failed() {
                Ok(true) => {
                    tracing::info!(resource_id = %resource_id, "Successfully retried failed resource");
                    retried_resource_ids.push(resource_id.clone());
                }
                Ok(false) => {
                    // Resource wasn't in a failed state or couldn't be retried
                }
                Err(e) => {
                    tracing::error!(
                        resource_id = %resource_id,
                        error = %e,
                        "Failed to retry resource"
                    );
                    return Err(e);
                }
            }
        }

        tracing::info!(retried_resource_ids = ?retried_resource_ids, "Completed retry operation on stack");
        Ok(retried_resource_ids)
    }

    fn prepare_for_destroy(&mut self) -> Result<Vec<String>> {
        prepare_for_destroy_matching(self, |_| Ok(true), DestroyPreparationMode::Teardown)
    }

    fn prepare_for_destroy_with_lifecycle_filter(
        &mut self,
        lifecycle_filter: &[ResourceLifecycle],
    ) -> Result<Vec<String>> {
        prepare_for_destroy_matching(
            self,
            |resource_state| {
                Ok(lifecycle_filter.contains(&resource_lifecycle(
                    resource_id_from_state(resource_state),
                    resource_state,
                )?))
            },
            DestroyPreparationMode::Delete,
        )
    }

    fn prepare_for_teardown_with_lifecycle_filter(
        &mut self,
        lifecycle_filter: &[ResourceLifecycle],
    ) -> Result<Vec<String>> {
        prepare_for_destroy_matching(
            self,
            |resource_state| {
                Ok(lifecycle_filter.contains(&resource_lifecycle(
                    resource_id_from_state(resource_state),
                    resource_state,
                )?))
            },
            DestroyPreparationMode::Teardown,
        )
    }

    fn prepare_for_runtime_cleanup_destroy(&mut self) -> Result<Vec<String>> {
        prepare_for_destroy_matching(
            self,
            is_runtime_cleanup_resource,
            DestroyPreparationMode::Delete,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DestroyPreparationMode {
    Delete,
    Teardown,
}

fn resource_id_from_state(resource_state: &StackResourceState) -> &str {
    resource_state.config.id()
}

fn resource_lifecycle(
    resource_id: &str,
    resource_state: &StackResourceState,
) -> Result<ResourceLifecycle> {
    resource_state.lifecycle.ok_or_else(|| {
        AlienError::new(ErrorData::ResourceLifecycleMissing {
            resource_id: resource_id.to_string(),
        })
    })
}

fn is_runtime_cleanup_resource(resource_state: &StackResourceState) -> Result<bool> {
    if resource_lifecycle(resource_id_from_state(resource_state), resource_state)?
        == ResourceLifecycle::Live
    {
        return Ok(true);
    }

    Ok(
        ownership_policy_for_resource_type(resource_state.config.resource_type().as_ref())
            .has_runtime_cleanup_before_teardown(),
    )
}

fn prepare_for_destroy_matching(
    stack_state: &mut StackState,
    should_prepare: impl Fn(&StackResourceState) -> Result<bool>,
    mode: DestroyPreparationMode,
) -> Result<Vec<String>> {
    let mut prepared_resource_ids = Vec::new();

    for (resource_id, resource_state) in &mut stack_state.resources {
        if !should_prepare(resource_state)? {
            continue;
        }

        match resource_state.status {
            ResourceStatus::ProvisionFailed
            | ResourceStatus::UpdateFailed
            | ResourceStatus::RefreshFailed => {
                // For non-delete failures during destroy, transition to delete start.
                match resource_state.get_internal_controller() {
                    Ok(Some(mut controller)) => {
                        tracing::info!(
                            resource_id = %resource_id,
                            current_status = ?resource_state.status,
                            "Transitioning failed resource to delete start for destroy operation"
                        );

                        match controller.transition_to_delete_start() {
                            Ok(()) => {
                                let next_status = controller.get_status();
                                let next_outputs = controller.get_outputs();

                                resource_state.set_internal_controller(Some(controller))?;
                                resource_state.retry_attempt = 0;
                                resource_state.error = None;
                                resource_state.status = next_status;
                                resource_state.outputs = next_outputs;
                                resource_state.last_failed_state = None;

                                prepared_resource_ids.push(resource_id.clone());
                                tracing::info!(
                                    resource_id = %resource_id,
                                    new_status = ?next_status,
                                    "Successfully transitioned resource to delete start"
                                );
                            }
                            Err(e) => {
                                if mode == DestroyPreparationMode::Teardown {
                                    return Err(e);
                                }

                                tracing::warn!(
                                    resource_id = %resource_id,
                                    error = %e,
                                    "Cannot transition resource to delete start - this may indicate the resource doesn't support deletion from this state"
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        if mode == DestroyPreparationMode::Teardown {
                            return Err(AlienError::new(
                                ErrorData::ResourceStateSerializationFailed {
                                    resource_id: resource_id.clone(),
                                    message:
                                        "Missing controller state for setup-owned resource teardown"
                                            .to_string(),
                                },
                            ));
                        }

                        if resource_state.status == ResourceStatus::ProvisionFailed {
                            tracing::info!(
                                resource_id = %resource_id,
                                "Marking never-provisioned resource as deleted"
                            );
                            resource_state.status = ResourceStatus::Deleted;
                            resource_state.outputs = None;
                            resource_state.retry_attempt = 0;
                            resource_state.error = None;
                            resource_state.last_failed_state = None;
                            prepared_resource_ids.push(resource_id.clone());
                        } else {
                            tracing::warn!(
                                resource_id = %resource_id,
                                current_status = ?resource_state.status,
                                "No internal controller state for failed resource - cannot transition to delete"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            resource_id = %resource_id,
                            error = %e,
                            "Failed to deserialize controller state for resource"
                        );
                        return Err(e);
                    }
                }
            }
            ResourceStatus::DeleteFailed => {
                // For delete failures, use normal retry logic
                match resource_state.retry_failed() {
                    Ok(true) => {
                        tracing::info!(resource_id = %resource_id, "Successfully retried failed delete operation");
                        prepared_resource_ids.push(resource_id.clone());
                    }
                    Ok(false) => {
                        tracing::debug!(resource_id = %resource_id, "Delete failed resource could not be retried");
                    }
                    Err(e) => {
                        tracing::error!(
                            resource_id = %resource_id,
                            error = %e,
                            "Failed to retry delete operation for resource"
                        );
                        return Err(e);
                    }
                }
            }
            ResourceStatus::TeardownRequired if mode == DestroyPreparationMode::Teardown => {
                match resource_state.get_internal_controller() {
                    Ok(Some(mut controller)) => {
                        tracing::info!(
                            resource_id = %resource_id,
                            "Transitioning resource to teardown start"
                        );

                        controller.transition_to_teardown_start()?;
                        let next_status = controller.get_status();
                        let next_outputs = controller.get_outputs();

                        resource_state.set_internal_controller(Some(controller))?;
                        resource_state.retry_attempt = 0;
                        resource_state.error = None;
                        resource_state.status = next_status;
                        resource_state.outputs = next_outputs;
                        resource_state.last_failed_state = None;

                        prepared_resource_ids.push(resource_id.clone());
                    }
                    Ok(None) => {
                        return Err(AlienError::new(
                            ErrorData::ResourceStateSerializationFailed {
                                resource_id: resource_id.clone(),
                                message: "Missing controller state for teardown-required resource"
                                    .to_string(),
                            },
                        ));
                    }
                    Err(e) => {
                        tracing::error!(
                            resource_id = %resource_id,
                            error = %e,
                            "Failed to deserialize controller state for teardown-required resource"
                        );
                        return Err(e);
                    }
                }
            }
            _ => {
                // Resource is not in a failed state, no action needed
            }
        }
    }

    tracing::info!(prepared_resource_ids = ?prepared_resource_ids, "Completed prepare for destroy operation on stack");
    Ok(prepared_resource_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::{TestWorkerController, TestWorkerState};
    use alien_core::{Platform, Resource, ResourceStatus, StackResourceState, StackState};
    use alien_core::{Worker, WorkerCode};
    use alien_error::GenericError;

    #[tokio::test]
    async fn test_prepare_for_destroy_provision_failed() {
        let mut stack_state = StackState::new(Platform::Test);

        // Create a function resource that failed during provision
        let function_config = Worker::new("test-function".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let mut failed_controller = TestWorkerController::default();
        failed_controller.state = TestWorkerState::CreateFailed;

        let mut resource_state = StackResourceState::new_pending(
            "worker".to_string(),
            Resource::new(function_config),
            None,
            Vec::new(),
        );
        resource_state.status = ResourceStatus::ProvisionFailed;
        resource_state
            .set_internal_controller(Some(Box::new(failed_controller)))
            .unwrap();

        stack_state
            .resources
            .insert("test-function".to_string(), resource_state);

        // Prepare for destroy
        let prepared = stack_state.prepare_for_destroy().unwrap();

        // Should have prepared the resource
        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0], "test-function");

        // Resource should now be in DeleteStart state
        let updated_resource = stack_state.resources.get("test-function").unwrap();
        assert_eq!(updated_resource.status, ResourceStatus::Deleting);

        let controller = updated_resource
            .get_internal_controller_typed::<TestWorkerController>()
            .unwrap();
        assert_eq!(controller.state, TestWorkerState::DeleteStart);

        // Error should be cleared
        assert!(updated_resource.error.is_none());
        assert_eq!(updated_resource.retry_attempt, 0);
        assert!(updated_resource.last_failed_state.is_none());
    }

    #[test]
    fn runtime_cleanup_marks_never_provisioned_resource_deleted() {
        let function_config = Worker::new("test-function".to_string())
            .code(WorkerCode::Image {
                image: "invalid-source-image".to_string(),
            })
            .permissions("execution".to_string())
            .build();
        let mut resource_state = StackResourceState::new_pending(
            "worker".to_string(),
            Resource::new(function_config),
            None,
            Vec::new(),
        );
        resource_state.lifecycle = Some(ResourceLifecycle::Live);
        resource_state.status = ResourceStatus::ProvisionFailed;
        resource_state.error = Some(AlienError::new(GenericError {
            message: "provision failed".to_string(),
        }));
        let mut stack_state = StackState::new(Platform::Test);
        stack_state
            .resources
            .insert("test-function".to_string(), resource_state);

        let prepared = stack_state.prepare_for_runtime_cleanup_destroy().unwrap();

        assert_eq!(prepared, vec!["test-function"]);
        let deleted_resource = stack_state.resources.get("test-function").unwrap();
        assert_eq!(deleted_resource.status, ResourceStatus::Deleted);
        assert!(deleted_resource.error.is_none());
        assert!(deleted_resource.outputs.is_none());
    }

    #[tokio::test]
    async fn test_prepare_for_destroy_update_failed() {
        let mut stack_state = StackState::new(Platform::Test);

        // Create a function resource that failed during update
        let function_config = Worker::new("test-function".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let mut failed_controller = TestWorkerController::default();
        failed_controller.state = TestWorkerState::UpdateFailed;

        let mut resource_state = StackResourceState::new_pending(
            "worker".to_string(),
            Resource::new(function_config),
            None,
            Vec::new(),
        );
        resource_state.status = ResourceStatus::UpdateFailed;
        resource_state
            .set_internal_controller(Some(Box::new(failed_controller)))
            .unwrap();

        stack_state
            .resources
            .insert("test-function".to_string(), resource_state);

        // Prepare for destroy
        let prepared = stack_state.prepare_for_destroy().unwrap();

        // Should have prepared the resource
        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0], "test-function");

        // Resource should now be in DeleteStart state
        let updated_resource = stack_state.resources.get("test-function").unwrap();
        assert_eq!(updated_resource.status, ResourceStatus::Deleting);

        let controller = updated_resource
            .get_internal_controller_typed::<TestWorkerController>()
            .unwrap();
        assert_eq!(controller.state, TestWorkerState::DeleteStart);
    }

    #[tokio::test]
    async fn test_prepare_for_destroy_refresh_failed() {
        let mut stack_state = StackState::new(Platform::Test);

        let function_config = Worker::new("test-function".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let mut failed_controller = TestWorkerController::default();
        failed_controller.state = TestWorkerState::Ready;

        let mut resource_state = StackResourceState::new_pending(
            "worker".to_string(),
            Resource::new(function_config),
            None,
            Vec::new(),
        );
        resource_state.status = ResourceStatus::RefreshFailed;
        resource_state.error = Some(AlienError::new(GenericError {
            message: "heartbeat failed".to_string(),
        }));
        resource_state.retry_attempt = 10;
        resource_state
            .set_internal_controller(Some(Box::new(failed_controller)))
            .unwrap();

        stack_state
            .resources
            .insert("test-function".to_string(), resource_state);

        let prepared = stack_state.prepare_for_destroy().unwrap();

        assert_eq!(prepared, vec!["test-function"]);

        let updated_resource = stack_state.resources.get("test-function").unwrap();
        assert_eq!(updated_resource.status, ResourceStatus::Deleting);
        assert_eq!(updated_resource.retry_attempt, 0);
        assert!(updated_resource.error.is_none());
        assert!(updated_resource.last_failed_state.is_none());

        let controller = updated_resource
            .get_internal_controller_typed::<TestWorkerController>()
            .unwrap();
        assert_eq!(controller.state, TestWorkerState::DeleteStart);
    }

    #[tokio::test]
    async fn test_prepare_for_destroy_delete_failed() {
        let mut stack_state = StackState::new(Platform::Test);

        // Create a function resource that failed during delete
        let function_config = Worker::new("test-function".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let mut failed_controller = TestWorkerController::default();
        failed_controller.state = TestWorkerState::DeleteFailed;

        let mut resource_state = StackResourceState::new_pending(
            "worker".to_string(),
            Resource::new(function_config),
            None,
            Vec::new(),
        );
        resource_state.status = ResourceStatus::DeleteFailed;
        resource_state
            .set_internal_controller(Some(Box::new(failed_controller.clone())))
            .unwrap();
        resource_state
            .set_last_failed_controller(Some(Box::new(failed_controller)))
            .unwrap();

        stack_state
            .resources
            .insert("test-function".to_string(), resource_state);

        // Prepare for destroy
        let prepared = stack_state.prepare_for_destroy().unwrap();

        // Should have prepared the resource
        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0], "test-function");

        // Resource should still be in DeleteFailed state but with retry cleared
        let updated_resource = stack_state.resources.get("test-function").unwrap();
        assert_eq!(updated_resource.status, ResourceStatus::DeleteFailed);
        assert_eq!(updated_resource.retry_attempt, 0);
        assert!(updated_resource.error.is_none());
    }

    #[tokio::test]
    async fn test_prepare_for_destroy_running_resource() {
        let mut stack_state = StackState::new(Platform::Test);

        // Create a running function resource
        let function_config = Worker::new("test-function".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let mut running_controller = TestWorkerController::default();
        running_controller.state = TestWorkerState::Ready;

        let mut resource_state = StackResourceState::new_pending(
            "worker".to_string(),
            Resource::new(function_config),
            None,
            Vec::new(),
        );
        resource_state.status = ResourceStatus::Running;
        resource_state
            .set_internal_controller(Some(Box::new(running_controller)))
            .unwrap();

        stack_state
            .resources
            .insert("test-function".to_string(), resource_state);

        // Prepare for destroy
        let prepared = stack_state.prepare_for_destroy().unwrap();

        // Should not have prepared any resources (running resources don't need preparation)
        assert_eq!(prepared.len(), 0);

        // Resource should be unchanged
        let updated_resource = stack_state.resources.get("test-function").unwrap();
        assert_eq!(updated_resource.status, ResourceStatus::Running);

        let controller = updated_resource
            .get_internal_controller_typed::<TestWorkerController>()
            .unwrap();
        assert_eq!(controller.state, TestWorkerState::Ready);
    }

    /// Test A: retry_failed() resets _internal_stay_count to None (Bug 2 fix).
    ///
    /// Simulates a resource that was mid-polling (stay count = 50) when it was saved
    /// as lastFailedState. After retry_failed(), the restored controller must have
    /// _internal_stay_count = None so the next run gets a full fresh polling window.
    #[tokio::test]
    async fn test_retry_failed_resets_internal_stay_count() {
        let function_config = Worker::new("test-function".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let mut polling_controller = TestWorkerController::default();
        polling_controller.state = TestWorkerState::CreateWorkerPolling;
        polling_controller.identifier = Some("test:function:test-function".to_string());
        // Simulate a controller that was mid-exhaustion with 50 out of 60 polls used.
        polling_controller._internal_stay_count = Some(50);

        let mut resource_state = StackResourceState::new_pending(
            "worker".to_string(),
            Resource::new(function_config),
            None,
            Vec::new(),
        );
        resource_state.status = ResourceStatus::ProvisionFailed;
        resource_state
            .set_last_failed_controller(Some(Box::new(polling_controller)))
            .unwrap();

        let retried = resource_state.retry_failed().unwrap();
        assert!(
            retried,
            "retry_failed() should return true for a ProvisionFailed resource"
        );
        assert_eq!(resource_state.status, ResourceStatus::Provisioning);
        assert!(
            resource_state.error.is_none(),
            "error should be cleared after retry"
        );
        assert_eq!(
            resource_state.retry_attempt, 0,
            "retry_attempt should be reset to 0"
        );

        // The restored controller must have _internal_stay_count = None so the
        // next run gets the full max_times polling window, not the leftover 10.
        let restored = resource_state
            .get_internal_controller_typed::<TestWorkerController>()
            .unwrap();
        assert_eq!(restored.state, TestWorkerState::CreateWorkerPolling);
        assert!(
            restored._internal_stay_count.is_none(),
            "_internal_stay_count must be None after retry, got {:?}",
            restored._internal_stay_count
        );
    }

    #[tokio::test]
    async fn test_retry_failed_recovers_refresh_failed_resource() {
        let function_config = Worker::new("test-function".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let mut last_ready_controller = TestWorkerController::default();
        last_ready_controller.state = TestWorkerState::Ready;

        let mut resource_state = StackResourceState::new_pending(
            "worker".to_string(),
            Resource::new(function_config),
            None,
            Vec::new(),
        );
        resource_state.status = ResourceStatus::RefreshFailed;
        resource_state.retry_attempt = 10;
        resource_state.error = Some(AlienError::new(GenericError {
            message: "heartbeat failed".to_string(),
        }));
        resource_state
            .set_last_failed_controller(Some(Box::new(last_ready_controller)))
            .unwrap();

        let retried = resource_state.retry_failed().unwrap();

        assert!(retried, "refresh-failed resources must be retryable");
        assert_eq!(resource_state.status, ResourceStatus::Running);
        assert_eq!(resource_state.retry_attempt, 0);
        assert!(resource_state.error.is_none());
        assert!(resource_state.last_failed_state.is_none());
    }
}
