use super::*;
use super::{AwsWorkerHandlerAction as HandlerAction, AwsWorkerState::*};

impl AwsWorkerController {
    pub(super) async fn update_importing_initial_certificate_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.importing_certificate(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiGateway,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "importing_certificate",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    pub(super) async fn update_creating_api_gateway_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_gateway(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiIntegration,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiIntegration,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_gateway",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    pub(super) async fn update_creating_api_integration_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_integration(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiRoute,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiRoute,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_integration",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    pub(super) async fn update_creating_api_route_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_route(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiStage,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiStage,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_route",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    pub(super) async fn update_creating_api_stage_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_stage(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiDomain,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiDomain,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateAddingApiGatewayPermission,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_stage",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    pub(super) async fn update_creating_api_domain_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_domain(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiMapping,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiMapping,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_domain",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    pub(super) async fn update_creating_api_mapping_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_mapping(ctx).await? {
            HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateAddingApiGatewayPermission,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_mapping",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    pub(super) async fn update_adding_api_gateway_permission_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.adding_api_gateway_permission(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForDns,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: ApplyingResourcePermissions,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateApplyingResourcePermissions,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "adding_api_gateway_permission",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    pub(super) async fn update_waiting_for_dns_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_dns(ctx).await? {
            HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_dns",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    pub(super) async fn update_running_readiness_probe_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Only run readiness probe if configured and we have a URL (for public workers)
        if worker_config.readiness_probe.is_some() && !worker_config.public_endpoints.is_empty() {
            if let Some(url) = &self.url {
                let dns_override = readiness_probe_dns_override(
                    url,
                    self.fqdn.as_deref(),
                    self.load_balancer.as_ref(),
                );

                match run_readiness_probe_with_dns_override(ctx, url, dns_override).await {
                    Ok(()) => {
                        // Probe succeeded, proceed to Ready
                    }
                    Err(_) => {
                        // Probe failed, let the framework handle retries
                        return Ok(HandlerAction::Stay {
                            max_times: Some(READINESS_PROBE_MAX_ATTEMPTS),
                            suggested_delay: Some(Duration::from_secs(5)),
                        });
                    }
                }
            }
        }

        // Either no readiness probe needed, or probe succeeded.
        Ok(HandlerAction::Continue {
            state: UpdateApplyingResourcePermissions,
            suggested_delay: None,
        })
    }
}
