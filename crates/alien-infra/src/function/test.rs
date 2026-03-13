use alien_error::AlienError;
use std::time::Duration;
use tracing::{info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{Function, FunctionOutputs, ResourceOutputs, ResourceStatus};
use alien_macros::controller;

pub(crate) const CREATE_POLL_COUNT: u32 = 3; // Reduced from 5 for faster tests
pub(crate) const UPDATE_POLL_COUNT: u32 = 2; // Reduced from 3
pub(crate) const DELETE_POLL_COUNT: u32 = 2; // Reduced from 3
pub(crate) const POLL_DELAY_MS: u64 = 50; // Short delay for tests

#[controller]
pub struct TestFunctionController {
    /// The identifier of the Test function, available after creation.
    pub(crate) identifier: Option<String>,
    /// The function URL, available after URL creation.
    pub(crate) url: Option<String>,
    /// Tracks the number of retryable failures that have occurred for this instance.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) retryable_failures_count: u32,
    /// The target number of retryable failures before succeeding (from SIMULATE_RETRYABLE_FAILURE_COUNT).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) retryable_failure_target: Option<u32>,
    /// Tracks the number of transient failures attempted for this instance.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) transient_failures_attempted: u32,
    /// The target number of transient failures to simulate for this instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) transient_failure_config: Option<u32>,
    /// Records the highest number of transient failures attempted for reporting/testing.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) transient_failures_recorded: u32,
    /// Polling counter for create function
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) create_poll_count: u32,
    /// Polling counter for update code
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) update_code_poll_count: u32,
    /// Polling counter for update config
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) update_config_poll_count: u32,
    /// Polling counter for delete
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) delete_poll_count: u32,
}

