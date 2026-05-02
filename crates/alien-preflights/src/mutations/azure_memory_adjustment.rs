//! Adjusts function memory values to valid Azure Container Apps combinations.
//!
//! Azure Container Apps requires fixed CPU/memory pairs (512 MB minimum).
//! This mutation rounds up any invalid memory value to the nearest valid combo,
//! so users don't need to know platform-specific constraints.

use crate::azure;
use crate::error::Result;
use crate::StackMutation;
use alien_core::{DeploymentConfig, Function, Platform, Stack, StackState};
use async_trait::async_trait;
use tracing::{info, warn};

pub struct AzureMemoryAdjustmentMutation;

#[async_trait]
impl StackMutation for AzureMemoryAdjustmentMutation {
    fn description(&self) -> &'static str {
        "Adjust function memory to valid Azure Container Apps values"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        if stack_state.platform != Platform::Azure {
            return false;
        }

        // Run if any function has an invalid Azure memory value
        stack.resources.values().any(|entry| {
            entry
                .config
                .downcast_ref::<Function>()
                .is_some_and(|f| !azure::is_valid_memory(f.memory_mb))
        })
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        for (id, entry) in &mut stack.resources {
            let Some(func) = entry.config.downcast_mut::<Function>() else {
                continue;
            };

            if azure::is_valid_memory(func.memory_mb) {
                continue;
            }

            let original = func.memory_mb;
            match azure::nearest_valid_memory(original) {
                Some(adjusted) => {
                    warn!(
                        function = %id,
                        original_mb = original,
                        adjusted_mb = adjusted,
                        "Adjusted function memory to nearest valid Azure Container Apps value"
                    );
                    func.memory_mb = adjusted;
                }
                None => {
                    // Above max — compile-time check already errors on this,
                    // so this path shouldn't be reached in practice
                    info!(
                        function = %id,
                        memory_mb = original,
                        "Function memory exceeds Azure maximum, leaving unchanged for validation to catch"
                    );
                }
            }
        }

        Ok(stack)
    }
}
