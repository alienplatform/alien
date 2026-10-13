//! Private worker ingress assembled from independently recoverable cloud owners.

use std::collections::BTreeSet;

use alien_aws_clients::{
    apigateway::ApiGatewayApi,
    ec2::{
        AuthorizeSecurityGroupIngressRequest, DescribeNetworkInterfacesRequest,
        DescribeSecurityGroupsRequest, DescribeSubnetsRequest, Ec2Api, IpPermission, IpRange,
        UserIdGroupPair,
    },
    ecs::{
        AwsVpcConfiguration, CreateServiceRequest, DeploymentCircuitBreaker,
        DeploymentConfiguration, EcsApi, LoadBalancer, NetworkConfiguration, Tag,
    },
    elbv2::{
        CreateLoadBalancerRequest, CreateTargetGroupRequest, DescribeTargetHealthRequest, ElbTag,
        Elbv2Api, LoadBalancerAttribute, ModifyLoadBalancerAttributesRequest,
        ModifyTargetGroupAttributesRequest, TargetGroupAttribute,
    },
    iam::IamApi,
    lambda::LambdaApi,
};
use alien_core::standard_resource_tags;
use alien_error::{AlienError, Context, ContextError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    aws_gateway_cluster::GatewayCluster,
    aws_gateway_permission::GatewayPermission,
    aws_gateway_config::{bootstrap, GatewaySpec, GATEWAY_HEALTH_PORT, GATEWAY_HTTP_PORT},
    aws_gateway_service::GatewayService,
    aws_gateway_task::GatewayTask,
    aws_private_endpoint::{EndpointPlan, PrivateEndpoint},
    aws_private_rest_api::{PrivateRestApi, RestRoutePlan},
};
use crate::core::aws_tag_scoped::{
    is_remote_not_found, preserve_composite_cleanup_error, CREATE_ATTEMPT_TAG,
};
use crate::{
    AwsPreparedVmFrontend, AwsVmFrontend, AwsVmFrontendPlan, AwsVmSecurityGroup, ErrorData, Result,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PrivateIngressPlan {
    pub account: String,
    pub region: String,
    pub stack: String,
    pub resource: String,
    pub vpc_id: String,
    /// Internal ALB and API interface endpoint subnets.
    pub private_subnets: Vec<String>,
    /// Gateway tasks use public IPs only to pull the pinned public image. Their
    /// ingress group accepts traffic exclusively from the internal ALB.
    pub gateway_subnets: Vec<String>,
    pub domains: Vec<String>,
    pub function_arn: String,
}

pub(super) struct PrivateIngressClients<'a> {
    pub api: &'a dyn ApiGatewayApi,
    pub ec2: &'a dyn Ec2Api,
    pub ecs: &'a dyn EcsApi,
    pub iam: &'a dyn IamApi,
    pub elb: &'a dyn Elbv2Api,
    pub lambda: &'a dyn LambdaApi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PrivateWorkerIngress {
    plan: PrivateIngressPlan,
    generation: String,
    api: Option<PrivateRestApi>,
    #[serde(default)]
    permission: Option<GatewayPermission>,
    endpoint: Option<PrivateEndpoint>,
    cluster: Option<GatewayCluster>,
    task: Option<GatewayTask>,
    service: Option<GatewayService>,
    frontend: AwsVmFrontend,
    frontend_deleted: bool,
    frontend_http_configured: bool,
    frontend_drain_configured: bool,
    lb_group: Option<AwsVmSecurityGroup>,
    gateway_group: Option<AwsVmSecurityGroup>,
    endpoint_group: Option<AwsVmSecurityGroup>,
    gateway_interfaces: BTreeSet<String>,
    retiring: bool,
}