#[controller]
impl TestFunctionController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let target_func = ctx.desired_resource_config::<Function>()?;

        println!(
            "→ [test-create] Handling CreateStart for `{}`",
            target_func.id
        );

        // --- Initialize retryable failure configuration ---
        if self.retryable_failure_target.is_none() {
            // Check for count-based retryable failures
            if let Some(count_str) = target_func
                .environment
                .get("SIMULATE_RETRYABLE_FAILURE_COUNT")
            {
                if let Ok(count) = count_str.parse::<u32>() {
                    self.retryable_failure_target = Some(count);
                }
            }
            // Check for single retryable failure (backwards compatibility)
            else if target_func
                .environment
                .get("SIMULATE_RETRYABLE_FAILURE")
                .is_some_and(|v| v == "true")
            {
                self.retryable_failure_target = Some(1);
            }
        }

        // --- Simulate retryable failures based on configuration ---
        if let Some(target_failures) = self.retryable_failure_target {
            if self.retryable_failures_count < target_failures {
                self.retryable_failures_count += 1;
                println!(
                    "→ [test-create] TRIGGERING SIMULATED RETRYABLE FAILURE {}/{} for {}",
                    self.retryable_failures_count, target_failures, target_func.id
                );
                return Err(AlienError::new(ErrorData::ExecutionStepFailed {
                    message: format!(
                        "Simulated retryable failure {}/{}",
                        self.retryable_failures_count, target_failures
                    ),
                    resource_id: Some(target_func.id.clone()),
                }));
            }
        }

        // Check for persistent failure mode (fails continuously)
        if target_func
            .environment
            .get("SIMULATE_PERSISTENT_FAILURE")
            .is_some_and(|v| v == "true")
        {
            println!(
                "→ [test-create] TRIGGERING SIMULATED PERSISTENT FAILURE for {}",
                target_func.id
            );
            return Err(AlienError::new(ErrorData::ExecutionStepFailed {
                message: "Simulated persistent failure".to_string(),
                resource_id: Some(target_func.id.clone()),
            }));
        }

        // Initialize transient failure config from environment
        if self.transient_failure_config.is_none() {
            self.transient_failure_config = target_func
                .environment
                .get("SIMULATE_TRANSIENT_FAILURE_COUNT")
                .and_then(|v| v.parse::<u32>().ok());
        }

        // --- Simulate transient failure based on internal counter ---
        if let Some(target_failures) = self.transient_failure_config {
            if self.transient_failures_attempted < target_failures {
                warn!(
                    "!!! SIMULATING TRANSIENT FAILURE (Attempt {}/{}) for {} !!!",
                    self.transient_failures_attempted + 1,
                    target_failures,
                    target_func.id
                );
                self.transient_failures_attempted += 1;
                // Record the maximum attempt number reached
                self.transient_failures_recorded = self
                    .transient_failures_recorded
                    .max(self.transient_failures_attempted);

                return Ok(HandlerAction::Continue {
                    state: CreateStart,
                    suggested_delay: Some(Duration::from_millis(POLL_DELAY_MS)),
                });
            }
        }

        // --- Simulate config-based failure (memory limit) ---
        if target_func.memory_mb > 4096 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Simulated failure: Memory {}MB exceeds 4096MB limit",
                    target_func.memory_mb
                ),
                resource_id: Some(target_func.id.clone()),
            }));
        }

        info!(
            "   Attempting CreateFunction with Memory: {}",
            target_func.memory_mb
        );

        // Create the function identifier
        self.identifier = Some(format!("test:function:{}", target_func.id));
        self.transient_failures_attempted = 0; // Reset for next state

        Ok(HandlerAction::Continue {
            state: CreateFunction,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreateFunction,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_function(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let target_func = ctx.desired_resource_config::<Function>()?;
        let identifier = self.identifier.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: target_func.id.clone(),
                message: "Identifier missing in CreateFunction state".to_string(),
            })
        })?;

        info!(
            "→ [test-create] Start polling (0/{}) for function readiness `{}`",
            CREATE_POLL_COUNT, identifier
        );

        self.create_poll_count = 0;

        Ok(HandlerAction::Continue {
            state: CreateFunctionPolling,
            suggested_delay: Some(Duration::from_millis(POLL_DELAY_MS)),
        })
    }

    #[handler(
        state = CreateFunctionPolling,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_function_polling(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let target_func = ctx.desired_resource_config::<Function>()?;
        let identifier = self.identifier.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: target_func.id.clone(),
                message: "Identifier missing in CreateFunctionPolling state".to_string(),
            })
        })?;

        // Support Stay-based exhaustion simulation for testing Bug 1/2 fixes.
        // When set, the handler always returns Stay and never advances, so the
        // macro's exhaustion path is exercised rather than the manual poll counter.
        if let Some(max_times_str) = target_func.environment.get("SIMULATE_STAY_EXHAUSTION") {
            let max_times: u32 = max_times_str.parse().unwrap_or(3);
            info!(
                "→ [test-create] SIMULATE_STAY_EXHAUSTION active (max_times={}) for `{}`",
                max_times, identifier
            );
            return Ok(HandlerAction::Stay {
                max_times,
                suggested_delay: Some(Duration::from_millis(POLL_DELAY_MS)),
            });
        }

        self.create_poll_count += 1;

        if self.create_poll_count <= CREATE_POLL_COUNT {
            info!(
                "→ [test-create] Polling ({}/{}) for function readiness `{}`",
                self.create_poll_count, CREATE_POLL_COUNT, identifier
            );

            Ok(HandlerAction::Continue {
                state: CreateFunctionPolling,
                suggested_delay: Some(Duration::from_millis(POLL_DELAY_MS)),
            })
        } else {
            info!(
                "→ [test-create] Polling complete ({}/{}), function `{}` is active (simulated), proceeding to CreateUrl",
                self.create_poll_count, CREATE_POLL_COUNT, identifier
            );

            Ok(HandlerAction::Continue {
                state: CreateUrl,
                suggested_delay: None,
            })
        }
    }

    #[handler(
        state = CreateUrl,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_url(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let target_func = ctx.desired_resource_config::<Function>()?;
        let identifier = self.identifier.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: target_func.id.clone(),
                message: "Identifier missing in CreateUrl state".to_string(),
            })
        })?;

        info!("→ [test-create] CreateFunctionUrl `{}`", identifier);

        self.url = Some(format!("https://test-url/{}", target_func.id));

        let url = self.url.as_ref().unwrap();
        info!("✓ [test-create] `{}` created at {}", identifier, url);

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = CreateFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // The system will automatically know if config changed and transition to update
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let target_func = ctx.desired_resource_config::<Function>()?;
        let identifier = self.identifier.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: target_func.id.clone(),
                message: "Identifier missing in UpdateStart state".to_string(),
            })
        })?;

        info!(
            "→ [test-update] Start UpdateCode polling (0/{}) `{}`",
            UPDATE_POLL_COUNT, identifier
        );

        self.update_code_poll_count = 0;

        Ok(HandlerAction::Continue {
            state: UpdateCodePolling,
            suggested_delay: Some(Duration::from_millis(POLL_DELAY_MS)),
        })
    }

    #[handler(
        state = UpdateCodePolling,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_code_polling(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let target_func = ctx.desired_resource_config::<Function>()?;
        let identifier = self.identifier.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: target_func.id.clone(),
                message: "Identifier missing in UpdateCodePolling state".to_string(),
            })
        })?;

        self.update_code_poll_count += 1;

        if self.update_code_poll_count <= UPDATE_POLL_COUNT {
            info!(
                "→ [test-update] Polling UpdateCode ({}/{}) `{}`",
                self.update_code_poll_count, UPDATE_POLL_COUNT, identifier
            );

            Ok(HandlerAction::Continue {
                state: UpdateCodePolling,
                suggested_delay: Some(Duration::from_millis(POLL_DELAY_MS)),
            })
        } else {
            info!(
                "→ [test-update] UpdateCode polling complete ({}/{}), proceeding to UpdateConfig polling (0/{}) `{}`",
                self.update_code_poll_count, UPDATE_POLL_COUNT, UPDATE_POLL_COUNT, identifier
            );

            self.update_config_poll_count = 0;

            Ok(HandlerAction::Continue {
                state: UpdateConfigPolling,
                suggested_delay: Some(Duration::from_millis(POLL_DELAY_MS)),
            })
        }
    }

    #[handler(
        state = UpdateConfigPolling,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_config_polling(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let target_func = ctx.desired_resource_config::<Function>()?;
        let identifier = self.identifier.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: target_func.id.clone(),
                message: "Identifier missing in UpdateConfigPolling state".to_string(),
            })
        })?;

        self.update_config_poll_count += 1;

        if self.update_config_poll_count <= UPDATE_POLL_COUNT {
            info!(
                "→ [test-update] Polling UpdateConfig ({}/{}) `{}`",
                self.update_config_poll_count, UPDATE_POLL_COUNT, identifier
            );

            Ok(HandlerAction::Continue {
                state: UpdateConfigPolling,
                suggested_delay: Some(Duration::from_millis(POLL_DELAY_MS)),
            })
        } else {
            info!(
                "→ [test-update] UpdateConfig polling complete ({}/{}), update finished for `{}`",
                self.update_config_poll_count, UPDATE_POLL_COUNT, identifier
            );

            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            })
        }
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        // If no identifier exists, the resource was never created, go directly to deleted
        if self.identifier.is_none() {
            info!("Resource failed before creation, marking as Deleted.");
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        let identifier = self.identifier.as_ref().unwrap();
        info!(
            "→ [test-delete] Start Delete polling (0/{}) `{}`",
            DELETE_POLL_COUNT, identifier
        );

        self.delete_poll_count = 0;

        Ok(HandlerAction::Continue {
            state: DeleteFunctionPolling,
            suggested_delay: Some(Duration::from_millis(POLL_DELAY_MS)),
        })
    }

    #[handler(
        state = DeleteFunctionPolling,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_function_polling(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let target_func = ctx.desired_resource_config::<Function>()?;
        let identifier = self.identifier.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: target_func.id.clone(),
                message: "Identifier missing in DeleteFunctionPolling state".to_string(),
            })
        })?;

        self.delete_poll_count += 1;

        if self.delete_poll_count <= DELETE_POLL_COUNT {
            info!(
                "→ [test-delete] Polling Delete ({}/{}) `{}`",
                self.delete_poll_count, DELETE_POLL_COUNT, identifier
            );

            Ok(HandlerAction::Continue {
                state: DeleteFunctionPolling,
                suggested_delay: Some(Duration::from_millis(POLL_DELAY_MS)),
            })
        } else {
            info!(
                "→ [test-delete] Delete polling complete ({}/{}), `{}` deleted",
                self.delete_poll_count, DELETE_POLL_COUNT, identifier
            );

            Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            })
        }
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
}

// Separate impl block for helper methods and get_outputs override
impl TestFunctionController {
    // Override the generated get_outputs method
    pub fn get_outputs(&self) -> Option<ResourceOutputs> {
        if matches!(self.state, TestFunctionState::Deleted) {
            return None;
        }

        // Outputs are generally available once identifier exists
        if let Some(identifier) = &self.identifier {
            let url = self.url.as_ref().filter(|u| !u.is_empty());
            Some(ResourceOutputs::new(FunctionOutputs {
                function_name: identifier.clone(),
                url: url.cloned(),
                identifier: Some(identifier.clone()),
                load_balancer_endpoint: None, // Test functions don't have load balancers
            }))
        } else {
            None
        }
    }
}

/// Helper function for skip_serializing_if
fn is_zero(n: &u32) -> bool {
    *n == 0
}
