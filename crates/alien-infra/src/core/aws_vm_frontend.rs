//! Shared frontend preparation for VM workloads, before target registration.

use alien_aws_clients::elbv2::{
    Action, Certificate, CreateListenerRequest, CreateLoadBalancerRequest,
    CreateTargetGroupRequest, Elbv2Api,
};
use alien_core::LoadBalancerEndpoint;
use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use super::{AwsVmLoadBalancer, AwsVmTargetGroup};
use crate::{ErrorData, Result};

/// One independently retained public or private frontend. Its security group
/// belongs to the parent workload and can be shared by both frontends.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsVmFrontend {
    #[serde(default)]
    target_group: AwsVmTargetGroup,
    #[serde(default)]
    load_balancer: AwsVmLoadBalancer,
}

#[cfg(test)]
#[path = "aws_vm_frontend_tests.rs"]
mod tests;

/// Computed from current desired configuration; never persisted as controller
/// state. Certificate references can change without replacing the frontend.
pub struct AwsVmFrontendPlan {
    pub target_group: CreateTargetGroupRequest,
    pub load_balancer: CreateLoadBalancerRequest,
    pub listener_port: i32,
    pub certificate_arn: Option<String>,
    pub ssl_policy: Option<String>,
}

/// Prepared does not mean healthy or published. Register this target group
/// with the retained workload before waiting for health or publishing DNS.
pub struct AwsPreparedVmFrontend {
    pub target_group_arn: String,
    pub load_balancer_arn: String,
    pub listener_arn: String,
    pub endpoint: LoadBalancerEndpoint,
}

impl AwsVmFrontend {
    /// Advances at most one provider mutation. Checkpoint pending results and
    /// keep an existing frontend serving throughout preparation of another.
    pub async fn step_prepare(
        &mut self,
        elb: &dyn Elbv2Api,
        plan: AwsVmFrontendPlan,
    ) -> Result<Option<AwsPreparedVmFrontend>> {
        let https = plan.load_balancer.load_balancer_type.as_deref() == Some("application")
            && plan.target_group.protocol.as_deref() == Some("HTTP")
            && plan
                .certificate_arn
                .as_deref()
                .is_some_and(|arn| !arn.is_empty());
        let tcp = plan.load_balancer.load_balancer_type.as_deref() == Some("network")
            && plan.target_group.protocol.as_deref() == Some("TCP")
            && plan.certificate_arn.is_none()
            && plan.ssl_policy.is_none();
        if (!https && !tcp) || !(1..=65535).contains(&plan.listener_port) {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VM frontend requires HTTP/application/HTTPS with a certificate, or TCP/network without TLS settings, and a valid listener port".to_string(),
                resource_id: None,
            }));
        }
        // Setup boundaries must be known before creating any runtime child.
        // In particular, missing public subnets must not leave a target group.
        super::aws_vm_target_group::prepare_request(plan.target_group.clone())?;
        super::aws_vm_load_balancer::prepare_request(plan.load_balancer.clone())?;
        let Some(target_group_arn) = self
            .target_group
            .step_create(elb, plan.target_group)
            .await?
        else {
            return Ok(None);
        };
        let Some(lb) = self
            .load_balancer
            .step_create(elb, plan.load_balancer)
            .await?
        else {
            return Ok(None);
        };
        // step_create returned only after verifying all three provider fields.
        let load_balancer_arn = lb.load_balancer_arn.expect("confirmed load balancer ARN");
        let endpoint = LoadBalancerEndpoint {
            dns_name: lb.dns_name.expect("confirmed load balancer DNS name"),
            hosted_zone_id: lb.canonical_hosted_zone_id,
        };
        let mut request = CreateListenerRequest::builder()
            .load_balancer_arn(load_balancer_arn.clone())
            .port(plan.listener_port)
            .protocol(if https { "HTTPS" } else { "TCP" }.to_string())
            .default_actions(vec![Action::builder()
                .action_type("forward".to_string())
                .target_group_arn(target_group_arn.clone())
                .build()])
            .build();
        request.certificates = plan.certificate_arn.map(|certificate_arn| {
            vec![Certificate {
                certificate_arn,
                is_default: None,
            }]
        });
        request.ssl_policy = plan.ssl_policy;
        let Some(listener_arn) = self.load_balancer.step_listener(elb, request).await? else {
            return Ok(None);
        };
        Ok(Some(AwsPreparedVmFrontend {
            target_group_arn,
            load_balancer_arn,
            listener_arn,
            endpoint,
        }))
    }

    /// Caller has withdrawn this frontend's DNS and workload registration.
    /// Confirms the LB (and its listener) gone before deleting its target group.
    /// Parent security groups and retained frontends are untouched.
    pub async fn step_delete(&mut self, elb: &dyn Elbv2Api) -> Result<bool> {
        if !self.load_balancer.step_delete(elb).await? {
            return Ok(false);
        }
        self.target_group.step_delete(elb).await
    }

    /// Include these and the desired certificate in the parent's ACM retention
    /// set until every serving/prepared listener has confirmed its rotation.
    pub fn serving_certificate_arns(&self) -> &[String] {
        self.load_balancer.serving_certificate_arns()
    }
}
