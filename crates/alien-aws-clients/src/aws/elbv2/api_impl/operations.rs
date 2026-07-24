use super::*;

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Elbv2Api for Elbv2Client {
    // ---------------------------------------------------------------------------
    // Load Balancer Operations
    // ---------------------------------------------------------------------------

    async fn create_load_balancer(
        &self,
        request: CreateLoadBalancerRequest,
    ) -> Result<CreateLoadBalancerResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateLoadBalancer".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("Name".to_string(), request.name.clone());

        for (i, subnet) in request.subnets.iter().enumerate() {
            form_data.insert(format!("Subnets.member.{}", i + 1), subnet.clone());
        }

        if let Some(ref subnet_mappings) = request.subnet_mappings {
            for (i, mapping) in subnet_mappings.iter().enumerate() {
                let idx = i + 1;
                form_data.insert(
                    format!("SubnetMappings.member.{}.SubnetId", idx),
                    mapping.subnet_id.clone(),
                );
                if let Some(ref allocation_id) = mapping.allocation_id {
                    form_data.insert(
                        format!("SubnetMappings.member.{}.AllocationId", idx),
                        allocation_id.clone(),
                    );
                }
                if let Some(ref private_ipv4_address) = mapping.private_ipv4_address {
                    form_data.insert(
                        format!("SubnetMappings.member.{}.PrivateIPv4Address", idx),
                        private_ipv4_address.clone(),
                    );
                }
            }
        }

        if let Some(ref security_groups) = request.security_groups {
            for (i, sg) in security_groups.iter().enumerate() {
                form_data.insert(format!("SecurityGroups.member.{}", i + 1), sg.clone());
            }
        }

        if let Some(ref scheme) = request.scheme {
            form_data.insert("Scheme".to_string(), scheme.clone());
        }

        if let Some(ref lb_type) = request.load_balancer_type {
            form_data.insert("Type".to_string(), lb_type.clone());
        }

        if let Some(ref ip_address_type) = request.ip_address_type {
            form_data.insert("IpAddressType".to_string(), ip_address_type.clone());
        }

        if let Some(ref tags) = request.tags {
            Self::add_tags(&mut form_data, tags);
        }

        self.send_form(form_data, "CreateLoadBalancer", &request.name)
            .await
    }

    async fn describe_load_balancers(
        &self,
        request: DescribeLoadBalancersRequest,
    ) -> Result<DescribeLoadBalancersResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeLoadBalancers".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());

        if let Some(ref arns) = request.load_balancer_arns {
            for (i, arn) in arns.iter().enumerate() {
                form_data.insert(format!("LoadBalancerArns.member.{}", i + 1), arn.clone());
            }
        }

        if let Some(ref names) = request.names {
            for (i, name) in names.iter().enumerate() {
                form_data.insert(format!("Names.member.{}", i + 1), name.clone());
            }
        }

        if let Some(ref marker) = request.marker {
            form_data.insert("Marker".to_string(), marker.clone());
        }

        if let Some(page_size) = request.page_size {
            form_data.insert("PageSize".to_string(), page_size.to_string());
        }

        self.send_form(form_data, "DescribeLoadBalancers", "LoadBalancer")
            .await
    }

    async fn modify_load_balancer_attributes(
        &self,
        request: ModifyLoadBalancerAttributesRequest,
    ) -> Result<ModifyLoadBalancerAttributesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "ModifyLoadBalancerAttributes".to_string(),
        );
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "LoadBalancerArn".to_string(),
            request.load_balancer_arn.clone(),
        );
        for (index, attribute) in request.attributes.iter().enumerate() {
            form_data.insert(
                format!("Attributes.member.{}.Key", index + 1),
                attribute.key.clone(),
            );
            form_data.insert(
                format!("Attributes.member.{}.Value", index + 1),
                attribute.value.clone(),
            );
        }

        self.send_form(
            form_data,
            "ModifyLoadBalancerAttributes",
            &request.load_balancer_arn,
        )
        .await
    }

    async fn delete_load_balancer(&self, load_balancer_arn: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteLoadBalancer".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("LoadBalancerArn".to_string(), load_balancer_arn.to_string());

        self.send_form_no_body(form_data, "DeleteLoadBalancer", load_balancer_arn)
            .await
    }

    // ---------------------------------------------------------------------------
    // Target Group Operations
    // ---------------------------------------------------------------------------

    async fn create_target_group(
        &self,
        request: CreateTargetGroupRequest,
    ) -> Result<CreateTargetGroupResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateTargetGroup".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("Name".to_string(), request.name.clone());

        if let Some(ref protocol) = request.protocol {
            form_data.insert("Protocol".to_string(), protocol.clone());
        }

        if let Some(ref protocol_version) = request.protocol_version {
            form_data.insert("ProtocolVersion".to_string(), protocol_version.clone());
        }

        if let Some(port) = request.port {
            form_data.insert("Port".to_string(), port.to_string());
        }

        if let Some(ref vpc_id) = request.vpc_id {
            form_data.insert("VpcId".to_string(), vpc_id.clone());
        }

        if let Some(ref health_check_protocol) = request.health_check_protocol {
            form_data.insert(
                "HealthCheckProtocol".to_string(),
                health_check_protocol.clone(),
            );
        }

        if let Some(ref health_check_port) = request.health_check_port {
            form_data.insert("HealthCheckPort".to_string(), health_check_port.clone());
        }

        if let Some(health_check_enabled) = request.health_check_enabled {
            form_data.insert(
                "HealthCheckEnabled".to_string(),
                health_check_enabled.to_string(),
            );
        }

        if let Some(ref health_check_path) = request.health_check_path {
            form_data.insert("HealthCheckPath".to_string(), health_check_path.clone());
        }

        if let Some(health_check_interval_seconds) = request.health_check_interval_seconds {
            form_data.insert(
                "HealthCheckIntervalSeconds".to_string(),
                health_check_interval_seconds.to_string(),
            );
        }

        if let Some(health_check_timeout_seconds) = request.health_check_timeout_seconds {
            form_data.insert(
                "HealthCheckTimeoutSeconds".to_string(),
                health_check_timeout_seconds.to_string(),
            );
        }

        if let Some(healthy_threshold_count) = request.healthy_threshold_count {
            form_data.insert(
                "HealthyThresholdCount".to_string(),
                healthy_threshold_count.to_string(),
            );
        }

        if let Some(unhealthy_threshold_count) = request.unhealthy_threshold_count {
            form_data.insert(
                "UnhealthyThresholdCount".to_string(),
                unhealthy_threshold_count.to_string(),
            );
        }

        if let Some(ref matcher) = request.matcher {
            if let Some(ref http_code) = matcher.http_code {
                form_data.insert("Matcher.HttpCode".to_string(), http_code.clone());
            }
            if let Some(ref grpc_code) = matcher.grpc_code {
                form_data.insert("Matcher.GrpcCode".to_string(), grpc_code.clone());
            }
        }

        if let Some(ref target_type) = request.target_type {
            form_data.insert("TargetType".to_string(), target_type.clone());
        }

        if let Some(ref ip_address_type) = request.ip_address_type {
            form_data.insert("IpAddressType".to_string(), ip_address_type.clone());
        }

        if let Some(ref tags) = request.tags {
            Self::add_tags(&mut form_data, tags);
        }

        self.send_form(form_data, "CreateTargetGroup", &request.name)
            .await
    }

    async fn describe_target_groups(
        &self,
        request: DescribeTargetGroupsRequest,
    ) -> Result<DescribeTargetGroupsResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeTargetGroups".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());

        if let Some(ref lb_arn) = request.load_balancer_arn {
            form_data.insert("LoadBalancerArn".to_string(), lb_arn.clone());
        }

        if let Some(ref arns) = request.target_group_arns {
            for (i, arn) in arns.iter().enumerate() {
                form_data.insert(format!("TargetGroupArns.member.{}", i + 1), arn.clone());
            }
        }

        if let Some(ref names) = request.names {
            for (i, name) in names.iter().enumerate() {
                form_data.insert(format!("Names.member.{}", i + 1), name.clone());
            }
        }

        if let Some(ref marker) = request.marker {
            form_data.insert("Marker".to_string(), marker.clone());
        }

        if let Some(page_size) = request.page_size {
            form_data.insert("PageSize".to_string(), page_size.to_string());
        }

        self.send_form(form_data, "DescribeTargetGroups", "TargetGroup")
            .await
    }

    async fn modify_target_group(
        &self,
        request: ModifyTargetGroupRequest,
    ) -> Result<ModifyTargetGroupResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "ModifyTargetGroup".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "TargetGroupArn".to_string(),
            request.target_group_arn.clone(),
        );

        if let Some(ref health_check_protocol) = request.health_check_protocol {
            form_data.insert(
                "HealthCheckProtocol".to_string(),
                health_check_protocol.clone(),
            );
        }

        if let Some(ref health_check_port) = request.health_check_port {
            form_data.insert("HealthCheckPort".to_string(), health_check_port.clone());
        }

        if let Some(ref health_check_path) = request.health_check_path {
            form_data.insert("HealthCheckPath".to_string(), health_check_path.clone());
        }

        if let Some(health_check_enabled) = request.health_check_enabled {
            form_data.insert(
                "HealthCheckEnabled".to_string(),
                health_check_enabled.to_string(),
            );
        }

        if let Some(health_check_interval_seconds) = request.health_check_interval_seconds {
            form_data.insert(
                "HealthCheckIntervalSeconds".to_string(),
                health_check_interval_seconds.to_string(),
            );
        }

        if let Some(health_check_timeout_seconds) = request.health_check_timeout_seconds {
            form_data.insert(
                "HealthCheckTimeoutSeconds".to_string(),
                health_check_timeout_seconds.to_string(),
            );
        }

        if let Some(healthy_threshold_count) = request.healthy_threshold_count {
            form_data.insert(
                "HealthyThresholdCount".to_string(),
                healthy_threshold_count.to_string(),
            );
        }

        if let Some(unhealthy_threshold_count) = request.unhealthy_threshold_count {
            form_data.insert(
                "UnhealthyThresholdCount".to_string(),
                unhealthy_threshold_count.to_string(),
            );
        }

        if let Some(ref matcher) = request.matcher {
            if let Some(ref http_code) = matcher.http_code {
                form_data.insert("Matcher.HttpCode".to_string(), http_code.clone());
            }
        }

        self.send_form(form_data, "ModifyTargetGroup", &request.target_group_arn)
            .await
    }

    async fn modify_target_group_attributes(
        &self,
        request: ModifyTargetGroupAttributesRequest,
    ) -> Result<ModifyTargetGroupAttributesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "ModifyTargetGroupAttributes".to_string(),
        );
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "TargetGroupArn".to_string(),
            request.target_group_arn.clone(),
        );

        for (i, attribute) in request.attributes.iter().enumerate() {
            let index = i + 1;
            form_data.insert(
                format!("Attributes.member.{index}.Key"),
                attribute.key.clone(),
            );
            form_data.insert(
                format!("Attributes.member.{index}.Value"),
                attribute.value.clone(),
            );
        }

        self.send_form(
            form_data,
            "ModifyTargetGroupAttributes",
            &request.target_group_arn,
        )
        .await
    }

    async fn delete_target_group(&self, target_group_arn: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteTargetGroup".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("TargetGroupArn".to_string(), target_group_arn.to_string());

        self.send_form_no_body(form_data, "DeleteTargetGroup", target_group_arn)
            .await
    }

    // ---------------------------------------------------------------------------
    // Target Operations
    // ---------------------------------------------------------------------------

    async fn register_targets(&self, request: RegisterTargetsRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "RegisterTargets".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "TargetGroupArn".to_string(),
            request.target_group_arn.clone(),
        );

        for (i, target) in request.targets.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(format!("Targets.member.{}.Id", idx), target.id.clone());
            if let Some(port) = target.port {
                form_data.insert(format!("Targets.member.{}.Port", idx), port.to_string());
            }
            if let Some(ref az) = target.availability_zone {
                form_data.insert(
                    format!("Targets.member.{}.AvailabilityZone", idx),
                    az.clone(),
                );
            }
        }

        self.send_form_no_body(form_data, "RegisterTargets", &request.target_group_arn)
            .await
    }

    async fn deregister_targets(&self, request: DeregisterTargetsRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeregisterTargets".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "TargetGroupArn".to_string(),
            request.target_group_arn.clone(),
        );

        for (i, target) in request.targets.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(format!("Targets.member.{}.Id", idx), target.id.clone());
            if let Some(port) = target.port {
                form_data.insert(format!("Targets.member.{}.Port", idx), port.to_string());
            }
        }

        self.send_form_no_body(form_data, "DeregisterTargets", &request.target_group_arn)
            .await
    }

    async fn describe_target_health(
        &self,
        request: DescribeTargetHealthRequest,
    ) -> Result<DescribeTargetHealthResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeTargetHealth".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "TargetGroupArn".to_string(),
            request.target_group_arn.clone(),
        );

        if let Some(ref targets) = request.targets {
            for (i, target) in targets.iter().enumerate() {
                let idx = i + 1;
                form_data.insert(format!("Targets.member.{}.Id", idx), target.id.clone());
                if let Some(port) = target.port {
                    form_data.insert(format!("Targets.member.{}.Port", idx), port.to_string());
                }
            }
        }

        self.send_form(form_data, "DescribeTargetHealth", &request.target_group_arn)
            .await
    }

    // ---------------------------------------------------------------------------
    // Listener Operations
    // ---------------------------------------------------------------------------

    async fn create_listener(
        &self,
        request: CreateListenerRequest,
    ) -> Result<CreateListenerResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateListener".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "LoadBalancerArn".to_string(),
            request.load_balancer_arn.clone(),
        );
        form_data.insert("Port".to_string(), request.port.to_string());
        form_data.insert("Protocol".to_string(), request.protocol.clone());

        // Add default actions
        for (i, action) in request.default_actions.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(
                format!("DefaultActions.member.{}.Type", idx),
                action.action_type.clone(),
            );

            if let Some(ref tg_arn) = action.target_group_arn {
                form_data.insert(
                    format!("DefaultActions.member.{}.TargetGroupArn", idx),
                    tg_arn.clone(),
                );
            }

            if let Some(order) = action.order {
                form_data.insert(
                    format!("DefaultActions.member.{}.Order", idx),
                    order.to_string(),
                );
            }

            if let Some(ref forward_config) = action.forward_config {
                if let Some(ref target_groups) = forward_config.target_groups {
                    for (j, tg) in target_groups.iter().enumerate() {
                        let tg_idx = j + 1;
                        form_data.insert(
                            format!("DefaultActions.member.{}.ForwardConfig.TargetGroups.member.{}.TargetGroupArn", idx, tg_idx),
                            tg.target_group_arn.clone(),
                        );
                        if let Some(weight) = tg.weight {
                            form_data.insert(
                                format!("DefaultActions.member.{}.ForwardConfig.TargetGroups.member.{}.Weight", idx, tg_idx),
                                weight.to_string(),
                            );
                        }
                    }
                }
            }

            if let Some(ref redirect_config) = action.redirect_config {
                form_data.insert(
                    format!("DefaultActions.member.{}.RedirectConfig.StatusCode", idx),
                    redirect_config.status_code.clone(),
                );
                if let Some(ref protocol) = redirect_config.protocol {
                    form_data.insert(
                        format!("DefaultActions.member.{}.RedirectConfig.Protocol", idx),
                        protocol.clone(),
                    );
                }
                if let Some(ref port) = redirect_config.port {
                    form_data.insert(
                        format!("DefaultActions.member.{}.RedirectConfig.Port", idx),
                        port.clone(),
                    );
                }
                if let Some(ref host) = redirect_config.host {
                    form_data.insert(
                        format!("DefaultActions.member.{}.RedirectConfig.Host", idx),
                        host.clone(),
                    );
                }
                if let Some(ref path) = redirect_config.path {
                    form_data.insert(
                        format!("DefaultActions.member.{}.RedirectConfig.Path", idx),
                        path.clone(),
                    );
                }
                if let Some(ref query) = redirect_config.query {
                    form_data.insert(
                        format!("DefaultActions.member.{}.RedirectConfig.Query", idx),
                        query.clone(),
                    );
                }
            }

            if let Some(ref fixed_response) = action.fixed_response_config {
                form_data.insert(
                    format!(
                        "DefaultActions.member.{}.FixedResponseConfig.StatusCode",
                        idx
                    ),
                    fixed_response.status_code.clone(),
                );
                if let Some(ref content_type) = fixed_response.content_type {
                    form_data.insert(
                        format!(
                            "DefaultActions.member.{}.FixedResponseConfig.ContentType",
                            idx
                        ),
                        content_type.clone(),
                    );
                }
                if let Some(ref message_body) = fixed_response.message_body {
                    form_data.insert(
                        format!(
                            "DefaultActions.member.{}.FixedResponseConfig.MessageBody",
                            idx
                        ),
                        message_body.clone(),
                    );
                }
            }
        }

        if let Some(ref ssl_policy) = request.ssl_policy {
            form_data.insert("SslPolicy".to_string(), ssl_policy.clone());
        }

        if let Some(ref certificates) = request.certificates {
            for (i, cert) in certificates.iter().enumerate() {
                let idx = i + 1;
                form_data.insert(
                    format!("Certificates.member.{}.CertificateArn", idx),
                    cert.certificate_arn.clone(),
                );
            }
        }

        if let Some(ref alpn_policy) = request.alpn_policy {
            for (i, policy) in alpn_policy.iter().enumerate() {
                form_data.insert(format!("AlpnPolicy.member.{}", i + 1), policy.clone());
            }
        }

        if let Some(ref tags) = request.tags {
            Self::add_tags(&mut form_data, tags);
        }

        self.send_form(form_data, "CreateListener", &request.load_balancer_arn)
            .await
    }

    async fn describe_listeners(
        &self,
        request: DescribeListenersRequest,
    ) -> Result<DescribeListenersResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeListeners".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());

        if let Some(ref lb_arn) = request.load_balancer_arn {
            form_data.insert("LoadBalancerArn".to_string(), lb_arn.clone());
        }

        if let Some(ref listener_arns) = request.listener_arns {
            for (i, arn) in listener_arns.iter().enumerate() {
                form_data.insert(format!("ListenerArns.member.{}", i + 1), arn.clone());
            }
        }

        if let Some(ref marker) = request.marker {
            form_data.insert("Marker".to_string(), marker.clone());
        }

        if let Some(page_size) = request.page_size {
            form_data.insert("PageSize".to_string(), page_size.to_string());
        }

        self.send_form(form_data, "DescribeListeners", "Listener")
            .await
    }

    async fn modify_listener(
        &self,
        request: ModifyListenerRequest,
    ) -> Result<ModifyListenerResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "ModifyListener".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("ListenerArn".to_string(), request.listener_arn.clone());

        if let Some(port) = request.port {
            form_data.insert("Port".to_string(), port.to_string());
        }

        if let Some(ref protocol) = request.protocol {
            form_data.insert("Protocol".to_string(), protocol.clone());
        }

        if let Some(ref ssl_policy) = request.ssl_policy {
            form_data.insert("SslPolicy".to_string(), ssl_policy.clone());
        }

        if let Some(ref certificates) = request.certificates {
            for (i, cert) in certificates.iter().enumerate() {
                let idx = i + 1;
                form_data.insert(
                    format!("Certificates.member.{}.CertificateArn", idx),
                    cert.certificate_arn.clone(),
                );
            }
        }

        if let Some(ref default_actions) = request.default_actions {
            for (i, action) in default_actions.iter().enumerate() {
                let idx = i + 1;
                form_data.insert(
                    format!("DefaultActions.member.{}.Type", idx),
                    action.action_type.clone(),
                );
                if let Some(ref tg_arn) = action.target_group_arn {
                    form_data.insert(
                        format!("DefaultActions.member.{}.TargetGroupArn", idx),
                        tg_arn.clone(),
                    );
                }
            }
        }

        self.send_form(form_data, "ModifyListener", &request.listener_arn)
            .await
    }

    async fn delete_listener(&self, listener_arn: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteListener".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("ListenerArn".to_string(), listener_arn.to_string());

        self.send_form_no_body(form_data, "DeleteListener", listener_arn)
            .await
    }
}