impl PrivateWorkerIngress {
    pub fn new(plan: PrivateIngressPlan) -> Result<Self> {
        for subnets in [&plan.private_subnets, &plan.gateway_subnets] {
            if subnets.len() < 2
                || subnets.iter().any(String::is_empty)
                || subnets.iter().collect::<BTreeSet<_>>().len() != subnets.len()
            {
                return Err(invalid(
                    "Private worker ingress requires distinct subnets in at least two zones",
                ));
            }
        }
        if plan.vpc_id.is_empty()
            || !plan.function_arn.starts_with(&format!(
                "arn:aws:lambda:{}:{}:function:",
                plan.region, plan.account
            ))
        {
            return Err(invalid(
                "Private worker ingress requires a VPC and the retained regional Lambda ARN",
            ));
        }
        bootstrap(GatewaySpec {
            endpoint_dns: "validation.example.test",
            api_id: "validation",
            domains: &plan.domains,
        })?;
        Ok(Self {
            permission: None,
            api: Some(PrivateRestApi::new(
                &plan.account,
                &plan.region,
                &plan.stack,
                &plan.resource,
            )?),
            cluster: Some(GatewayCluster::new(
                &plan.account,
                &plan.region,
                &plan.stack,
                &plan.resource,
            )?),
            lb_group: Some(AwsVmSecurityGroup::new(
                plan.vpc_id.clone(),
                &plan.stack,
                &plan.resource,
            )),
            gateway_group: Some(AwsVmSecurityGroup::new(
                plan.vpc_id.clone(),
                &plan.stack,
                &plan.resource,
            )),
            endpoint_group: Some(AwsVmSecurityGroup::new(
                plan.vpc_id.clone(),
                &plan.stack,
                &plan.resource,
            )),
            plan,
            generation: Uuid::new_v4().to_string(),
            endpoint: None,
            task: None,
            service: None,
            frontend: AwsVmFrontend::default(),
            frontend_deleted: false,
            frontend_http_configured: false,
            frontend_drain_configured: false,
            gateway_interfaces: BTreeSet::new(),
            retiring: false,
        })
    }

    /// Run before any workload or ingress mutation, including retained Lambda
    /// creation. Setup alone owns the ECS service-linked role and VPC topology.
    pub async fn validate_setup(&self, clients: &PrivateIngressClients<'_>) -> Result<()> {
        self.cluster
            .as_ref()
            .ok_or_else(|| invalid("Private gateway cluster receipt is missing"))?
            .validate_setup(clients.iam)
            .await?;
        for selected in [&self.plan.private_subnets, &self.plan.gateway_subnets] {
            let response = clients
                .ec2
                .describe_subnets(
                    DescribeSubnetsRequest::builder()
                        .subnet_ids(selected.clone())
                        .build(),
                )
                .await
                .context(cloud("Read private gateway setup subnets"))?;
            if response.next_token.is_some() {
                return Err(invalid(
                    "Private gateway setup subnet inventory is incomplete",
                ));
            }
            let subnets = response
                .subnet_set
                .ok_or_else(|| invalid("Private gateway setup subnets are missing"))?
                .items;
            let mut ids = BTreeSet::new();
            let mut zones = BTreeSet::new();
            for subnet in subnets {
                let id = subnet
                    .subnet_id
                    .ok_or_else(|| invalid("Private gateway subnet ID is missing"))?;
                if !selected.contains(&id)
                    || !ids.insert(id)
                    || subnet.vpc_id.as_deref() != Some(&self.plan.vpc_id)
                    || subnet.state.as_deref() != Some("available")
                {
                    return Err(invalid(
                        "Private gateway setup subnets changed network, identity or availability",
                    ));
                }
                zones.insert(
                    subnet.availability_zone_id.ok_or_else(|| {
                        invalid("Private gateway subnet physical zone is missing")
                    })?,
                );
            }
            if ids.len() != selected.len() || zones.len() != selected.len() {
                return Err(invalid(
                    "Private gateway requires complete subnet membership in two physical zones",
                ));
            }
        }
        Ok(())
    }

    /// Discover the owned API before preparing its invocation grant and ingress.
    pub async fn step_api(
        &mut self,
        clients: &PrivateIngressClients<'_>,
    ) -> Result<Option<String>> {
        if self.retiring {
            return Err(invalid("Retiring private ingress cannot be reused"));
        }
        self.validate_setup(clients).await?;
        let Some((id, root)) = self
            .api
            .as_mut()
            .ok_or_else(|| invalid("Private REST receipt missing"))?
            .step_create(clients.api)
            .await?
        else {
            return Ok(None);
        };
        self.api
            .as_mut()
            .ok_or_else(|| invalid("Private REST receipt missing"))?
            .configure_routes(RestRoutePlan {
                api_id: id.clone(),
                root_id: root,
                region: self.plan.region.clone(),
                function_arn: self.plan.function_arn.clone(),
            })?;
        Ok(Some(id))
    }

