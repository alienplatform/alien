use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::resources::ResourcesApi;
use alien_core::{ResourceOutputs, ResourceStatus, ServiceActivation, ServiceActivationOutputs};
use alien_error::{AlienError, Context, ContextError as _};
use alien_macros::{controller, flow_entry, handler, terminal_state};

#[controller]
pub struct AzureServiceActivationController {
    /// The name of the service being managed.
    pub service_name: Option<String>,
    /// Whether the service is currently registered.
    pub service_activated: bool,
}

#[controller]
impl AzureServiceActivationController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;
        let config = ctx.desired_resource_config::<ServiceActivation>()?;

        info!(
            service_id = %config.id,
            service_name = %config.service_name,
            "Starting Azure service activation (provider registration)"
        );

        // Check if the provider is already registered (reading before command is allowed)
        let already_registered = match client.get_provider(&config.service_name).await {
            Ok(provider) => {
                if let Some(registration_state) = provider.registration_state {
                    registration_state.to_lowercase() == "registered"
                } else {
                    false
                }
            }
            Err(e) => {
                let msg = format!(
                    "Failed to get Azure provider '{}': {}",
                    config.service_name, e
                );
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: msg,
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        if already_registered {
            info!(
                service_id = %config.id,
                service_name = %config.service_name,
                "Provider is already registered, but following linear flow"
            );
        }

        // Update state and always transition to RegisteringProvider to maintain linear flow
        self.service_name = Some(config.service_name.clone());
        self.service_activated = already_registered;

        Ok(HandlerAction::Continue {
            state: RegisteringProvider,
            suggested_delay: None,
        })
    }

    #[handler(
        state = RegisteringProvider,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn registering_provider(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;
        let config = ctx.desired_resource_config::<ServiceActivation>()?;

        info!(
            service_id = %config.id,
            service_name = %config.service_name,
            "Registering Azure provider"
        );

        // If provider is already registered, skip the register operation but maintain linear flow
        if self.service_activated {
            info!(
                service_id = %config.id,
                service_name = %config.service_name,
                "Provider already registered, skipping register operation"
            );
            return Ok(HandlerAction::Continue {
                state: WaitingForProviderRegistered,
                suggested_delay: Some(Duration::from_secs(1)), // Small delay to maintain flow timing
            });
        }

        // Provider is not registered, proceed to register it
        match client.register_provider(&config.service_name, None).await {
            Ok(provider) => {
                info!(
                    service_id = %config.id,
                    service_name = %config.service_name,
                    registration_state = ?provider.registration_state,
                    "Provider registration operation started"
                );

                Ok(HandlerAction::Continue {
                    state: WaitingForProviderRegistered,
                    suggested_delay: Some(Duration::from_secs(5)), // Wait before checking registration status
                })
            }
            Err(e) => {
                let msg = format!(
                    "Failed to register Azure provider '{}': {}",
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
        state = WaitingForProviderRegistered,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_provider_registered(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;
        let config = ctx.desired_resource_config::<ServiceActivation>()?;

        debug!(
            service_id = %config.id,
            service_name = %config.service_name,
            "Checking provider registration status"
        );

        // If service is already activated, transition to Ready
        if self.service_activated {
            info!(
                service_id = %config.id,
                service_name = %config.service_name,
                "Provider was already registered, transitioning to Ready"
            );
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        // Check the actual provider registration status
        match client.get_provider(&config.service_name).await {
            Ok(provider) => {
                if let Some(registration_state) = provider.registration_state {
                    if registration_state.to_lowercase() == "registered" {
                        info!(
                            service_id = %config.id,
                            service_name = %config.service_name,
                            "Provider is now registered"
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
                            current_state = %registration_state,
                            "Provider not yet registered"
                        );
                    }
                } else {
                    debug!(
                        service_id = %config.id,
                        service_name = %config.service_name,
                        "Provider registration state not available"
                    );
                }
            }
            Err(e) => {
                let msg = format!(
                    "Failed to get Azure provider '{}': {}",
                    config.service_name, e
                );
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: msg,
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        // Provider not yet registered, continue waiting
        Ok(HandlerAction::Continue {
            state: WaitingForProviderRegistered,
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
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<ServiceActivation>()?;

        // Heartbeat check: verify provider is still registered
        if let Some(service_name) = &self.service_name {
            let client = ctx
                .service_provider
                .get_azure_resources_client(azure_config)?;

            let provider =
                client
                    .get_provider(service_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get Azure provider '{}'", service_name),
                        resource_id: Some(config.id.clone()),
                    })?;

            if let Some(registration_state) = provider.registration_state {
                if registration_state.to_lowercase() != "registered" {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Provider registration state changed from Registered to {}",
                            registration_state
                        ),
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
        info!(id=%config.id, "Azure ServiceActivation update (no-op — service_name is immutable)");
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
            "Azure Service activation deletion requested - skipping (providers are not unregistered on delete)"
        );

        // We don't unregister providers on delete as it can be dangerous and break other resources
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

impl AzureServiceActivationController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(service_name: &str) -> Self {
        Self {
            state: AzureServiceActivationState::Ready,
            service_name: Some(service_name.to_string()),
            service_activated: true,
            _internal_stay_count: None,
        }
    }
}
