use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_aws_clients::sqs::SetQueueAttributesRequest;
use alien_core::{Queue, QueueOutputs, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::{controller, flow_entry, handler, terminal_state};

/// Generates the full, prefixed AWS queue name.
fn get_aws_queue_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[controller]
pub struct AwsQueueController {
    /// The SQS queue URL
    pub(crate) queue_url: Option<String>,
    /// The SQS queue name (physical)
    pub(crate) queue_name: Option<String>,
}

#[controller]
impl AwsQueueController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Queue>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_sqs_client(aws_cfg)?;

        let queue_name = get_aws_queue_name(ctx.resource_prefix, &config.id);
        info!(id=%config.id, name=%queue_name, "Creating SQS queue");

        let resp = client
            .create_queue(
                alien_aws_clients::sqs::CreateQueueRequest::builder()
                    .queue_name(queue_name.clone())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create SQS queue '{}'", queue_name),
                resource_id: Some(config.id.clone()),
            })?;

        let queue_url = resp.create_queue_result.queue_url;
        self.queue_url = Some(queue_url.clone());
        self.queue_name = Some(queue_name.clone());

        info!(url=%queue_url, "SQS queue created successfully");

        Ok(HandlerAction::Continue {
            state: ConfigureVisibilityTimeout,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ConfigureVisibilityTimeout,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configure_visibility_timeout(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_sqs_client(aws_cfg)?;
        let config = ctx.desired_resource_config::<Queue>()?;

        let queue_url = self.queue_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Queue URL not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Calculate visibility timeout based on functions in the stack that use this queue
        let desired_visibility_timeout =
            self.calculate_visibility_timeout_for_queue(ctx, &config)?;

        // Read current visibility timeout to avoid decreasing it unintentionally
        let current_visibility_timeout = match client
            .get_queue_attributes(
                queue_url,
                alien_aws_clients::sqs::GetQueueAttributesRequest::builder()
                    .attribute_names(vec!["VisibilityTimeout".to_string()])
                    .build(),
            )
            .await
        {
            Ok(resp) => resp
                .get_queue_attributes_result
                .attributes
                .into_iter()
                .find(|a| a.name == "VisibilityTimeout")
                .and_then(|a| a.value.parse::<u32>().ok())
                .unwrap_or(0),
            Err(_) => 0, // If fetch fails, proceed with desired value
        };

        // Only increase; never decrease to preserve compatibility with other consumers
        let visibility_timeout =
            std::cmp::max(current_visibility_timeout, desired_visibility_timeout);

        info!(
            queue=%config.id,
            visibility_timeout=%visibility_timeout,
            "Setting SQS queue visibility timeout based on stack analysis"
        );

        // VisibilityTimeout in seconds
        let mut attrs = std::collections::HashMap::new();
        attrs.insert(
            "VisibilityTimeout".to_string(),
            visibility_timeout.to_string(),
        );

        client
            .set_queue_attributes(
                queue_url,
                SetQueueAttributesRequest::builder()
                    .attributes(attrs)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to set visibility timeout for SQS queue '{}'",
                    queue_url
                ),
                resource_id: Some(config.id.clone()),
            })?;

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // Heartbeat: poll attributes to ensure queue exists
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_sqs_client(aws_cfg)?;
        let config = ctx.desired_resource_config::<Queue>()?;
        let queue_url = match &self.queue_url {
            Some(u) => u,
            None => {
                debug!(id=%config.id, "No queue URL set; skipping heartbeat");
                return Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: Some(Duration::from_secs(30)),
                });
            }
        };

        let _ = client
            .get_queue_attributes(
                queue_url,
                alien_aws_clients::sqs::GetQueueAttributesRequest::builder().build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get attributes for SQS queue '{}' during heartbeat",
                    queue_url
                ),
                resource_id: Some(config.id.clone()),
            })?;

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    // Queue has no mutable fields — update is a no-op that also recovers RefreshFailed.
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Queue>()?;
        info!(id=%config.id, "AWS Queue update (no-op — no mutable fields)");
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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_sqs_client(aws_cfg)?;
        let config = ctx.desired_resource_config::<Queue>()?;

        if let Some(queue_url) = &self.queue_url {
            info!(url=%queue_url, "Deleting SQS queue");
            match client.delete_queue(queue_url).await {
                Ok(_) => {
                    self.queue_url = None;
                    self.queue_name = None;
                }
                Err(e) => {
                    // Consider not found as successful deletion
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete SQS queue '{}'", queue_url),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        } else {
            info!(id=%config.id, "No SQS queue to delete");
        }

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

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(name), Some(url)) = (&self.queue_name, &self.queue_url) {
            Some(ResourceOutputs::new(QueueOutputs {
                queue_name: name.clone(),
                identifier: Some(url.clone()),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{BindingValue, QueueBinding};
        if let Some(url) = &self.queue_url {
            let binding = QueueBinding::sqs(BindingValue::value(url.clone()));
            serde_json::to_value(binding).ok()
        } else {
            None
        }
    }
}

impl AwsQueueController {
    /// Calculates the appropriate visibility timeout for this queue based on functions that use it
    /// Implements the formula from QUEUE.md: max(30s, min(12h, max_function_timeout * 6))
    fn calculate_visibility_timeout_for_queue(
        &self,
        ctx: &ResourceControllerContext<'_>,
        queue_config: &Queue,
    ) -> Result<u32> {
        use alien_core::{Function, FunctionTrigger};

        let mut max_function_timeout = 0u32;
        let mut functions_using_queue = 0;

        // Find all functions in the stack that have queue triggers referencing this queue
        for (_resource_id, resource) in &ctx.desired_stack.resources {
            if let Some(function) = resource.config.downcast_ref::<Function>() {
                // Check if this function has a queue trigger that references our queue
                for trigger in &function.triggers {
                    if let FunctionTrigger::Queue { queue } = trigger {
                        if queue.id == queue_config.id {
                            max_function_timeout =
                                max_function_timeout.max(function.timeout_seconds);
                            functions_using_queue += 1;
                            info!(
                                queue=%queue_config.id,
                                function=%function.id,
                                function_timeout=%function.timeout_seconds,
                                "Found function that uses this queue"
                            );
                            break; // A function should only have one trigger per queue
                        }
                    }
                }
            }
        }

        let visibility_timeout = if functions_using_queue > 0 {
            // Apply the QUEUE.md formula: max(30s, min(12h, max_function_timeout * 6))
            let calculated = max_function_timeout * 6;
            let min_visibility = 30; // 30 seconds minimum
            let max_visibility = 12 * 60 * 60; // 12 hours maximum

            std::cmp::max(min_visibility, std::cmp::min(max_visibility, calculated))
        } else {
            // Default visibility timeout when no functions use this queue
            30
        };

        info!(
            queue=%queue_config.id,
            functions_using_queue=%functions_using_queue,
            max_function_timeout=%max_function_timeout,
            calculated_visibility_timeout=%visibility_timeout,
            "Calculated queue visibility timeout based on stack analysis"
        );

        Ok(visibility_timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        MockPlatformServiceProvider, PlatformServiceProvider,
    };
    use alien_aws_clients::sqs::{
        CreateQueueResponse, CreateQueueResult, GetQueueAttributesResponse,
        GetQueueAttributesResult, MockSqsApi,
    };
    use alien_core::{Platform, Queue, ResourceStatus};
    use std::sync::Arc;

    fn setup_mock_sqs_for_create_and_delete(queue_url: &str) -> Arc<MockSqsApi> {
        let mut mock = MockSqsApi::new();

        mock.expect_create_queue().returning(|_req| {
            Ok(CreateQueueResponse {
                create_queue_result: CreateQueueResult {
                    queue_url: "https://sqs.us-east-1.amazonaws.com/123/test-q".to_string(),
                },
            })
        });

        mock.expect_set_queue_attributes().returning(|_, _| Ok(()));

        mock.expect_get_queue_attributes().returning(|_, _| {
            Ok(GetQueueAttributesResponse {
                get_queue_attributes_result: GetQueueAttributesResult { attributes: vec![] },
            })
        });

        mock.expect_delete_queue().returning(|_| Ok(()));

        Arc::new(mock)
    }

    fn setup_mock_provider(mock_sqs: Arc<MockSqsApi>) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_sqs_client()
            .returning(move |_| Ok(mock_sqs.clone()));
        Arc::new(provider)
    }

    #[tokio::test]
    async fn test_create_and_delete_queue_succeeds() {
        let queue = Queue::new("my-queue".to_string()).build();
        let mock_sqs =
            setup_mock_sqs_for_create_and_delete("https://sqs.us-east-1.amazonaws.com/123/test-q");
        let mock_provider = setup_mock_provider(mock_sqs);

        let mut executor = SingleControllerExecutor::builder()
            .resource(queue)
            .controller(AwsQueueController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }
}