    /// Prepared output is published only after both ECS replicas and their ALB
    /// targets are healthy. The caller owns DNS and the retained Lambda.
    pub async fn step_prepare(
        &mut self,
        clients: &PrivateIngressClients<'_>,
        certificate_arn: &str,
    ) -> Result<Option<AwsPreparedVmFrontend>> {
        if !certificate_arn.starts_with(&format!(
            "arn:aws:acm:{}:{}:certificate/",
            self.plan.region, self.plan.account
        )) {
            return Err(invalid(
                "Private worker TLS requires a certificate in its account and region",
            ));
        }
        let Some(api_id) = self.step_api(clients).await? else {
            return Ok(None);
        };
        if self.permission.is_none() {
            self.permission = Some(GatewayPermission::new(
                &self.plan.function_arn,
                &self.plan.account,
                &self.plan.region,
                &api_id,
            )?);
            return Ok(None);
        }
        if !self.permission.as_mut()
            .ok_or_else(|| invalid("Private invocation permission receipt missing"))?
            .step_prepare(clients.lambda).await?
        {
            return Ok(None);
        }
        let Some(lb_group) = self
            .lb_group
            .as_mut()
            .ok_or_else(|| invalid("Private ALB group receipt missing"))?
            .step_prepare(clients.ec2)
            .await?
        else {
            return Ok(None);
        };
        let Some(gateway_group) = self
            .gateway_group
            .as_mut()
            .ok_or_else(|| invalid("Gateway group receipt missing"))?
            .step_prepare(clients.ec2)
            .await?
        else {
            return Ok(None);
        };
        let Some(endpoint_group) = self
            .endpoint_group
            .as_mut()
            .ok_or_else(|| invalid("Endpoint group receipt missing"))?
            .step_prepare(clients.ec2)
            .await?
        else {
            return Ok(None);
        };
        if !ensure_rule(clients.ec2, &lb_group, 443, 443, None).await?
            || !ensure_rule(
                clients.ec2,
                &gateway_group,
                i32::from(GATEWAY_HTTP_PORT),
                i32::from(GATEWAY_HEALTH_PORT),
                Some(&lb_group),
            )
            .await?
            || !ensure_rule(clients.ec2, &endpoint_group, 443, 443, Some(&gateway_group)).await?
        {
            return Ok(None);
        }
        if self.endpoint.is_none() {
            self.endpoint = Some(PrivateEndpoint::new(EndpointPlan {
                account_id: self.plan.account.clone(),
                vpc_id: self.plan.vpc_id.clone(),
                service_name: format!("com.amazonaws.{}.execute-api", self.plan.region),
                subnet_ids: self.plan.private_subnets.clone(),
                security_group_ids: vec![endpoint_group],
                policy: serde_json::json!({"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"execute-api:Invoke",
                    "Resource":format!("arn:aws:execute-api:{}:{}:{api_id}/*",self.plan.region,self.plan.account)}]}),
                stack_prefix: self.plan.stack.clone(),
                resource_id: self.plan.resource.clone(),
            })?);
            return Ok(None);
        }
        let Some(endpoint) = self
            .endpoint
            .as_mut()
            .ok_or_else(|| invalid("Endpoint receipt missing"))?
            .step_create(clients.ec2)
            .await?
        else {
            return Ok(None);
        };
        let api = self
            .api
            .as_mut()
            .ok_or_else(|| invalid("Private REST receipt missing"))?;
        // REST resource policies are deployed with the stage. Bind the endpoint
        // before taking the first route deployment snapshot.
        if !api
            .step_policy(clients.api, &api_id, &endpoint.vpc_endpoint_id)
            .await?
            || !api.step_routes(clients.api).await?
        {
            return Ok(None);
        }
        let Some(cluster) = self
            .cluster
            .as_mut()
            .ok_or_else(|| invalid("Gateway cluster receipt missing"))?
            .step_create(clients.ecs, clients.iam)
            .await?
        else {
            return Ok(None);
        };
        let tags = standard_resource_tags(&self.plan.stack, &self.plan.resource)
            .into_iter()
            .map(|(key, value)| ElbTag { key, value })
            .collect::<Vec<_>>();
        let name = format!("private-{}", &self.generation[..8]);
        let frontend = self
            .frontend
            .step_prepare(
                clients.elb,
                AwsVmFrontendPlan {
                    target_group: CreateTargetGroupRequest::builder()
                        .name(name.clone())
                        .protocol("HTTP".into())
                        .port(i32::from(GATEWAY_HTTP_PORT))
                        .vpc_id(self.plan.vpc_id.clone())
                        .target_type("ip".into())
                        .health_check_protocol("HTTP".into())
                        .health_check_port(GATEWAY_HEALTH_PORT.to_string())
                        .health_check_path("/".into())
                        .tags(tags.clone())
                        .build(),
                    load_balancer: CreateLoadBalancerRequest::builder()
                        .name(name)
                        .subnets(self.plan.private_subnets.clone())
                        .security_groups(vec![lb_group])
                        .scheme("internal".into())
                        .load_balancer_type("application".into())
                        .ip_address_type("ipv4".into())
                        .tags(tags)
                        .build(),
                    listener_port: 443,
                    certificate_arn: Some(certificate_arn.into()),
                    ssl_policy: Some("ELBSecurityPolicy-TLS13-1-2-2021-06".into()),
                },
            )
            .await?;
        let Some(frontend) = frontend else {
            return Ok(None);
        };
        if !self.configure_frontend(clients.elb, &frontend).await? {
            return Ok(None);
        }
        if self.task.is_none() {
            let suffix = format!(".execute-api.{}.vpce.amazonaws.com", self.plan.region);
            let prefix = format!("{}-", endpoint.vpc_endpoint_id);
            let dns = endpoint
                .dns_entry_set
                .ok_or_else(|| invalid("Private endpoint DNS entries missing"))?
                .items
                .into_iter()
                .map(|entry| entry.dns_name)
                .filter(|name| name.starts_with(&prefix) && name.ends_with(&suffix))
                .min_by_key(|name| (name.len(), name.clone()))
                .ok_or_else(|| invalid("Private endpoint has no owned regional DNS name"))?;
            self.task = Some(GatewayTask::new(
                self.plan.account.clone(),
                self.plan.region.clone(),
                &self.plan.stack,
                &self.plan.resource,
                &bootstrap(GatewaySpec {
                    endpoint_dns: &dns,
                    api_id: &api_id,
                    domains: &self.plan.domains,
                })?,
            )?);
            return Ok(None);
        }
        let Some(task) = self
            .task
            .as_mut()
            .ok_or_else(|| invalid("Gateway task receipt missing"))?
            .step_create(clients.ecs)
            .await?
        else {
            return Ok(None);
        };
        if self.service.is_none() {
            let mut tags = standard_resource_tags(&self.plan.stack, &self.plan.resource);
            tags.insert(CREATE_ATTEMPT_TAG.into(), self.generation.clone());
            self.service = Some(GatewayService::new(
                CreateServiceRequest::builder()
                    .cluster(cluster)
                    .service_name(format!(
                        "{}-{}-gateway-{}",
                        self.plan.stack, self.plan.resource, self.generation
                    ))
                    .client_token(self.generation.clone())
                    .task_definition(task)
                    .desired_count(2)
                    .launch_type("FARGATE".into())
                    .platform_version("1.4.0".into())
                    .network_configuration(NetworkConfiguration {
                        awsvpc_configuration: AwsVpcConfiguration {
                            subnets: self.plan.gateway_subnets.clone(),
                            security_groups: vec![gateway_group],
                            assign_public_ip: "ENABLED".into(),
                        },
                    })
                    .load_balancers(vec![LoadBalancer {
                        target_group_arn: frontend.target_group_arn.clone(),
                        container_name: "gateway".into(),
                        container_port: i32::from(GATEWAY_HTTP_PORT),
                    }])
                    .deployment_configuration(
                        DeploymentConfiguration::builder()
                            .maximum_percent(200)
                            .minimum_healthy_percent(100)
                            .deployment_circuit_breaker(DeploymentCircuitBreaker {
                                enable: true,
                                rollback: false,
                            })
                            .build(),
                    )
                    .health_check_grace_period_seconds(60)
                    .tags(
                        tags.into_iter()
                            .map(|(key, value)| Tag { key, value })
                            .collect(),
                    )
                    .propagate_tags("SERVICE".into())
                    .enable_ecs_managed_tags(true)
                    .build(),
            )?);
            return Ok(None);
        }
        let service = self
            .service
            .as_mut()
            .ok_or_else(|| invalid("Gateway service receipt missing"))?;
        let ready = service.step_create(clients.ecs).await;
        self.gateway_interfaces
            .extend(service.network_interface_ids.iter().cloned());
        let Some(interfaces) = ready? else {
            return Ok(None);
        };
        let expected_addresses = interfaces
            .values()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if expected_addresses.len() != 2 {
            return Err(invalid(
                "Gateway replicas do not have distinct private addresses",
            ));
        }
        let health = clients
            .elb
            .describe_target_health(
                DescribeTargetHealthRequest::builder()
                    .target_group_arn(frontend.target_group_arn.clone())
                    .build(),
            )
            .await
            .context(cloud("Read private gateway target health"))?;
        let Some(targets) = health
            .describe_target_health_result
            .target_health_descriptions
        else {
            return Ok(None);
        };
        let addresses = targets
            .members
            .iter()
            .filter_map(|description| {
                description
                    .target
                    .as_ref()
                    .and_then(|target| target.id.as_deref())
            })
            .collect::<BTreeSet<_>>();
        if targets.members.len() != 2
            || addresses != expected_addresses
            || targets.members.iter().any(|description| {
                description
                    .target_health
                    .as_ref()
                    .and_then(|health| health.state.as_deref())
                    != Some("healthy")
                    || description.target.as_ref().and_then(|target| target.port)
                        != Some(i32::from(GATEWAY_HTTP_PORT))
                    || description.health_check_port.as_deref()
                        != Some(GATEWAY_HEALTH_PORT.to_string().as_str())
            })
        {
            return Ok(None);
        }
        Ok(Some(frontend))
    }

    async fn configure_frontend(
        &mut self,
        elb: &dyn Elbv2Api,
        frontend: &AwsPreparedVmFrontend,
    ) -> Result<bool> {
        if !self.frontend_http_configured {
            elb.modify_load_balancer_attributes(
                ModifyLoadBalancerAttributesRequest::builder()
                    .load_balancer_arn(frontend.load_balancer_arn.clone())
                    .attributes(vec![
                        LoadBalancerAttribute {
                            key: "routing.http.preserve_host_header.enabled".into(),
                            value: "true".into(),
                        },
                        LoadBalancerAttribute {
                            key: "idle_timeout.timeout_seconds".into(),
                            value: "4000".into(),
                        },
                    ])
                    .build(),
            )
            .await
            .context(cloud("Preserve worker Host and streaming idle duration"))?;
            self.frontend_http_configured = true;
            return Ok(false);
        }
        if !self.frontend_drain_configured {
            elb.modify_target_group_attributes(
                ModifyTargetGroupAttributesRequest::builder()
                    .target_group_arn(frontend.target_group_arn.clone())
                    .attributes(vec![TargetGroupAttribute {
                        key: "deregistration_delay.timeout_seconds".into(),
                        value: "900".into(),
                    }])
                    .build(),
            )
            .await
            .context(cloud(
                "Drain gateway targets for the Lambda response duration",
            ))?;
            self.frontend_drain_configured = true;
            return Ok(false);
        }
        Ok(true)
    }

    pub async fn step_delete(&mut self, clients: &PrivateIngressClients<'_>) -> Result<bool> {
        self.delete_inner(clients)
            .await
            .map_err(preserve_composite_cleanup_error)
    }

    async fn delete_inner(&mut self, clients: &PrivateIngressClients<'_>) -> Result<bool> {
        self.retiring = true;
        if let Some(service) = &mut self.service {
            let deleted = service.step_delete(clients.ecs).await;
            self.gateway_interfaces
                .extend(service.network_interface_ids.iter().cloned());
            if !deleted? {
                return Ok(false);
            }
            self.service = None;
            return Ok(false);
        }
        if !self.frontend_deleted {
            if !self.frontend.step_delete(clients.elb).await? {
                return Ok(false);
            }
            self.frontend_deleted = true;
            return Ok(false);
        }
        for id in &self.gateway_interfaces {
            match clients
                .ec2
                .describe_network_interfaces(
                    DescribeNetworkInterfacesRequest::builder()
                        .network_interface_ids(vec![id.clone()])
                        .build(),
                )
                .await
            {
                Err(error) if is_remote_not_found(&error) => {}
                Err(error) => return Err(error.context(cloud("Confirm gateway interface removal"))),
                Ok(response)
                    if response.next_token.is_none()
                        && response
                            .network_interface_set
                            .as_ref()
                            .is_some_and(|set| set.items.is_empty()) => {}
                Ok(_) => return Ok(false),
            }
        }
        if let Some(endpoint) = &mut self.endpoint {
            if !endpoint.step_delete(clients.ec2).await? {
                return Ok(false);
            }
            self.endpoint = None;
            return Ok(false);
        }
        if let Some(permission) = &mut self.permission {
            if !permission.step_delete(clients.lambda).await? {
                return Ok(false);
            }
            self.permission = None;
            return Ok(false);
        }
        if let Some(api) = &mut self.api {
            if !api.step_delete(clients.api).await? {
                return Ok(false);
            }
            self.api = None;
            return Ok(false);
        }
        // Destination groups refer to source groups; retire in that order.
        for group in [
            &mut self.endpoint_group,
            &mut self.gateway_group,
            &mut self.lb_group,
        ] {
            if let Some(owner) = group {
                if !owner.step_delete(clients.ec2).await? {
                    return Ok(false);
                }
                *group = None;
                return Ok(false);
            }
        }
        if let Some(task) = &mut self.task {
            if !task.step_delete(clients.ecs).await? {
                return Ok(false);
            }
            self.task = None;
            return Ok(false);
        }
        if let Some(cluster) = &mut self.cluster {
            if !cluster.step_delete(clients.ecs, clients.iam).await? {
                return Ok(false);
            }
            self.cluster = None;
            return Ok(false);
        }
        Ok(true)
    }
}

