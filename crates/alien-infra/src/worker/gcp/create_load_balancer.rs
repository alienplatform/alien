use super::*;
use super::{GcpWorkerHandlerAction as HandlerAction, GcpWorkerState::*};

impl GcpWorkerController {
    pub(super) async fn creating_serverless_neg_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.serverless_neg_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForServerlessNeg
            } else {
                CreatingBackendService
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service name not set".to_string(),
            })
        })?;

        let neg_name = get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "neg");

        // Create serverless NEG pointing to Cloud Run service
        // According to GCP API: https://docs.cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups
        // For serverless NEGs, we must specify cloud_run, app_engine, or cloud_function
        let cloud_run_config = NetworkEndpointGroupCloudRun::builder()
            .service(service_name.clone())
            .build();

        let neg = NetworkEndpointGroup::builder()
            .name(neg_name.clone())
            .description(format!("Serverless NEG for worker {}", worker_config.id))
            .network_endpoint_type(NetworkEndpointType::Serverless)
            .cloud_run(cloud_run_config)
            .build();

        let operation = compute_client
            .insert_region_network_endpoint_group(gcp_config.region.clone(), neg)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create serverless NEG".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.serverless_neg_name = Some(neg_name);
        self.record_compute_operation(
            operation,
            Some(gcp_config.region.clone()),
            &worker_config.id,
            "serverless NEG creation",
        )?;

        info!(
            worker=%worker_config.id,
            neg_name=%self.serverless_neg_name.as_ref().unwrap(),
            "Serverless NEG created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForServerlessNeg,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    pub(super) async fn waiting_for_serverless_neg_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "serverless NEG creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingBackendService,
            suggested_delay: None,
        })
    }

    pub(super) async fn creating_backend_service_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.backend_service_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForBackendService
            } else {
                CreatingUrlMap
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let neg_name = self.serverless_neg_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Serverless NEG name not set".to_string(),
            })
        })?;

        let backend_service_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "backend");

        let neg_url = format!(
            "projects/{}/regions/{}/networkEndpointGroups/{}",
            gcp_config.project_id, gcp_config.region, neg_name
        );

        // Create backend service with serverless NEG (no health check for serverless)
        let backend_service = BackendService::builder()
            .name(backend_service_name.clone())
            .description(format!("Backend service for worker {}", worker_config.id))
            .protocol(BackendServiceProtocol::Https)
            .load_balancing_scheme(LoadBalancingScheme::External)
            .backends(vec![Backend::builder()
                .group(neg_url)
                .balancing_mode(BalancingMode::Utilization)
                .build()])
            .build();

        let operation = compute_client
            .insert_backend_service(backend_service)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create backend service".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.backend_service_name = Some(backend_service_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "backend service creation",
        )?;

        info!(
            worker=%worker_config.id,
            backend_service_name=%self.backend_service_name.as_ref().unwrap(),
            "Backend service created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForBackendService,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    pub(super) async fn waiting_for_backend_service_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "backend service creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingUrlMap,
            suggested_delay: None,
        })
    }

    pub(super) async fn creating_url_map_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.url_map_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForUrlMap
            } else {
                CreatingTargetHttpsProxy
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let backend_service_name = self.backend_service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Backend service name not set".to_string(),
            })
        })?;

        let url_map_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "urlmap");

        let backend_service_url = format!(
            "projects/{}/global/backendServices/{}",
            gcp_config.project_id, backend_service_name
        );

        // Create URL map routing to backend service
        let url_map = UrlMap::builder()
            .name(url_map_name.clone())
            .description(format!("URL map for worker {}", worker_config.id))
            .default_service(backend_service_url)
            .build();

        let operation = compute_client.insert_url_map(url_map).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to create URL map".to_string(),
                resource_id: Some(worker_config.id.clone()),
            },
        )?;

        self.url_map_name = Some(url_map_name);
        self.record_compute_operation(operation, None, &worker_config.id, "URL map creation")?;

        info!(
            worker=%worker_config.id,
            url_map_name=%self.url_map_name.as_ref().unwrap(),
            "URL map created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForUrlMap,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    pub(super) async fn waiting_for_url_map_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "URL map creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingTargetHttpsProxy,
            suggested_delay: None,
        })
    }

    pub(super) async fn creating_target_https_proxy_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.target_https_proxy_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForTargetHttpsProxy
            } else {
                CreatingGlobalAddress
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let url_map_name = self.url_map_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "URL map name not set".to_string(),
            })
        })?;

        let ssl_cert_name = self.ssl_certificate_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "SSL certificate name not set".to_string(),
            })
        })?;

        let proxy_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "https-proxy");

        let url_map_url = format!(
            "projects/{}/global/urlMaps/{}",
            gcp_config.project_id, url_map_name
        );

        let ssl_cert_url = format!(
            "projects/{}/global/sslCertificates/{}",
            gcp_config.project_id, ssl_cert_name
        );

        // Create HTTPS proxy with SSL certificate
        let https_proxy = TargetHttpsProxy::builder()
            .name(proxy_name.clone())
            .description(format!("HTTPS proxy for worker {}", worker_config.id))
            .url_map(url_map_url)
            .ssl_certificates(vec![ssl_cert_url])
            .build();

        let operation = compute_client
            .insert_target_https_proxy(https_proxy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create target HTTPS proxy".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.target_https_proxy_name = Some(proxy_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "target HTTPS proxy creation",
        )?;

        info!(
            worker=%worker_config.id,
            proxy_name=%self.target_https_proxy_name.as_ref().unwrap(),
            "Target HTTPS proxy created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForTargetHttpsProxy,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    pub(super) async fn waiting_for_target_https_proxy_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "target HTTPS proxy creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingGlobalAddress,
            suggested_delay: None,
        })
    }

    pub(super) async fn creating_global_address_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.global_address_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForGlobalAddress
            } else {
                CreatingForwardingRule
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let address_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "ip");

        // Create global static IP address
        let address = Address::builder()
            .name(address_name.clone())
            .description(format!("Global IP for worker {}", worker_config.id))
            .address_type(AddressType::External)
            .build();

        let operation = compute_client
            .insert_global_address(address)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create global address".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.global_address_name = Some(address_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "global address creation",
        )?;

        info!(
            worker=%worker_config.id,
            address_name=%self.global_address_name.as_ref().unwrap(),
            "Global address created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForGlobalAddress,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    pub(super) async fn waiting_for_global_address_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "global address creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingForwardingRule,
            suggested_delay: None,
        })
    }

    pub(super) async fn creating_forwarding_rule_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.forwarding_rule_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForForwardingRule
            } else {
                WaitingForDns
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let proxy_name = self.target_https_proxy_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Target HTTPS proxy name not set".to_string(),
            })
        })?;

        let address_name = self.global_address_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Global address name not set".to_string(),
            })
        })?;

        let ip_address = self
            .ensure_global_address_ip(ctx, &worker_config.id, &address_name)
            .await?;

        let forwarding_rule_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "https");

        let proxy_url = format!(
            "projects/{}/global/targetHttpsProxies/{}",
            gcp_config.project_id, proxy_name
        );

        // Create forwarding rule exposing HTTPS endpoint
        let forwarding_rule = ForwardingRule::builder()
            .name(forwarding_rule_name.clone())
            .description(format!("Forwarding rule for worker {}", worker_config.id))
            .ip_address(ip_address)
            .ip_protocol(ForwardingRuleProtocol::Tcp)
            .port_range("443-443".to_string())
            .target(proxy_url)
            .load_balancing_scheme(LoadBalancingScheme::External)
            .build();

        let operation = compute_client
            .insert_global_forwarding_rule(forwarding_rule)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create forwarding rule".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.forwarding_rule_name = Some(forwarding_rule_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "forwarding rule creation",
        )?;

        info!(
            worker=%worker_config.id,
            forwarding_rule_name=%self.forwarding_rule_name.as_ref().unwrap(),
            "Forwarding rule created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForForwardingRule,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    pub(super) async fn waiting_for_forwarding_rule_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "forwarding rule creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: WaitingForDns,
            suggested_delay: None,
        })
    }

    pub(super) async fn waiting_for_dns_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if let Some(address_name) = self.global_address_name.clone() {
            self.ensure_global_address_ip(ctx, &worker_config.id, &address_name)
                .await?;
        }

        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id));

        let status = metadata.map(|m| &m.dns_status);

        match status {
            Some(DnsRecordStatus::Active) => {
                info!(
                    worker=%worker_config.id,
                    fqdn=%self.fqdn.as_ref().unwrap_or(&"unknown".to_string()),
                    "DNS record created successfully"
                );
                Ok(HandlerAction::Continue {
                    state: CreatingPushSubscriptions,
                    suggested_delay: None,
                })
            }
            Some(DnsRecordStatus::Failed) => {
                let fqdn = metadata.map(|m| m.fqdn.as_str()).unwrap_or("unknown");
                let detail = metadata
                    .and_then(|m| m.dns_error.as_deref())
                    .unwrap_or("unknown error");
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("DNS record creation failed for {fqdn}: {detail}"),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
    }
}
