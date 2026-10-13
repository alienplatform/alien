use alien_aws_clients::elbv2::{
    CreateListenerRequest, DescribeListenersRequest, ModifyListenerRequest,
};

use super::*;

/// The load balancer's persisted attempt also owns its single forwarding
/// listener; another ownership/discovery mechanism is unnecessary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ListenerReceipt {
    fingerprint: String,
    arn: Option<String>,
    certificate_arns: Vec<String>,
}

impl AwsVmLoadBalancer {
    /// Prepares one HTTPS or TCP forwarding listener without waiting for target
    /// health. The caller must register its target group with the workload next.
    /// Certificate rotation preserves the listener ARN and forwarding action.
    /// Retain both the desired certificate and `serving_certificate_arns()`
    /// until a separate read confirms the rotation.
    pub async fn step_listener(
        &mut self,
        elb: &dyn Elbv2Api,
        mut request: CreateListenerRequest,
    ) -> Result<Option<String>> {
        let lb_arn = self
            .arn
            .as_ref()
            .ok_or_else(|| invalid("Prepare the load balancer before its listener"))?;
        if &request.load_balancer_arn != lb_arn
            || !(1..=65535).contains(&request.port)
            || !matches!(request.protocol.as_str(), "HTTPS" | "TCP")
            || request.tags.is_some()
            || request.default_actions.len() != 1
        {
            return Err(invalid("VM listener requires its owned load balancer, a valid port, HTTPS/TCP, and one forwarding action"));
        }
        let action = &request.default_actions[0];
        if action.action_type != "forward"
            || action.target_group_arn.as_deref().is_none_or(str::is_empty)
            || action.forward_config.is_some()
            || action.redirect_config.is_some()
            || action.fixed_response_config.is_some()
        {
            return Err(invalid(
                "VM listener must forward directly to its prepared target group",
            ));
        }
        let certificates = request.certificates.take();
        let desired_certificates = certificates
            .as_ref()
            .map(|certificates| {
                certificates
                    .iter()
                    .map(|certificate| certificate.certificate_arn.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if (request.protocol == "HTTPS"
            && (desired_certificates.len() != 1 || desired_certificates[0].is_empty()))
            || (request.protocol == "TCP"
                && (!desired_certificates.is_empty()
                    || request.ssl_policy.is_some()
                    || request.alpn_policy.is_some()))
        {
            return Err(invalid(
                "HTTPS listener requires one certificate; TCP listener cannot use TLS settings",
            ));
        }
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&request)
                    .into_alien_error()
                    .context(config_data("Encode VM listener configuration"))?
            )
        );
        if let Some(receipt) = &self.listener {
            if receipt.fingerprint != fingerprint {
                return Err(invalid(
                    "Listener forwarding configuration changed during its lifetime",
                ));
            }
        } else {
            self.listener = Some(ListenerReceipt {
                fingerprint,
                arn: None,
                certificate_arns: vec![],
            });
            return Ok(None);
        }
        // Refuse even a correctly tagged listener if its parent was replaced.
        let lb = self
            .find(elb)
            .await?
            .ok_or_else(|| invalid("Listener's recorded load balancer disappeared"))?;
        if required_arn(&lb)? != *lb_arn {
            return Err(invalid("Listener's recorded load balancer was replaced"));
        }
        self.verify_owned(elb, lb_arn).await?;
        let response = elb
            .describe_listeners(
                DescribeListenersRequest::builder()
                    .load_balancer_arn(lb_arn.clone())
                    .build(),
            )
            .await
            .context(cloud_data("Discover VM frontend listener"))?
            .describe_listeners_result;
        if response.next_marker.is_some() {
            return Err(invalid(
                "Listener discovery is incomplete; refusing creation or adoption",
            ));
        }
        let mut listeners = response
            .listeners
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if listeners.iter().any(|listener| listener.port.is_none()) {
            return Err(invalid(
                "Listener discovery returned an entry without a port",
            ));
        }
        listeners.retain(|listener| listener.port == Some(request.port));
        if listeners.len() > 1 {
            return Err(invalid("Listener port discovery is ambiguous"));
        }
        if let Some(listener) = listeners.pop() {
            let arn = listener
                .listener_arn
                .clone()
                .filter(|arn| !arn.is_empty())
                .ok_or_else(|| invalid("Listener has no provider ARN"))?;
            self.verify_owned(elb, &arn).await?;
            let actions = listener
                .default_actions
                .as_ref()
                .map(|wrapper| wrapper.members.as_slice())
                .unwrap_or_default();
            if listener.load_balancer_arn.as_ref() != Some(lb_arn)
                || listener.protocol.as_deref() != Some(request.protocol.as_str())
                || request
                    .ssl_policy
                    .as_ref()
                    .is_some_and(|policy| listener.ssl_policy.as_ref() != Some(policy))
                || request.alpn_policy.as_ref().is_some_and(|policy| {
                    listener.alpn_policy.as_ref().map(|actual| &actual.members) != Some(policy)
                })
                || actions.len() != 1
                || actions[0].action_type.as_deref() != Some("forward")
                || actions[0].target_group_arn != action.target_group_arn
                || actions[0].forward_config.as_ref().is_some_and(|forward| {
                    forward.target_groups.as_ref().is_none_or(|groups| {
                        groups.members.len() != 1
                            || groups.members[0].target_group_arn != action.target_group_arn
                            || groups.members[0].weight.is_some_and(|weight| weight <= 0)
                    })
                })
                || actions[0].redirect_config.is_some()
                || actions[0].fixed_response_config.is_some()
            {
                return Err(invalid(
                    "Discovered listener does not match its recorded forwarding plan",
                ));
            }
            let receipt = self
                .listener
                .as_mut()
                .expect("listener intent was recorded");
            if let Some(recorded) = &receipt.arn {
                if recorded != &arn {
                    return Err(invalid("Recorded VM listener was replaced"));
                }
            } else {
                receipt.arn = Some(arn);
                return Ok(None);
            }
            let actual_certificates: Vec<String> = listener
                .certificates
                .as_ref()
                .map(|wrapper| {
                    wrapper
                        .members
                        .iter()
                        .filter_map(|certificate| certificate.certificate_arn.clone())
                        .collect()
                })
                .unwrap_or_default();
            if actual_certificates != desired_certificates {
                if request.protocol != "HTTPS" {
                    return Err(invalid("TCP listener unexpectedly has a certificate"));
                }
                elb.modify_listener(
                    ModifyListenerRequest::builder()
                        .listener_arn(arn)
                        .certificates(certificates.expect("HTTPS certificate was validated"))
                        .build(),
                )
                .await
                .context(cloud_data("Rotate VM frontend listener certificate"))?;
                return Ok(None);
            }
            receipt.certificate_arns = actual_certificates;
            return Ok(Some(arn));
        }
        if self
            .listener
            .as_ref()
            .is_some_and(|receipt| receipt.arn.is_some())
        {
            return Err(invalid(
                "Recorded VM listener disappeared; refusing implicit replacement",
            ));
        }
        request.certificates = certificates;
        request.tags = Some(
            self.ownership_tags
                .iter()
                .map(|(key, value)| ElbTag {
                    key: key.clone(),
                    value: value.clone(),
                })
                .chain(std::iter::once(ElbTag {
                    key: CREATE_ATTEMPT_TAG.to_string(),
                    value: self
                        .token
                        .clone()
                        .ok_or_else(|| invalid("Load balancer attempt token is missing"))?,
                }))
                .collect(),
        );
        let response = elb
            .create_listener(request)
            .await
            .context(cloud_data("Create VM frontend listener"))?;
        let listeners = response
            .create_listener_result
            .listeners
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if listeners.len() != 1 {
            return Err(invalid(
                "CreateListener did not return exactly one listener",
            ));
        }
        self.listener
            .as_mut()
            .expect("listener intent was recorded")
            .arn = Some(
            listeners[0]
                .listener_arn
                .clone()
                .filter(|arn| !arn.is_empty())
                .ok_or_else(|| invalid("Created listener has no provider ARN"))?,
        );
        Ok(None)
    }

    /// Last read-confirmed certificate references, retained through an uncertain
    /// listener update. Empty until a listener has actually been confirmed.
    pub fn serving_certificate_arns(&self) -> &[String] {
        self.listener
            .as_ref()
            .map(|receipt| receipt.certificate_arns.as_slice())
            .unwrap_or_default()
    }
}
