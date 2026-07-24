use super::*;
use super::{GcpWorkerHandlerAction as HandlerAction, GcpWorkerState::*};

impl GcpWorkerController {
    pub(super) async fn update_creating_serverless_neg_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_serverless_neg(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForServerlessNeg,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForServerlessNeg,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingBackendService,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingBackendService,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_serverless_neg",
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

    pub(super) async fn update_waiting_for_serverless_neg_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_serverless_neg(ctx).await? {
            HandlerAction::Continue {
                state: CreatingBackendService,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingBackendService,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_serverless_neg",
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

    pub(super) async fn update_creating_backend_service_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_backend_service(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForBackendService,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForBackendService,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingUrlMap,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingUrlMap,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_backend_service",
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

    pub(super) async fn update_waiting_for_backend_service_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_backend_service(ctx).await? {
            HandlerAction::Continue {
                state: CreatingUrlMap,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingUrlMap,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_backend_service",
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

    pub(super) async fn update_creating_url_map_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_url_map(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForUrlMap,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForUrlMap,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingTargetHttpsProxy,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingTargetHttpsProxy,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_url_map",
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

    pub(super) async fn update_waiting_for_url_map_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_url_map(ctx).await? {
            HandlerAction::Continue {
                state: CreatingTargetHttpsProxy,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingTargetHttpsProxy,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_url_map",
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

    pub(super) async fn update_creating_target_https_proxy_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_target_https_proxy(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForTargetHttpsProxy,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForTargetHttpsProxy,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingGlobalAddress,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingGlobalAddress,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_target_https_proxy",
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

    pub(super) async fn update_waiting_for_target_https_proxy_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_target_https_proxy(ctx).await? {
            HandlerAction::Continue {
                state: CreatingGlobalAddress,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingGlobalAddress,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_target_https_proxy",
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

    pub(super) async fn update_creating_global_address_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_global_address(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForGlobalAddress,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForGlobalAddress,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingForwardingRule,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingForwardingRule,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_global_address",
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

    pub(super) async fn update_waiting_for_global_address_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_global_address(ctx).await? {
            HandlerAction::Continue {
                state: CreatingForwardingRule,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingForwardingRule,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_global_address",
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

    pub(super) async fn update_creating_forwarding_rule_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_forwarding_rule(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForForwardingRule,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForForwardingRule,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForDns,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_forwarding_rule",
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

    pub(super) async fn update_waiting_for_forwarding_rule_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_forwarding_rule(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForDns,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_forwarding_rule",
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
                state: CreatingPushSubscriptions,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
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
}