/// Every group has one exact ingress rule. Refuse drift rather than accepting a
/// broader source or a second rule. Lost authorize replies recover by rereading.
async fn ensure_rule(
    ec2: &dyn Ec2Api,
    group: &str,
    from: i32,
    to: i32,
    source: Option<&str>,
) -> Result<bool> {
    let response = ec2
        .describe_security_groups(
            DescribeSecurityGroupsRequest::builder()
                .group_ids(vec![group.into()])
                .build(),
        )
        .await
        .context(cloud("Read gateway ingress rule"))?;
    if response.next_token.is_some() {
        return Err(invalid("Gateway ingress group response is incomplete"));
    }
    let groups = response
        .security_group_info
        .ok_or_else(|| invalid("Gateway ingress group is missing"))?
        .items;
    if groups.len() != 1 || groups[0].group_id.as_deref() != Some(group) {
        return Err(invalid("Gateway ingress group identity changed"));
    }
    let rules = groups[0]
        .ip_permissions
        .as_ref()
        .map(|set| set.items.as_slice())
        .unwrap_or_default();
    if !rules.is_empty() {
        if rules.len() != 1 {
            return Err(invalid("Gateway ingress group has unexpected rules"));
        }
        let rule = &rules[0];
        let ranges = rule
            .ip_ranges
            .as_ref()
            .map(|set| set.items.as_slice())
            .unwrap_or_default();
        let groups = rule
            .groups
            .as_ref()
            .map(|set| set.items.as_slice())
            .unwrap_or_default();
        let source_matches = match source {
            Some(id) => {
                ranges.is_empty() && groups.len() == 1 && groups[0].group_id.as_deref() == Some(id)
            }
            None => {
                groups.is_empty()
                    && ranges.len() == 1
                    && ranges[0].cidr_ip.as_deref() == Some("0.0.0.0/0")
            }
        };
        if rule.ip_protocol.as_deref() != Some("tcp")
            || rule.from_port != Some(from)
            || rule.to_port != Some(to)
            || !source_matches
            || rule
                .ipv6_ranges
                .as_ref()
                .is_some_and(|set| !set.items.is_empty())
            || rule
                .prefix_list_ids
                .as_ref()
                .is_some_and(|set| !set.items.is_empty())
        {
            return Err(invalid("Gateway ingress group source or port changed"));
        }
        return Ok(true);
    }
    ec2.authorize_security_group_ingress(
        AuthorizeSecurityGroupIngressRequest::builder()
            .group_id(group.into())
            .ip_permissions(vec![IpPermission {
                ip_protocol: "tcp".into(),
                from_port: Some(from),
                to_port: Some(to),
                ipv6_ranges: None,
                ip_ranges: source.is_none().then(|| {
                    vec![IpRange {
                        cidr_ip: "0.0.0.0/0".into(),
                        description: None,
                    }]
                }),
                user_id_group_pairs: source.map(|id| {
                    vec![UserIdGroupPair {
                        group_id: Some(id.into()),
                        user_id: None,
                        description: None,
                    }]
                }),
            }])
            .build(),
    )
    .await
    .context(cloud("Authorize exact gateway ingress rule"))?;
    Ok(false)
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.into(),
        resource_id: None,
    })
}
fn cloud(message: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: message.into(),
        resource_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::{
        apigateway::MockApiGatewayApi, ec2::MockEc2Api, ecs::MockEcsApi, elbv2::MockElbv2Api,
        iam::MockIamApi, lambda::MockLambdaApi,
    };
    use alien_client_core::ErrorData as ClientError;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn plan() -> PrivateIngressPlan {
        PrivateIngressPlan {
            account: "123456789012".into(),
            region: "us-east-1".into(),
            stack: "stack".into(),
            resource: "worker".into(),
            vpc_id: "vpc-owned".into(),
            private_subnets: vec!["subnet-private-a".into(), "subnet-private-b".into()],
            gateway_subnets: vec!["subnet-public-a".into(), "subnet-public-b".into()],
            domains: vec!["private-api.example.test".into()],
            function_arn: "arn:aws:lambda:us-east-1:123456789012:function:worker".into(),
        }
    }

    #[tokio::test]
    async fn setup_role_and_subnet_failures_precede_every_ingress_mutation() {
        for scenario in [
            "missing-role",
            "wrong-vpc",
            "duplicate-zone",
            "incomplete-page",
        ] {
            let mut ingress = PrivateWorkerIngress::new(plan()).unwrap();
            let api = MockApiGatewayApi::new();
            let ecs = MockEcsApi::new();
            let elb = MockElbv2Api::new();
            let lambda = MockLambdaApi::new();
            let mut iam = MockIamApi::new();
            let mut ec2 = MockEc2Api::new();
            iam.expect_get_role().times(1).returning(move |name| {
                assert_eq!(name,"AWSServiceRoleForECS");
                if scenario=="missing-role" { return Err(AlienError::new(ClientError::RemoteResourceNotFound {resource_type:"role".into(),resource_name:name.into()})); }
                Ok(serde_json::from_value(json!({"GetRoleResult":{"Role":{
                    "Path":"/aws-service-role/ecs.amazonaws.com/","RoleName":"AWSServiceRoleForECS","RoleId":"role-owned",
                    "Arn":"arn:aws:iam::123456789012:role/aws-service-role/ecs.amazonaws.com/AWSServiceRoleForECS","CreateDate":"2026-01-01T00:00:00Z"
                }}})).unwrap())
            });
            if scenario != "missing-role" {
                ec2.expect_describe_subnets().times(1).returning(move |request| {
                    let ids=request.subnet_ids.unwrap();
                    Ok(serde_json::from_value(json!({"subnetSet":{"item":ids.iter().enumerate().map(|(index,id)|json!({
                        "subnetId":id,"vpcId":if scenario=="wrong-vpc" {"vpc-foreign"} else {"vpc-owned"},"state":"available",
                        "availabilityZoneId":if scenario=="duplicate-zone" {"use1-az1".into()} else {format!("use1-az{index}")}
                    })).collect::<Vec<_>>()},"nextToken":if scenario=="incomplete-page" {Some("more")} else {None}})).unwrap())
                });
            }
            let clients = PrivateIngressClients {
                api: &api,
                ec2: &ec2,
                ecs: &ecs,
                iam: &iam,
                elb: &elb,
                lambda: &lambda,
            };
            assert!(ingress.step_api(&clients).await.is_err(), "{scenario}");
            assert!(
                !serde_json::to_value(&ingress).unwrap()["api"]["createPermitted"]
                    .as_bool()
                    .unwrap()
            );
        }
    }

    #[tokio::test]
    async fn security_rule_lost_reply_recovers_but_broader_sources_are_refused() {
        let installed = Arc::new(Mutex::new(false));
        let mut ec2 = MockEc2Api::new();
        let world = installed.clone();
        ec2.expect_describe_security_groups().returning(move |request| {
            assert_eq!(request.group_ids,Some(vec!["sg-gateway".into()]));
            let rules=if *world.lock().unwrap() {vec![json!({"ipProtocol":"tcp","fromPort":8080,"toPort":8081,"groups":{"item":[{"groupId":"sg-alb"}]}})]} else {vec![]};
            Ok(serde_json::from_value(json!({"securityGroupInfo":{"item":[{"groupId":"sg-gateway","ipPermissions":{"item":rules}}]}})).unwrap())
        });
        let world = installed.clone();
        ec2.expect_authorize_security_group_ingress()
            .times(1)
            .returning(move |request| {
                assert_eq!(request.group_id, "sg-gateway");
                let rule = &request.ip_permissions[0];
                assert_eq!(rule.from_port, Some(8080));
                assert_eq!(rule.to_port, Some(8081));
                assert!(rule.ip_ranges.is_none());
                assert_eq!(
                    rule.user_id_group_pairs.as_ref().unwrap()[0]
                        .group_id
                        .as_deref(),
                    Some("sg-alb")
                );
                *world.lock().unwrap() = true;
                Err(AlienError::new(ClientError::Timeout {
                    message: "authorize committed; reply lost".into(),
                }))
            });
        assert!(ensure_rule(&ec2, "sg-gateway", 8080, 8081, Some("sg-alb"))
            .await
            .is_err());
        assert!(ensure_rule(&ec2, "sg-gateway", 8080, 8081, Some("sg-alb"))
            .await
            .unwrap());
        assert!(
            ensure_rule(&ec2, "sg-gateway", 8080, 8081, Some("sg-other"))
                .await
                .is_err()
        );
        assert!(ensure_rule(&ec2, "sg-gateway", 8080, 8081, None)
            .await
            .is_err());
    }
    #[tokio::test]
    async fn stream_attribute_updates_replay_after_lost_replies_and_checkpoint_independently() {
        let mut ingress = PrivateWorkerIngress::new(plan()).unwrap();
        let frontend = AwsPreparedVmFrontend {
            target_group_arn: "target-owned".into(),
            load_balancer_arn: "lb-owned".into(),
            listener_arn: "listener-owned".into(),
            endpoint: alien_core::LoadBalancerEndpoint {
                dns_name: "private.example.test".into(),
                hosted_zone_id: None,
            },
        };
        let mut elb = MockElbv2Api::new();
        let calls = Arc::new(Mutex::new((0, 0)));
        let world = calls.clone();
        elb.expect_modify_load_balancer_attributes()
            .times(2)
            .returning(move |request| {
                assert_eq!(request.load_balancer_arn, "lb-owned");
                assert_eq!(
                    request
                        .attributes
                        .iter()
                        .map(|attribute| (attribute.key.as_str(), attribute.value.as_str()))
                        .collect::<Vec<_>>(),
                    vec![
                        ("routing.http.preserve_host_header.enabled", "true"),
                        ("idle_timeout.timeout_seconds", "4000")
                    ]
                );
                let mut calls = world.lock().unwrap();
                calls.0 += 1;
                assert_eq!(calls.1, 0, "drain update must follow the HTTP checkpoint");
                if calls.0 == 1 {
                    return Err(AlienError::new(ClientError::Timeout {
                        message: "HTTP attributes committed; reply lost".into(),
                    }));
                }
                Ok(
                    serde_json::from_value(json!({"ModifyLoadBalancerAttributesResult":{}}))
                        .unwrap(),
                )
            });
        let world = calls.clone();
        elb.expect_modify_target_group_attributes()
            .times(2)
            .returning(move |request| {
                assert_eq!(request.target_group_arn, "target-owned");
                assert_eq!(
                    request
                        .attributes
                        .iter()
                        .map(|attribute| (attribute.key.as_str(), attribute.value.as_str()))
                        .collect::<Vec<_>>(),
                    vec![("deregistration_delay.timeout_seconds", "900")]
                );
                let mut calls = world.lock().unwrap();
                calls.1 += 1;
                assert_eq!(calls.0, 2);
                if calls.1 == 1 {
                    return Err(AlienError::new(ClientError::Timeout {
                        message: "drain attributes committed; reply lost".into(),
                    }));
                }
                Ok(
                    serde_json::from_value(json!({"ModifyTargetGroupAttributesResult":{}}))
                        .unwrap(),
                )
            });
        for expected in [None, Some(false), None, Some(false), Some(true)] {
            let result = ingress.configure_frontend(&elb, &frontend).await;
            if let Some(value) = expected {
                assert_eq!(result.unwrap(), value);
            } else {
                assert!(result.unwrap_err().retryable);
            }
            ingress = serde_json::from_value(serde_json::to_value(ingress).unwrap()).unwrap();
        }
        assert_eq!(*calls.lock().unwrap(), (2, 2));
    }
}
