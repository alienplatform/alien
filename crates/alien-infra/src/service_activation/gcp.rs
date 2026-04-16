use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{ResourceOutputs, ResourceStatus, ServiceActivation, ServiceActivationOutputs};
use alien_error::{AlienError, Context, ContextError as _};
use alien_gcp_clients::longrunning::OperationResult;
use alien_gcp_clients::service_usage::{ServiceUsageApi, State};
use alien_macros::{controller, flow_entry, handler, terminal_state};

#[controller]
pub struct GcpServiceActivationController {
    /// The name of the service being managed.
    pub service_name: Option<String>,
    /// Whether the service is currently enabled.
    pub service_activated: bool,
    /// The name of the current operation being tracked (for async operations).
    pub operation_name: Option<String>,
}

#[controller]
impl GcpServiceActivationController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_service_usage_client(gcp_config)?;
        let config = ctx.desired_resource_config::<ServiceActivation>()?;

        info!(
            service_id = %config.id,
            service_name = %config.service_name,
            "Starting GCP service activation"
        );

        // Check if the service is already enabled (reading before command is allowed)
        let already_enabled = match client.get_service(config.service_name.clone()).await {
            Ok(service) => {
                if let Some(state) = service.state {
                    state == State::Enabled
                } else {
                    false
                }
            }
            Err(e) => {
                let msg = format!("Failed to get GCP service '{}': {}", config.service_name, e);
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: msg,
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        if already_enabled {
            info!(
                service_id = %config.id,
                service_name = %config.service_name,
                "Service is already enabled, but following linear flow"
            );
        }

        // Update state and always transition to EnablingService to maintain linear flow
        self.service_name = Some(config.service_name.clone());
        self.service_activated = already_enabled;

        Ok(HandlerAction::Continue {
            state: EnablingService,
            suggested_delay: None,
        })
    }

    #[handler(
        state = EnablingService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn enabling_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_service_usage_client(gcp_config)?;
        let config = ctx.desired_resource_config::<ServiceActivation>()?;

        info!(
            service_id = %config.id,
            service_name = %config.service_name,
            "Enabling GCP service"
        );

        // If service is already enabled, skip the enable operation but maintain linear flow
        if self.service_activated {
            info!(
                service_id = %config.id,
                service_name = %config.service_name,
                "Service already enabled, skipping enable operation"
            );
            self.operation_name = None; // No operation needed
            return Ok(HandlerAction::Continue {
                state: WaitingForServiceEnabled,
                suggested_delay: Some(Duration::from_secs(1)), // Small delay to maintain flow timing
            });
        }

        // Service is not enabled, proceed to enable it
        match client.enable_service(config.service_name.clone()).await {
            Ok(operation) => {
                info!(
                    service_id = %config.id,
                    service_name = %config.service_name,
                    operation_name = ?operation.name,
                    "Service enablement operation started"
                );

                self.operation_name = operation.name;
                Ok(HandlerAction::Continue {
                    state: WaitingForServiceEnabled,
                    suggested_delay: Some(Duration::from_secs(5)), // Wait before checking operation status
                })
            }
            Err(e) => {
                let msg = format!(
                    "Failed to enable GCP service '{}': {}",
                    config.service_name, e
                );
                Err(e.context(ErrorData::CloudPlatformError {
                    message: msg,
                    resource_id: Some(config.id.clone()),
                }))
            }
        }
    }

    #[handler(
        state = WaitingForServiceEnabled,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_service_enabled(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_service_usage_client(gcp_config)?;
        let config = ctx.desired_resource_config::<ServiceActivation>()?;

        debug!(
            service_id = %config.id,
            service_name = %config.service_name,
            operation_name = ?self.operation_name,
            "Checking service enablement status"
        );

        // If no operation name and service is already enabled, transition to Ready
        if self.operation_name.is_none() && self.service_activated {
            info!(
                service_id = %config.id,
                service_name = %config.service_name,
                "Service was already enabled, transitioning to Ready"
            );
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        // If we have an operation name, check the operation status first
        if let Some(op_name) = self.operation_name.as_ref() {
            // GCP returns "operations/noop.DONE_OPERATION" when a service was already
            // enabled. This synthetic operation name cannot be polled, so skip directly
            // to verifying the service state.
            if op_name.contains("noop.DONE_OPERATION") {
                info!(
                    service_id = %config.id,
                    service_name = %config.service_name,
                    "Service enablement was a noop (service already enabled), skipping operation poll"
                );
            } else {
                match client.get_operation(op_name.to_string()).await {
                    Ok(operation) => {
                        if let Some(done) = operation.done {
                            if done {
                                if let Some(result) = operation.result {
                                    match result {
                                        OperationResult::Error { error } => {
                                            return Err(AlienError::new(
                                                ErrorData::CloudPlatformError {
                                                    message: format!(
                                                        "Service enablement operation failed: {}",
                                                        error.message
                                                    ),
                                                    resource_id: Some(config.id.clone()),
                                                },
                                            ));
                                        }
                                        _ => {}
                                    }
                                }
                                info!(
                                    service_id = %config.id,
                                    service_name = %config.service_name,
                                    "Service enablement operation completed successfully"
                                );
                            } else {
                                // Operation still in progress
                                debug!(
                                    service_id = %config.id,
                                    service_name = %config.service_name,
                                    "Service enablement operation still in progress"
                                );
                                return Ok(HandlerAction::Stay {
                                    max_times: 60,
                                    suggested_delay: Some(Duration::from_secs(10)),
                                });
                            }
                        }
                    }
                    Err(e) => {
                        let msg = format!(
                            "Failed to get GCP service enablement operation '{}': {}",
                            op_name, e
                        );
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: msg,
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }
            } // else (non-noop operation)
        }

        // Check the actual service status
        match client.get_service(config.service_name.clone()).await {
            Ok(service) => {
                if let Some(state) = service.state {
                    if state == State::Enabled {
                        info!(
                            service_id = %config.id,
                            service_name = %config.service_name,
                            "Service is now enabled"
                        );
                        self.service_activated = true;
                        return Ok(HandlerAction::Continue {
                            state: Ready,
                            suggested_delay: None,
                        });
                    } else {
                        debug!(
                            service_id = %config.id,
                            service_name = %config.service_name,
                            current_state = ?state,
                            "Service not yet enabled"
                        );
                    }
                } else {
                    debug!(
                        service_id = %config.id,
                        service_name = %config.service_name,
                        "Service state not available"
                    );
                }
            }
            Err(e) => {
                let msg = format!("Failed to get GCP service '{}': {}", config.service_name, e);
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: msg,
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        // Service not yet enabled, continue waiting
        Ok(HandlerAction::Stay {
            max_times: 60,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<ServiceActivation>()?;

        // Heartbeat check: verify service is still enabled
        if let Some(service_name) = &self.service_name {
            let client = ctx
                .service_provider
                .get_gcp_service_usage_client(gcp_config)?;

            let service = client.get_service(service_name.clone()).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get GCP service '{}'", service_name),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            if let Some(state) = service.state {
                if state != State::Enabled {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!("Service state changed from Enabled to {:?}", state),
                    }));
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    // service_name is immutable; no fields can change after creation.
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceActivation>()?;
        info!(id=%config.id, "GCP ServiceActivation update (no-op — service_name is immutable)");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceActivation>()?;

        info!(
            service_id = %config.id,
            service_name = %config.service_name,
            "GCP Service activation deletion requested - skipping (services are not disabled on delete)"
        );

        // We don't disable services on delete as it can be dangerous and break other resources
        // Just mark as deleted immediately
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    // Override the generated get_outputs method
    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let Some(service_name) = &self.service_name {
            Some(ResourceOutputs::new(ServiceActivationOutputs {
                service_name: service_name.clone(),
                activated: self.service_activated,
            }))
        } else {
            None
        }
    }
}

impl GcpServiceActivationController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(service_name: &str) -> Self {
        Self {
            state: GcpServiceActivationState::Ready,
            service_name: Some(service_name.to_string()),
            service_activated: true,
            operation_name: None,
            _internal_stay_count: None,
        }
    }
}
